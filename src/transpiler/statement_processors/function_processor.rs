use crate::ast::{CobbleType, Expression, FunctionDef, IfStatement, Statement};
use crate::transpiler::function_context::unbound_local_message;
use crate::transpiler::score_holders::{rename_holders, written_holders};
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{
    FunctionContext, FunctionReturnKind, GeneratedCommand, GeneratedCommandKind, ReturnContext,
    SourceLocation, TranspileError, TranspileErrorKind, Transpiler,
};
use std::borrow::Cow;
use std::collections::{BTreeSet, HashMap, HashSet};

/// `giveReward` as `give_reward`.
fn snake_case(name: &str) -> String {
    let mut snake = String::new();
    let mut after_lowercase = false;
    for ch in name.chars() {
        if ch.is_ascii_uppercase() && after_lowercase {
            snake.push('_');
        }
        after_lowercase = ch.is_ascii_lowercase() || ch.is_ascii_digit();
        snake.push(ch.to_ascii_lowercase());
    }
    snake
}

impl Transpiler {
    pub(in crate::transpiler) fn process_function_def(
        &mut self,
        func: &FunctionDef,
    ) -> Result<(), TranspileError> {
        let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
        let context = FunctionContext::with_params(param_names.clone(), &func.body);
        let shadows: Vec<String> = context
            .locals
            .iter()
            .filter(|local| !self.in_prelude && self.symbols.module_variables.contains(*local))
            .cloned()
            .collect();
        let previous_shadows = std::mem::replace(&mut self.unwarned_shadows, shadows);
        let def_source = self
            .current_statement_source
            .clone()
            .or_else(|| self.function_def_source(func));
        if func.name.contains(|ch: char| ch.is_ascii_uppercase()) {
            return Err(TranspileError::new(
                TranspileErrorKind::Semantic,
                format!(
                    "Function name `{}` has capital letters, which Minecraft function paths can't use. Rename it, for example to `{}`.",
                    func.name,
                    snake_case(&func.name)
                ),
            )
            .with_location_if_missing(def_source));
        }

        // Save previous function context
        let previous_function = self.current_function.take();
        let previous_metadata = self.current_function_metadata.take();
        let previous_context = self.current_context.clone();
        let previous_return = self.current_return.clone();
        let previous_function_name = self.current_function_name.take();
        let previous_symbols = self.symbols.clone();
        let functions_before: HashSet<String> = self.data_pack.functions.keys().cloned().collect();

        // Parameters and local variables hide module variables and constants
        // with the same names, like in Python. The tables are restored after
        // the function below.
        let mut shadowed_constants = Vec::new();
        for name in param_names.iter().chain(&context.locals) {
            self.symbols.variables.remove(name);
            self.symbols.variable_objectives.remove(name);
            self.symbols.scoreboard_variables.remove(name);
            self.symbols.variable_storage_paths.remove(name);
            self.symbols.variable_types.remove(name);
            if let Some(value) = self.symbols.compile_time_constants.remove(name) {
                shadowed_constants.push((name.clone(), value));
            }
        }

        // Set up new function context
        let globals = context.globals.clone();
        let recursive = self.functions.recursive.contains(&func.name);
        if recursive {
            // A recursive function runs many commands, like a loop.
            self.data_pack.track_runtime_loop();
        }
        self.current_context = context;
        self.current_function = Some(Vec::new());
        self.current_function_metadata = Some(HashMap::new());
        self.current_function_name = Some(func.name.clone());
        let return_kind = Self::analyze_function_return_kind(&func.body);
        // Only functions that actually contain a `return` statement need the
        // return-flag protocol. Everything else skips the prologue and the
        // `temp` objective so simple packs stay free of `_cobble_init`.
        if matches!(return_kind, FunctionReturnKind::None) {
            self.current_return = None;
        } else {
            let return_id = self.next_temp_id();
            self.current_return = Some(ReturnContext {
                value_score: format!("#return_value_{}", return_id),
                flag_score: format!("#return_flag_{}", return_id),
            });
            self.data_pack.track_objective("temp");
            if let (Some(commands), Some(return_context)) =
                (&mut self.current_function, &self.current_return)
            {
                commands.push(format!(
                    "scoreboard players set {} temp 0",
                    return_context.flag_score
                ));
            }
        }
        self.symbols.global_variables.clear();

        // Store function parameters
        self.functions.params.insert(func.name.clone(), param_names);
        self.functions
            .returns
            .insert(func.name.clone(), return_kind);

        // Process function body
        for statement in &func.body {
            self.process_statement(statement)?;
        }
        // A read inside an unrolled loop is lowered from a copy of its
        // statement, so it is reported here.
        if let Some(read) = &self.current_context.unbound_read {
            return Err(unbound_local_message(&read.name, &func.name).into());
        }
        // A local whose assignments were never lowered, such as one in a
        // branch the compiler drops, warns at the `def` line.
        for local in std::mem::replace(&mut self.unwarned_shadows, previous_shadows) {
            self.warn_shadowed_module_variable(&local, def_source.clone());
        }

        // Get the generated commands
        let return_context = self.current_return.clone();
        if let Some(mut commands) = self.current_function.take() {
            let mut metadata = self.current_function_metadata.take().unwrap_or_default();
            if recursive {
                self.route_returns_through_exit(
                    &func.name,
                    return_context.as_ref(),
                    &mut commands,
                    &mut metadata,
                );
            } else if matches!(return_kind, FunctionReturnKind::Integer) {
                if let Some(return_context) = &return_context {
                    commands.push(format!(
                        "execute unless score {} temp matches 1 run return 0",
                        return_context.flag_score
                    ));
                }
            }
            self.data_pack
                .add_function_with_metadata(func.name.clone(), commands, metadata);
        }

        // Restore previous context. Lowering the body can add hidden locals.
        let locals = self.current_context.locals.clone();
        self.current_function = previous_function;
        self.current_function_metadata = previous_metadata;
        self.current_context = previous_context;
        self.current_return = previous_return;
        self.current_function_name = previous_function_name;
        // Variables assigned in the body don't outlive it. Constants,
        // selector aliases, and entity templates it declares stay visible.
        let Symbols {
            mut compile_time_constants,
            selector_aliases,
            entity_templates,
            ..
        } = std::mem::replace(&mut self.symbols, previous_symbols);
        compile_time_constants.extend(shadowed_constants);
        self.symbols.compile_time_constants = compile_time_constants;
        self.symbols.selector_aliases = selector_aliases;
        self.symbols.entity_templates = entity_templates;

        self.localize_function(&func.name, &locals, &functions_before);
        if recursive {
            self.add_recursion_frame(&func.name, &locals, &globals, return_context.as_ref());
        }
        Ok(())
    }

    /// In a recursive function, every way out of the function goes through
    /// its exit helper, which restores the caller's frame: returns at the
    /// function's top level, return propagation from its blocks, and the
    /// end of the body. Returns inside block helpers only leave the helper
    /// and keep their form.
    fn route_returns_through_exit(
        &self,
        function: &str,
        return_context: Option<&ReturnContext>,
        commands: &mut Vec<String>,
        metadata: &mut HashMap<usize, GeneratedCommand>,
    ) {
        let exit = format!(
            "return run function {}:{}",
            self.data_pack.namespace,
            exit_function_name(function)
        );
        if let Some(context) = return_context {
            let value_return = format!(
                "return run scoreboard players get {} temp",
                context.value_score
            );
            for (index, command) in commands.iter_mut().enumerate() {
                let routed = if *command == value_return || command == "return 0" {
                    Some(exit.clone())
                } else {
                    command
                        .strip_suffix(&value_return)
                        .filter(|prefix| prefix.ends_with(" run "))
                        .map(|prefix| format!("{}{}", prefix, exit))
                };
                if let Some(routed) = routed {
                    if let Some(generated) = metadata.get_mut(&index) {
                        generated.text = routed.clone();
                    }
                    *command = routed;
                }
            }
            // Reaching the end means no `return` ran.
            if commands.last() != Some(&exit) {
                commands.push(format!(
                    "scoreboard players set {} temp 0",
                    context.value_score
                ));
                commands.push(exit);
            }
        } else {
            commands.push(format!(
                "function {}:{}",
                self.data_pack.namespace,
                exit_function_name(function)
            ));
        }
    }

    /// Saves the scores and local storage a recursive function writes when
    /// it starts and restores them in its exit helper, so a call to it
    /// leaves the caller's state as it was. The frame holds the function's
    /// locals, the compiler's scratch scores in the function and its
    /// helpers, and its return value and flag. Module variables and the
    /// variables it declares `global` stay out, so writes to them last.
    fn add_recursion_frame(
        &mut self,
        function: &str,
        locals: &[String],
        globals: &[String],
        return_context: Option<&ReturnContext>,
    ) {
        let namespace = self.data_pack.namespace.clone();
        let objective = self.data_pack.vars_objective();
        let owned: Vec<String> = self
            .functions
            .owners
            .iter()
            .filter(|(_, owner)| owner.as_str() == function)
            .map(|(name, _)| name.clone())
            .collect();

        let mut holders: BTreeSet<String> = locals
            .iter()
            .map(|local| local_holder(function, local))
            .collect();
        let local_storage = format!("locals.{}", function);
        let mut uses_local_storage = false;
        for name in &owned {
            let commands = &self.data_pack.functions[name];
            let metadata = self.data_pack.command_metadata.get(name);
            for (index, command) in commands.iter().enumerate() {
                uses_local_storage |= command.contains(&local_storage);
                let kind = metadata
                    .and_then(|metadata| metadata.get(&index))
                    .map(|generated| &generated.kind);
                if matches!(
                    kind,
                    Some(GeneratedCommandKind::UserCommand | GeneratedCommandKind::StdLib)
                ) {
                    continue;
                }
                for holder in written_holders(command, &objective) {
                    if holder.starts_with('@')
                        || holder.starts_with("$(")
                        || SHARED_SCRATCH.contains(&holder)
                        || self.symbols.module_variables.contains(holder)
                        || globals.iter().any(|global| global == holder)
                    {
                        continue;
                    }
                    holders.insert(holder.to_string());
                }
            }
        }

        let storage = |path: &str| format!("{}:global {}", namespace, path);
        let frames = format!("frames.{}", function);
        let top = format!("{}[-1]", frames);
        let mut push = vec![format!(
            "data modify storage {} append value {{}}",
            storage(&frames)
        )];
        let mut restore = Vec::new();
        let out_holder = format!("#{}.return", function);
        if let Some(context) = return_context {
            restore.push(format!(
                "scoreboard players operation {} {} = {} {}",
                out_holder, objective, context.value_score, objective
            ));
        }
        for (index, holder) in holders.iter().enumerate() {
            push.push(format!(
                "execute store result storage {} int 1 run scoreboard players get {} {}",
                storage(&format!("{}.v{}", top, index)),
                holder,
                objective
            ));
            restore.push(format!(
                "execute store result score {} {} run data get storage {}",
                holder,
                objective,
                storage(&format!("{}.v{}", top, index))
            ));
        }
        if uses_local_storage {
            push.push(format!(
                "data modify storage {} set from storage {}",
                storage(&format!("{}.locals", top)),
                storage(&local_storage)
            ));
            restore.push(format!("data remove storage {}", storage(&local_storage)));
            restore.push(format!(
                "data modify storage {} set from storage {}",
                storage(&local_storage),
                storage(&format!("{}.locals", top))
            ));
        }
        restore.push(format!("data remove storage {}", storage(&top)));
        if return_context.is_some() {
            restore.push(format!(
                "return run scoreboard players get {} {}",
                out_holder, objective
            ));
        }

        let pushed = push.len();
        if let Some(commands) = self.data_pack.functions.get_mut(function) {
            commands.splice(0..0, push.clone());
        }
        if let Some(metadata) = self.data_pack.command_metadata.get_mut(function) {
            let shifted: HashMap<usize, GeneratedCommand> = metadata
                .drain()
                .map(|(index, generated)| (index + pushed, generated))
                .collect();
            *metadata = shifted;
            for (index, command) in push.into_iter().enumerate() {
                metadata.insert(
                    index,
                    GeneratedCommand::new(command, None, GeneratedCommandKind::ControlFlow),
                );
            }
        }
        let exit = exit_function_name(function);
        let restore_metadata = restore
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(command.clone(), None, GeneratedCommandKind::ControlFlow),
                )
            })
            .collect();
        self.data_pack
            .add_function_with_metadata(exit.clone(), restore, restore_metadata);
        self.functions.owners.insert(exit, function.to_string());
    }

    /// Gives `function`'s local variables their own score holders,
    /// `#<function>.<name>`, in the function and the helpers lowered with it
    /// (block bodies, loops, condition helpers). Lowering writes locals under
    /// their source names, and this pass renames them once the generated
    /// commands are final, so raw commands in the function that name a local
    /// on the variable objective reach the same score.
    fn localize_function(
        &mut self,
        function: &str,
        locals: &[String],
        functions_before: &HashSet<String>,
    ) {
        let mut owned: Vec<String> = self
            .data_pack
            .functions
            .keys()
            .filter(|name| {
                !functions_before.contains(*name)
                    && !self.functions.owners.contains_key(*name)
                    && !name.starts_with("_cobble_math_")
            })
            .cloned()
            .collect();
        // The function exists already when a transpiler compiles the same
        // file twice. Its commands were just generated again either way.
        if !owned.iter().any(|name| name == function) {
            owned.push(function.to_string());
        }
        for name in &owned {
            self.functions
                .owners
                .insert(name.clone(), function.to_string());
        }
        if locals.is_empty() {
            return;
        }

        let renames: HashMap<String, String> = locals
            .iter()
            .map(|local| (local.clone(), local_holder(function, local)))
            .collect();
        let objective = self.data_pack.vars_objective();
        for name in owned {
            if let Some(commands) = self.data_pack.functions.get_mut(&name) {
                for command in commands.iter_mut() {
                    if let Cow::Owned(renamed) = rename_holders(command, &objective, &renames) {
                        *command = renamed;
                    }
                }
            }
            if let Some(metadata) = self.data_pack.command_metadata.get_mut(&name) {
                for generated in metadata.values_mut() {
                    if let Cow::Owned(renamed) =
                        rename_holders(&generated.text, &objective, &renames)
                    {
                        generated.text = renamed;
                    }
                }
            }
        }
    }

    /// Warns that `local` of the function being lowered hides the module
    /// variable with the same name.
    pub(in crate::transpiler) fn warn_shadowed_module_variable(
        &self,
        local: &str,
        location: Option<SourceLocation>,
    ) {
        let function = self.current_function_name.as_deref().unwrap_or_default();
        self.warn_at(
            "local-shadows-module-variable",
            format!(
                "`{local}` is assigned in `{function}`, so it is a local variable there. The module variable `{local}` keeps its value."
            ),
            Some(&format!(
                "Add `global {local}` to `{function}` to change the module variable."
            )),
            location,
        );
    }

    /// Whether the function being lowered is recursive.
    pub(in crate::transpiler) fn in_recursive_function(&self) -> bool {
        self.current_function_name
            .as_ref()
            .is_some_and(|name| self.functions.recursive.contains(name))
    }

    /// Storage path of a list, map, or string variable: `locals.<function>.
    /// <name>` for a local variable of the function being lowered, and
    /// `vars.<name>` for a module variable.
    pub(in crate::transpiler) fn variable_storage_path(&self, name: &str) -> String {
        match &self.current_function_name {
            Some(function) if self.current_context.is_local(name) => {
                format!("locals.{}.{}", function, name)
            }
            _ => format!("vars.{}", name),
        }
    }

    pub(in crate::transpiler) fn process_return(
        &mut self,
        expr: Option<&Expression>,
    ) -> Result<(), TranspileError> {
        if self.current_function.is_none() {
            return Err("return can only be used inside a function"
                .to_string()
                .into());
        }

        if let Some(condition) = expr.filter(|expr| Self::is_runtime_condition_expression(expr)) {
            // `return x > 3` gives 1 or 0.
            return self.process_if(&IfStatement {
                condition: condition.clone(),
                then_branch: vec![Statement::Return(Some(Expression::Number(1.0)))],
                elif_branches: Vec::new(),
                else_branch: Some(vec![Statement::Return(Some(Expression::Number(0.0)))]),
            });
        }

        let mut commands = Vec::new();
        let context = self.current_return.clone();
        if let Some(expr) = expr {
            match self.infer_type(expr) {
                CobbleType::String | CobbleType::List | CobbleType::Map => {
                    return Err(
                        "return values must be integer scoreboard expressions; use storage/output variables for strings, lists, and maps."
                            .into(),
                    );
                }
                _ => {}
            }
            let temp_var = context
                .as_ref()
                .map(|context| context.value_score.clone())
                .unwrap_or_else(|| {
                    let temp_var = format!("#return_value_{}", self.next_temp_id());
                    temp_var
                });
            commands.extend(self.evaluate_expression_to_target(expr, &temp_var)?);
            if let Some(context) = &context {
                commands.push(format!(
                    "scoreboard players set {} temp 1",
                    context.flag_score
                ));
            }
            commands.push(format!(
                "return run scoreboard players get {} temp",
                temp_var
            ));
        } else {
            if let Some(context) = &context {
                commands.push(format!(
                    "scoreboard players set {} temp 0",
                    context.value_score
                ));
                commands.push(format!(
                    "scoreboard players set {} temp 1",
                    context.flag_score
                ));
            }
            commands.push("return 0".to_string());
        }

        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }

        Ok(())
    }

    pub(in crate::transpiler) fn return_propagation_command(&self) -> Option<String> {
        let context = self.current_return.as_ref()?;
        Some(format!(
            "execute if score {} temp matches 1 run return run scoreboard players get {} temp",
            context.flag_score, context.value_score
        ))
    }
}

/// The score holder of a function's local variable.
pub(in crate::transpiler) fn local_holder(function: &str, name: &str) -> String {
    format!("#{}.{}", function, name)
}

/// The helper a recursive function leaves through.
pub(in crate::transpiler) fn exit_function_name(function: &str) -> String {
    format!("_cobble_exit_{}", function)
}

/// Scratch scores that the compiler sets and reads right away, so they are
/// never live across a call and stay out of recursion frames.
const SHARED_SCRATCH: &[&str] = &[
    "#multiplier",
    "#divisor",
    "#modulus",
    "#neg_const",
    "#addend",
    "#expr_param",
    "#math_input",
    "#math_input2",
    "#math_result",
    "#true_const",
    "#false_const",
    "#cobble_always_false",
];
