use crate::ast::{Assignment, Expression};
use crate::stdlib::EventType;
use crate::transpiler::storage_paths::StorageValueOps;
use crate::transpiler::{escape_quoted, TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn generate_function_call(
        &mut self,
        func_name: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let plain_call = format!("function {}:{}", namespace, func_name);

        let Some(param_names) = self.functions.params.get(func_name).cloned() else {
            let commands = self.preserving_args(plain_call);
            self.extend_current_function(commands);
            return Ok(());
        };
        // A function without parameters takes no arguments either.
        if args.len() != param_names.len() {
            return Err(format!(
                "Function '{}' expects {} argument(s), but {} provided.\n\
                Expected parameters: ({})",
                func_name,
                param_names.len(),
                args.len(),
                param_names.join(", ")
            )
            .into());
        }
        if param_names.is_empty() {
            let commands = self.preserving_args(plain_call);
            self.extend_current_function(commands);
            return Ok(());
        }

        let staging = if self.in_recursive_function() {
            ArgumentStaging::Stack
        } else {
            ArgumentStaging::Site(self.next_temp_id())
        };
        let mut argument_commands = Vec::new();
        for (param_name, arg) in param_names.iter().zip(args) {
            let target_path = staging.argument_path(param_name);
            argument_commands.extend(self.call_argument_commands(
                func_name,
                param_name,
                &target_path,
                arg,
            )?);
        }
        let commands = staging.wrap(
            &namespace,
            !self.current_context.params.is_empty(),
            argument_commands,
            format!(
                "function {}:{} with storage {}:global args",
                namespace, func_name, namespace
            ),
        );
        self.extend_current_function(commands);
        Ok(())
    }

    /// A call to a function without parameters. The callee can still call
    /// functions that have some, which replaces `args`, so a caller with
    /// parameters keeps its `args` around the call.
    fn preserving_args(&mut self, call: String) -> Vec<String> {
        if self.current_context.params.is_empty() {
            return vec![call];
        }
        let staging = if self.in_recursive_function() {
            ArgumentStaging::Stack
        } else {
            ArgumentStaging::Site(self.next_temp_id())
        };
        staging.preserve(&self.data_pack.namespace, call)
    }

    pub(in crate::transpiler) fn extend_current_function(&mut self, commands: Vec<String>) {
        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }
    }

    /// Commands that store one argument of a call statement at `target_path`.
    /// An arithmetic or condition argument is computed into a scratch score
    /// first, and those commands go straight into the current function.
    fn call_argument_commands(
        &mut self,
        func_name: &str,
        param_name: &str,
        target_path: &str,
        arg: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let set_value = |value: &dyn std::fmt::Display| {
            vec![format!(
                "data modify storage {}:global {} set value {}",
                namespace, target_path, value
            )]
        };
        let set_from_storage = |path: &str| {
            vec![format!(
                "data modify storage {}:global {} set from storage {}:global {}",
                namespace, target_path, namespace, path
            )]
        };
        let store_score = |holder: &str, objective: &str| {
            vec![format!(
                "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                namespace, target_path, holder, objective
            )]
        };
        match arg {
            Expression::String(s) => Ok(set_value(&format!("\"{}\"", escape_quoted(s)))),
            Expression::Number(n) => Ok(set_value(&(*n as i32))),
            Expression::Boolean(b) => Ok(set_value(&i32::from(*b))),
            // A selector is passed as text.
            Expression::Identifier(var) if var.starts_with('@') => {
                Ok(set_value(&format!("\"{}\"", var)))
            }
            Expression::Identifier(var) if self.is_current_macro_param(var) => {
                Ok(set_from_storage(&format!("args.{}", var)))
            }
            // A storage-backed string, list, or map passes its value, not
            // the variable's name.
            Expression::Identifier(var) if self.symbols.variable_storage_paths.contains_key(var) => {
                Ok(set_from_storage(&self.symbols.variable_storage_paths[var]))
            }
            Expression::Identifier(var)
                if self.symbols.scoreboard_variables.contains(var)
                    || self.symbols.variable_objectives.contains_key(var) =>
            {
                let objective = self
                    .symbols
                    .variable_objectives
                    .get(var)
                    .map(String::as_str)
                    .unwrap_or("temp");
                Ok(store_score(var, objective))
            }
            Expression::Identifier(var) if self.symbols.variables.contains_key(var) => {
                match self.symbols.compile_time_constants.get(var) {
                    Some(value) => Ok(set_value(value)),
                    None => Ok(set_value(&format!("\"{}\"", var))),
                }
            }
            // Passing an unknown name through as a string would hide typos
            // from library callers. The CLI reports `undefined-variable`
            // before this point.
            Expression::Identifier(var) => Err(format!(
                "Unknown identifier '{}' passed as argument to function '{}'. Define the variable first, or quote it (\"{}\") to pass a string literal.",
                var, func_name, var
            )
            .into()),
            Expression::Array(_) | Expression::Map(_) => {
                self.storage_value_commands(target_path, arg)
            }
            Expression::Subscript(base, index) if self.string_variable_path(base).is_some() => {
                let base_path = self.string_variable_path(base).unwrap_or_default();
                self.string_char_commands(target_path, &base_path, index)
            }
            Expression::Binary(..)
            | Expression::Unary(..)
            | Expression::Call(..)
            | Expression::Attribute(..)
            | Expression::Subscript(..) => {
                // The scratch score is unique per call, so calls passing
                // differently typed values (`f(x + 1)`, then `f(a > b)`)
                // never share a slot.
                let temp_var = format!("#arg_temp_{}", self.next_temp_id());
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(temp_var.clone(), "temp".to_string());
                self.symbols.scoreboard_variables.insert(temp_var.clone());
                self.process_assignment(&Assignment {
                    target: temp_var.clone(),
                    target_expr: None,
                    value: arg.clone(),
                    persist: false,
                })?;
                Ok(store_score(&temp_var, "temp"))
            }
            _ => Err(format!(
                "Function parameter does not support this expression type.\n\
                 \n\
                 Parameter '{}' received: {:?}\n\
                 \n\
                 Supported parameter types:\n\
                 - Literal numbers: func(42)\n\
                 - Literal strings: func(\"text\")\n\
                 - Boolean values: func(True), func(False)\n\
                 - Lists and maps: func([1, 2]), func({{\"a\": 1}})\n\
                 - Variables: func(my_var)\n\
                 - Arithmetic expressions: func(x + 10)",
                param_name, arg
            )
            .into()),
        }
    }

    pub(in crate::transpiler) fn process_add_event_listener(
        &mut self,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err("addEventListener requires 2 arguments: (event, handler)"
                .to_string()
                .into());
        }

        // Extract event type
        let event_type = match &args[0] {
            Expression::Attribute(obj, attr) => {
                // Handle event.LOAD, event.TICK
                if let Expression::Identifier(module) = &**obj {
                    if module == "event" {
                        match attr.as_str() {
                            "LOAD" => EventType::Load,
                            "TICK" => EventType::Tick,
                            _ => return Err(format!("Unknown event type: {}", attr).into()),
                        }
                    } else {
                        return Err(format!("Unknown module: {}", module).into());
                    }
                } else {
                    return Err("Event must be from 'event' module".into());
                }
            }
            _ => {
                return Err("First argument must be an event (e.g., event.LOAD)"
                    .to_string()
                    .into())
            }
        };

        // Extract handler function name
        let handler_name = match &args[1] {
            Expression::Identifier(name) => name.clone(),
            _ => return Err("Second argument must be a function name".into()),
        };

        // Register the event listener
        self.data_pack
            .stdlib
            .add_event_listener(event_type, handler_name);

        Ok(())
    }
}

/// Where a call's arguments are built before they become `args`. See
/// `docs/src/reference/calling-convention.md`.
pub(in crate::transpiler) enum ArgumentStaging {
    /// A path of the call site's own, `_call_args_<id>`. A caller with
    /// parameters keeps its `args` in `_caller_args_<id>` during the call.
    Site(u32),
    /// The top of `call_stack`. Calls inside a recursive function use it,
    /// since one call site can then be running more than once at a time.
    Stack,
}

impl ArgumentStaging {
    /// The storage path one argument is written to.
    pub(in crate::transpiler) fn argument_path(&self, param: &str) -> String {
        match self {
            Self::Site(id) => format!("_call_args_{}.{}", id, param),
            Self::Stack => format!("call_stack[-1].{}", param),
        }
    }

    /// The commands around a call to a function without parameters from a
    /// caller with parameters: the caller's `args` are kept and restored.
    pub(in crate::transpiler) fn preserve(&self, namespace: &str, call: String) -> Vec<String> {
        let storage = |path: &str| format!("{}:global {}", namespace, path);
        match self {
            Self::Site(id) => {
                let caller_path = format!("_caller_args_{}", id);
                vec![
                    format!("data remove storage {}", storage(&caller_path)),
                    format!(
                        "data modify storage {} set from storage {}",
                        storage(&caller_path),
                        storage("args")
                    ),
                    call,
                    format!(
                        "data modify storage {} set from storage {}",
                        storage("args"),
                        storage(&caller_path)
                    ),
                    format!("data remove storage {}", storage(&caller_path)),
                ]
            }
            Self::Stack => vec![
                format!(
                    "data modify storage {} append from storage {}",
                    storage("call_stack"),
                    storage("args")
                ),
                call,
                format!(
                    "data modify storage {} set from storage {}",
                    storage("args"),
                    storage("call_stack[-1]")
                ),
                format!("data remove storage {}", storage("call_stack[-1]")),
            ],
        }
    }

    /// The commands around a call to a function with parameters:
    /// `argument_commands` build the arguments, which are then copied to
    /// `args` for `call`. A caller that has parameters of its own keeps its
    /// `args` and gets them back after the call.
    pub(in crate::transpiler) fn wrap(
        &self,
        namespace: &str,
        preserve_caller_args: bool,
        argument_commands: Vec<String>,
        call: String,
    ) -> Vec<String> {
        let storage = |path: &str| format!("{}:global {}", namespace, path);
        let copy = |to: &str, from: &str| {
            format!(
                "data modify storage {} set from storage {}",
                storage(to),
                storage(from)
            )
        };
        let remove = |path: &str| format!("data remove storage {}", storage(path));
        let mut commands = Vec::new();
        match self {
            Self::Site(id) => {
                let call_path = format!("_call_args_{}", id);
                let caller_path = format!("_caller_args_{}", id);
                if preserve_caller_args {
                    commands.push(remove(&caller_path));
                    commands.push(copy(&caller_path, "args"));
                }
                commands.push(remove(&call_path));
                commands.extend(argument_commands);
                commands.push(copy("args", &call_path));
                commands.push(call);
                if preserve_caller_args {
                    commands.push(copy("args", &caller_path));
                    commands.push(remove(&caller_path));
                }
                commands.push(remove(&call_path));
            }
            Self::Stack => {
                if preserve_caller_args {
                    commands.push(format!(
                        "data modify storage {} append from storage {}",
                        storage("call_stack"),
                        storage("args")
                    ));
                }
                commands.push(format!(
                    "data modify storage {} append value {{}}",
                    storage("call_stack")
                ));
                commands.extend(argument_commands);
                commands.push(copy("args", "call_stack[-1]"));
                commands.push(remove("call_stack[-1]"));
                commands.push(call);
                if preserve_caller_args {
                    commands.push(copy("args", "call_stack[-1]"));
                    commands.push(remove("call_stack[-1]"));
                }
            }
        }
        commands
    }
}
