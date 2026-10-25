use crate::ast::{Assignment, Expression};
use crate::stdlib::EventType;
use crate::transpiler::expression_evaluator::{call_args_path, staged_call_commands};
use crate::transpiler::{escape_quoted, TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn generate_function_call(
        &mut self,
        func_name: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.record_user_function_call(func_name)?;
        let namespace = self.data_pack.namespace.clone();
        let plain_call = format!("function {}:{}", namespace, func_name);

        let Some(param_names) = self.functions.params.get(func_name).cloned() else {
            self.extend_current_function(vec![plain_call]);
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
            self.extend_current_function(vec![plain_call]);
            return Ok(());
        }

        let id = self.next_temp_id();
        let mut argument_commands = Vec::new();
        for (param_name, arg) in param_names.iter().zip(args) {
            let target_path = format!("{}.{}", call_args_path(id), param_name);
            argument_commands.extend(self.call_argument_commands(
                func_name,
                param_name,
                &target_path,
                arg,
            )?);
        }
        let commands = staged_call_commands(
            &namespace,
            id,
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

    fn extend_current_function(&mut self, commands: Vec<String>) {
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
                 - Variables: func(my_var)\n\
                 - Arithmetic expressions: func(x + 10)\n\
                 \n\
                 Not supported:\n\
                 - Arrays/maps as direct macro arguments",
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
