use crate::ast::{CobbleType, Expression, FunctionDef};
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{
    FunctionContext, FunctionReturnKind, ReturnContext, TranspileError, Transpiler,
};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

impl Transpiler {
    pub(in crate::transpiler) fn process_function_def(
        &mut self,
        func: &FunctionDef,
    ) -> Result<(), TranspileError> {
        // Save previous function context
        let previous_function = self.current_function.take();
        let previous_metadata = self.current_function_metadata.take();
        let previous_context = self.current_context.clone();
        let previous_return = self.current_return.clone();
        let previous_function_name = self.current_function_name.take();
        let previous_symbols = self.symbols.clone();

        // Extract parameter names from Parameter structs
        let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();

        // A parameter shadows a module variable or constant with the same
        // name, like in Python. Without this, conditions and arithmetic
        // would read the module score while `{name}` reads the argument.
        // The tables are restored after the function below.
        let mut shadowed_constants = Vec::new();
        for name in &param_names {
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
        self.current_context = FunctionContext::with_params(param_names.clone(), &func.body);
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

        // Get the generated commands
        if let Some(mut commands) = self.current_function.take() {
            if matches!(return_kind, FunctionReturnKind::Integer) {
                if let Some(return_context) = &self.current_return {
                    commands.push(format!(
                        "execute unless score {} temp matches 1 run return 0",
                        return_context.flag_score
                    ));
                }
            }
            let metadata = self.current_function_metadata.take().unwrap_or_default();
            self.data_pack
                .add_function_with_metadata(func.name.clone(), commands, metadata);
        }

        // Restore previous context
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

        Ok(())
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

    pub(in crate::transpiler) fn record_user_function_call(
        &mut self,
        callee: &str,
    ) -> Result<(), TranspileError> {
        self.functions
            .record_call(self.current_function_name.as_deref(), callee)
    }

    /// Reject mutually recursive user-function call cycles. Direct self-calls
    /// error eagerly at the call site; this catches `a -> b -> a` style cycles
    /// once all call edges have been recorded. Recursion cannot compile
    /// correctly because argument frames and return flags are global fake
    /// players, not per-invocation stacks.
    pub(in crate::transpiler) fn check_recursive_call_cycles(&self) -> Result<(), TranspileError> {
        fn visit(
            node: &str,
            edges: &BTreeMap<String, BTreeSet<String>>,
            visiting: &mut Vec<String>,
            done: &mut HashSet<String>,
        ) -> Option<Vec<String>> {
            if let Some(position) = visiting.iter().position(|name| name == node) {
                let mut cycle = visiting[position..].to_vec();
                cycle.push(node.to_string());
                return Some(cycle);
            }
            if done.contains(node) {
                return None;
            }
            visiting.push(node.to_string());
            if let Some(callees) = edges.get(node) {
                for callee in callees {
                    if let Some(cycle) = visit(callee, edges, visiting, done) {
                        return Some(cycle);
                    }
                }
            }
            visiting.pop();
            done.insert(node.to_string());
            None
        }

        let mut done = HashSet::new();
        for node in self.functions.call_edges.keys() {
            let mut visiting = Vec::new();
            if let Some(cycle) = visit(node, &self.functions.call_edges, &mut visiting, &mut done) {
                return Err(format!(
                    "Recursive function calls are not supported: cycle {}. Rewrite the recursion as a loop (while/for) instead.",
                    cycle.join(" -> ")
                ).into());
            }
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
