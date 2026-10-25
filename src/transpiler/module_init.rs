use crate::ast::Expression;
use crate::transpiler::storage_paths::StorageValueOps;
use crate::transpiler::{
    GeneratedCommand, GeneratedCommandKind, SourceLocation, TranspileError, Transpiler,
};
use std::collections::HashMap;

#[derive(Clone)]
pub(in crate::transpiler) enum ModuleLevelAssignment {
    Variable {
        target: String,
        value: Expression,
        source: Option<SourceLocation>,
        persist: bool,
    },
    StorageTarget {
        target_expr: Expression,
        value: Expression,
        source: Option<SourceLocation>,
    },
}

impl Transpiler {
    pub(in crate::transpiler) fn build_module_init_commands(
        &mut self,
    ) -> Result<(Vec<String>, HashMap<usize, GeneratedCommand>), TranspileError> {
        let mut init_commands = Vec::new();
        let mut init_metadata = HashMap::new();

        for assignment in self.module_level_vars.clone() {
            match assignment {
                ModuleLevelAssignment::Variable {
                    target,
                    value,
                    source,
                    persist,
                } => {
                    let previous_source = self.current_statement_source.clone();
                    self.current_statement_source = source.clone();
                    let generated = self.module_variable_init_commands(&target, &value);
                    self.current_statement_source = previous_source;
                    let mut generated = generated
                        .map_err(|error| error.with_location_if_missing(source.clone()))?;
                    if persist {
                        generated = self.guard_persistent_init(&target, generated, &source);
                    }
                    Self::append_module_init_commands(
                        &mut init_commands,
                        &mut init_metadata,
                        generated,
                        source,
                    );
                }
                ModuleLevelAssignment::StorageTarget {
                    target_expr,
                    value,
                    source,
                } => {
                    let previous_source = self.current_statement_source.clone();
                    self.current_statement_source = source.clone();
                    let generated = self.storage_target_assignment_commands(&target_expr, &value);
                    self.current_statement_source = previous_source;
                    let generated = generated
                        .map_err(|error| error.with_location_if_missing(source.clone()))?;
                    Self::append_module_init_commands(
                        &mut init_commands,
                        &mut init_metadata,
                        generated,
                        source,
                    );
                }
            }
        }

        Ok((init_commands, init_metadata))
    }

    /// Wraps a `persist` variable's initializer so it only runs while the
    /// value is unset. A multi-command initializer moves into a helper
    /// function behind the same guard.
    pub(in crate::transpiler) fn guard_persistent_init(
        &mut self,
        name: &str,
        commands: Vec<String>,
        source: &Option<SourceLocation>,
    ) -> Vec<String> {
        let namespace = self.data_pack.namespace.clone();
        let guard = match self.symbols.variable_storage_paths.get(name) {
            Some(path) => format!("unless data storage {namespace}:global {path}"),
            None => format!("unless score {name} temp matches -2147483648.."),
        };
        if let [command] = commands.as_slice() {
            return vec![format!("execute {guard} run {command}")];
        }

        let helper = format!("_cobble_persist_{}", self.next_temp_id());
        let metadata = commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(
                        command.clone(),
                        source.clone(),
                        GeneratedCommandKind::ControlFlow,
                    ),
                )
            })
            .collect();
        self.data_pack
            .add_function_with_metadata(helper.clone(), commands, metadata);
        vec![format!("execute {guard} run function {namespace}:{helper}")]
    }

    /// Whether `value` can initialize a `persist` variable: a compile-time
    /// number or boolean, or a string, list, or map literal of constants.
    pub(in crate::transpiler) fn is_persist_initializer(&self, value: &Expression) -> bool {
        if self.try_eval_const(value).is_some() {
            return true;
        }
        match value {
            Expression::String(_) => true,
            Expression::Array(items) => items.iter().all(|item| self.is_persist_initializer(item)),
            Expression::Map(entries) => entries
                .iter()
                .all(|(_, item)| self.is_persist_initializer(item)),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn append_module_init_commands(
        init_commands: &mut Vec<String>,
        init_metadata: &mut HashMap<usize, GeneratedCommand>,
        commands: Vec<String>,
        source: Option<SourceLocation>,
    ) {
        for command in commands {
            let index = init_commands.len();
            init_metadata.insert(
                index,
                GeneratedCommand::new(
                    command.clone(),
                    source.clone(),
                    GeneratedCommandKind::ControlFlow,
                ),
            );
            init_commands.push(command);
        }
    }

    pub(in crate::transpiler) fn module_variable_init_commands(
        &mut self,
        var_name: &str,
        value: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(const_value) = self.try_eval_const(value) {
            let truncated = const_value as i32;

            if const_value > i32::MAX as f64 || const_value < i32::MIN as f64 {
                eprintln!(
                    "⚠️  Warning: Module-level value {} for variable '{}' exceeds Minecraft scoreboard range.\n\
                    Scoreboard range: {} to {}\n\
                    Value will be clamped to: {}",
                    const_value,
                    var_name,
                    i32::MIN,
                    i32::MAX,
                    if const_value > i32::MAX as f64 {
                        i32::MAX
                    } else {
                        i32::MIN
                    }
                );
            }

            if const_value.fract() != 0.0 {
                eprintln!(
                    "⚠️  Warning: Module-level value {} for variable '{}' will lose precision.\n\
                    Scoreboard only supports integers. Fractional part will be truncated to: {}",
                    const_value, var_name, truncated
                );
            }

            return Ok(vec![format!(
                "scoreboard players set {} temp {}",
                var_name, truncated
            )]);
        }

        match value {
            Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                let storage_path = format!("vars.{}", var_name);
                self.symbols
                    .variable_storage_paths
                    .insert(var_name.to_string(), storage_path.clone());
                self.storage_value_commands(&storage_path, value)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
            _ => {
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(var_name.to_string(), "temp".to_string());
                self.symbols
                    .scoreboard_variables
                    .insert(var_name.to_string());
                self.evaluate_expression_to_target(value, var_name)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
        }
    }
}
