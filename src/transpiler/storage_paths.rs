//! Shared storage-path planning and dynamic-path lowering.
//!
//! `Transpiler` (statement lowering) and `ExpressionEvaluator` (expression
//! lowering) both need to resolve `config.count` / `values[i]` /
//! `groups[key]` expressions into storage path plans and emit the dynamic
//! macro-argument plumbing around them. This logic used to exist as two
//! parallel copies that had already started to drift; it now lives once
//! here, as provided methods on [`StoragePathOps`], with each host supplying
//! its lookup tables through the small accessor surface.

use crate::ast::{CobbleType, Expression};
use crate::transpiler::data_pack::DataPack;
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::{
    DynamicStoragePathArg, DynamicStoragePathArgKind, GeneratedCommand, GeneratedCommandKind,
    SourceLocation, StoragePathPlan, Transpiler,
};
use std::collections::HashMap;

/// Borrowed views of the tables the shared storage-path logic reads.
pub(in crate::transpiler) struct StoragePathTables<'a> {
    pub(in crate::transpiler) variable_storage_paths: &'a HashMap<String, String>,
    pub(in crate::transpiler) variable_types: &'a HashMap<String, CobbleType>,
    pub(in crate::transpiler) compile_time_constants: &'a HashMap<String, f64>,
    pub(in crate::transpiler) current_params: &'a [String],
}

pub(in crate::transpiler) trait StoragePathOps {
    fn storage_tables(&self) -> StoragePathTables<'_>;
    fn storage_data_pack(&self) -> &DataPack;
    fn storage_data_pack_mut(&mut self) -> &mut DataPack;
    /// Source location attached to generated helper-function commands.
    fn storage_helper_source(&self) -> Option<SourceLocation>;
    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String>;

    /// Whether `name` is a parameter of the function currently being lowered.
    fn is_storage_current_param(&self, name: &str) -> bool {
        self.storage_tables()
            .current_params
            .iter()
            .any(|param| param == name)
    }

    fn storage_expr_type(&self, expr: &Expression) -> Option<CobbleType> {
        match expr {
            Expression::Identifier(name) => self.storage_tables().variable_types.get(name).cloned(),
            Expression::Attribute(obj, _) => {
                if matches!(self.storage_expr_type(obj), Some(CobbleType::Map)) {
                    Some(CobbleType::Unknown)
                } else {
                    None
                }
            }
            Expression::Subscript(obj, _) => {
                if matches!(
                    self.storage_expr_type(obj),
                    Some(CobbleType::List | CobbleType::Map | CobbleType::String)
                ) {
                    Some(CobbleType::Unknown)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn is_string_storage_key_expr(&self, expr: &Expression) -> bool {
        match expr {
            Expression::String(_) => true,
            Expression::Identifier(name) => {
                self.storage_tables()
                    .variable_types
                    .get(name)
                    .is_some_and(|ty| matches!(ty, CobbleType::String))
                    || self.is_storage_current_param(name)
            }
            _ => false,
        }
    }

    fn should_use_dynamic_storage_key(
        &self,
        base_type: Option<CobbleType>,
        index: &Expression,
    ) -> bool {
        matches!(base_type, Some(CobbleType::Map))
            || (matches!(base_type, Some(CobbleType::Unknown))
                && self.is_string_storage_key_expr(index))
    }

    fn resolve_storage_path_plan(&self, expr: &Expression) -> Option<StoragePathPlan> {
        match expr {
            Expression::Identifier(name) => self
                .storage_tables()
                .variable_storage_paths
                .get(name)
                .cloned()
                .or_else(|| {
                    self.is_storage_current_param(name)
                        .then(|| format!("args.{}", name))
                })
                .map(StoragePathPlan::static_path),
            Expression::Attribute(obj, attr) => {
                let base_path = self.resolve_storage_path_plan(obj)?;
                Some(base_path.append_field(attr))
            }
            Expression::Subscript(obj, index) => {
                let base_path = self.resolve_storage_path_plan(obj)?;
                let base_type = self.storage_expr_type(obj);
                match &**index {
                    Expression::Number(n)
                        if self.should_use_dynamic_storage_key(base_type.clone(), index) =>
                    {
                        Some(base_path.append_dynamic_key(Expression::Number(*n)))
                    }
                    Expression::Number(n) => Some(base_path.append_static_index(*n as i32)),
                    Expression::String(s) => Some(base_path.append_field(s)),
                    Expression::Identifier(name) => {
                        if self.should_use_dynamic_storage_key(base_type.clone(), index)
                            && self
                                .storage_tables()
                                .compile_time_constants
                                .contains_key(name)
                        {
                            Some(base_path.append_dynamic_key((**index).clone()))
                        } else if let Some(value) =
                            self.storage_tables().compile_time_constants.get(name)
                        {
                            Some(base_path.append_static_index(*value as i32))
                        } else if self.should_use_dynamic_storage_key(base_type.clone(), index) {
                            Some(base_path.append_dynamic_key((**index).clone()))
                        } else {
                            Some(base_path.append_dynamic_index((**index).clone()))
                        }
                    }
                    _ if self.should_use_dynamic_storage_key(base_type, index) => {
                        Some(base_path.append_dynamic_key((**index).clone()))
                    }
                    _ => Some(base_path.append_dynamic_index((**index).clone())),
                }
            }
            _ => None,
        }
    }

    fn add_generated_helper_function(
        &mut self,
        name: String,
        commands: Vec<String>,
        kind: GeneratedCommandKind,
    ) {
        let source = self.storage_helper_source();
        let metadata = commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(command.clone(), source.clone(), kind.clone()),
                )
            })
            .collect();
        self.storage_data_pack_mut()
            .add_function_with_metadata(name, commands, metadata);
    }

    fn prepare_dynamic_storage_path(
        &mut self,
        id: u32,
        plan: &StoragePathPlan,
    ) -> Result<(Vec<String>, String), String> {
        let args_path = format!("_cobble_dyn_{}", id);
        let namespace = self.storage_data_pack().namespace.clone();
        let mut commands = vec![format!(
            "data remove storage {}:global {}",
            namespace, args_path
        )];

        for arg in plan.dynamic_args() {
            match arg.kind {
                DynamicStoragePathArgKind::Index => {
                    let temp_score = format!("#cobble_dyn_{}_{}", id, arg.placeholder);
                    commands.extend(self.storage_eval_to_target(&arg.expr, &temp_score)?);
                    commands.push(format!(
                        "execute store result storage {}:global {}.{} int 1 run scoreboard players get {} temp",
                        namespace, args_path, arg.placeholder, temp_score
                    ));
                }
                DynamicStoragePathArgKind::Key => {
                    commands.extend(self.dynamic_storage_key_arg_commands(&args_path, arg)?);
                }
            }
        }

        Ok((commands, args_path))
    }

    fn dynamic_storage_key_arg_commands(
        &self,
        args_path: &str,
        arg: &DynamicStoragePathArg,
    ) -> Result<Vec<String>, String> {
        let tables = self.storage_tables();
        let namespace = &self.storage_data_pack().namespace;
        let target_path = format!("{}.{}", args_path, arg.placeholder);
        match &arg.expr {
            Expression::String(value) => {
                let escaped = value.replace('\\', "\\\\").replace('"', "\\\"");
                Ok(vec![format!(
                    "data modify storage {}:global {} set value \"{}\"",
                    namespace, target_path, escaped
                )])
            }
            Expression::Identifier(name) => {
                if tables.compile_time_constants.contains_key(name) {
                    return Err(format!(
                        "Dynamic map key `{}` must be a string value, not a numeric const",
                        name
                    ));
                }
                if let Some(source_path) = tables.variable_storage_paths.get(name) {
                    if tables
                        .variable_types
                        .get(name)
                        .is_some_and(|ty| !matches!(ty, CobbleType::String | CobbleType::Unknown))
                    {
                        return Err(format!("Dynamic map key `{}` must be a string value", name));
                    }
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if self.is_storage_current_param(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global args.{}",
                        namespace, target_path, namespace, name
                    )]);
                }
                Err(format!(
                    "Dynamic map key `{}` must be a string variable or function parameter",
                    name
                ))
            }
            _ => Err(
                "Dynamic map keys must be string literals, string variables, or function parameters"
                    .to_string(),
            ),
        }
    }
}

impl StoragePathOps for Transpiler {
    fn storage_tables(&self) -> StoragePathTables<'_> {
        StoragePathTables {
            variable_storage_paths: &self.variable_storage_paths,
            variable_types: &self.variable_types,
            compile_time_constants: &self.compile_time_constants,
            current_params: &self.current_context.params,
        }
    }

    fn storage_data_pack(&self) -> &DataPack {
        &self.data_pack
    }

    fn storage_data_pack_mut(&mut self) -> &mut DataPack {
        &mut self.data_pack
    }

    fn storage_helper_source(&self) -> Option<SourceLocation> {
        self.current_statement_source.clone()
    }

    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String> {
        self.evaluate_expression_to_target(expr, target)
    }
}

impl<'a> StoragePathOps for ExpressionEvaluator<'a> {
    fn storage_tables(&self) -> StoragePathTables<'_> {
        StoragePathTables {
            variable_storage_paths: self.variable_storage_paths,
            variable_types: self.variable_types,
            compile_time_constants: self.compile_time_constants,
            current_params: self.current_params,
        }
    }

    fn storage_data_pack(&self) -> &DataPack {
        &*self.data_pack
    }

    fn storage_data_pack_mut(&mut self) -> &mut DataPack {
        self.data_pack
    }

    fn storage_helper_source(&self) -> Option<SourceLocation> {
        self.source.clone()
    }

    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String> {
        self.evaluate_expression_to_target(expr, target)
    }
}
