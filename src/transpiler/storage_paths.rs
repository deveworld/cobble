//! Shared storage-path planning and dynamic-path lowering.
//!
//! `Transpiler` (statement lowering) and `ExpressionEvaluator` (expression
//! lowering) both resolve `config.count` / `values[i]` / `groups[key]`
//! expressions into storage path plans and emit the dynamic macro-argument
//! plumbing around them. The logic lives here as provided methods on
//! [`StoragePathOps`]; each host supplies its symbol tables through a few
//! accessors.

use crate::ast::{CobbleType, Expression};
use crate::transpiler::data_pack::DataPack;
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{
    escape_quoted, GeneratedCommand, GeneratedCommandKind, SourceLocation, TranspileError,
    Transpiler,
};

pub(in crate::transpiler) trait StoragePathOps {
    fn storage_symbols(&self) -> &Symbols;
    /// Parameters of the function currently being lowered.
    fn storage_current_params(&self) -> &[String];
    fn storage_data_pack(&self) -> &DataPack;
    fn storage_data_pack_mut(&mut self) -> &mut DataPack;
    /// Source location attached to generated helper-function commands.
    fn storage_helper_source(&self) -> Option<SourceLocation>;
    fn storage_eval_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError>;

    /// Whether `name` is a parameter of the function currently being lowered.
    fn is_storage_current_param(&self, name: &str) -> bool {
        self.storage_current_params()
            .iter()
            .any(|param| param == name)
    }

    fn storage_expr_type(&self, expr: &Expression) -> Option<CobbleType> {
        match expr {
            Expression::Identifier(name) => {
                self.storage_symbols().variable_types.get(name).cloned()
            }
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
                self.storage_symbols()
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
                .storage_symbols()
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
                                .storage_symbols()
                                .compile_time_constants
                                .contains_key(name)
                        {
                            Some(base_path.append_dynamic_key((**index).clone()))
                        } else if let Some(value) =
                            self.storage_symbols().compile_time_constants.get(name)
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
    ) -> Result<(Vec<String>, String), TranspileError> {
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
    ) -> Result<Vec<String>, TranspileError> {
        let tables = self.storage_symbols();
        let namespace = &self.storage_data_pack().namespace;
        let target_path = format!("{}.{}", args_path, arg.placeholder);
        match &arg.expr {
            Expression::String(value) => {
                let escaped = escape_quoted(value);
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
                    ).into());
                }
                if let Some(source_path) = tables.variable_storage_paths.get(name) {
                    if tables
                        .variable_types
                        .get(name)
                        .is_some_and(|ty| !matches!(ty, CobbleType::String | CobbleType::Unknown))
                    {
                        return Err(format!("Dynamic map key `{}` must be a string value", name).into());
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
                ).into())
            }
            _ => Err(
                "Dynamic map keys must be string literals, string variables, or function parameters"
                    .into(),
            ),
        }
    }
}

/// Lowering of Cobble values into `data modify storage` command sequences:
/// static SNBT fast path, shell-plus-runtime-patch for containers holding
/// runtime values, and leaf sets for scalars. Provided methods over a small
/// accessor surface, like [`StoragePathOps`], so the logic lives once here.
pub(in crate::transpiler) trait StorageValueOps: StoragePathOps {
    /// Next unique id for generated temp scores / storage paths.
    fn storage_next_temp_id(&mut self) -> u32;
    /// Whether `name` is a macro parameter of the function currently being lowered.
    fn is_storage_macro_param(&self, name: &str) -> bool;
    /// Objective for a scoreboard-backed identifier, if `name` is one.
    fn storage_scoreboard_objective(&self, name: &str) -> Option<&str>;
    /// Static SNBT serialization for literal/const expressions.
    fn storage_static_snbt(&self, expr: &Expression) -> Result<String, TranspileError>;

    fn storage_value_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        if let Ok(snbt) = self.storage_static_snbt(expr) {
            return Ok(vec![format!(
                "data modify storage {}:global {} set value {}",
                self.storage_data_pack().namespace,
                target_path,
                snbt
            )]);
        }

        match expr {
            Expression::Array(_) | Expression::Map(_) => {
                let shell = self.storage_shell_snbt(expr)?;
                let mut commands = vec![format!(
                    "data modify storage {}:global {} set value {}",
                    self.storage_data_pack().namespace,
                    target_path,
                    shell
                )];
                commands.extend(self.storage_runtime_patch_commands(target_path, expr)?);
                Ok(commands)
            }
            _ => self.storage_leaf_set_commands(target_path, expr),
        }
    }

    fn materialize_storage_value(
        &mut self,
        expr: &Expression,
    ) -> Result<(Vec<String>, String), TranspileError> {
        let id = self.storage_next_temp_id();
        let path = format!("_cobble_value_{}", id);
        let commands = self.storage_value_commands(&path, expr)?;
        Ok((commands, path))
    }

    fn storage_shell_snbt(&self, expr: &Expression) -> Result<String, TranspileError> {
        match expr {
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => self.storage_static_snbt(expr),
            Expression::Identifier(name) => {
                if self
                    .storage_symbols()
                    .compile_time_constants
                    .contains_key(name)
                {
                    self.storage_static_snbt(expr)
                } else {
                    Ok(self.default_snbt_for_runtime_identifier(name))
                }
            }
            Expression::Array(items) => {
                let mut result = String::from("[");
                for (index, item) in items.iter().enumerate() {
                    if index > 0 {
                        result.push(',');
                    }
                    result.push_str(&self.storage_shell_snbt(item)?);
                }
                result.push(']');
                Ok(result)
            }
            Expression::Map(entries) => {
                let mut result = String::from("{");
                for (index, (key, value)) in entries.iter().enumerate() {
                    if index > 0 {
                        result.push(',');
                    }
                    result.push_str(&Transpiler::serialize_snbt_key(key));
                    result.push(':');
                    result.push_str(&self.storage_shell_snbt(value)?);
                }
                result.push('}');
                Ok(result)
            }
            _ => Ok("0".to_string()),
        }
    }

    fn default_snbt_for_runtime_identifier(&self, name: &str) -> String {
        match self.storage_symbols().variable_types.get(name) {
            Some(CobbleType::String) => "\"\"".to_string(),
            Some(CobbleType::List) => "[]".to_string(),
            Some(CobbleType::Map) => "{}".to_string(),
            Some(CobbleType::Boolean) => "0b".to_string(),
            _ => "0".to_string(),
        }
    }

    fn storage_runtime_patch_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        match expr {
            Expression::Array(items) => {
                let mut commands = Vec::new();
                for (index, item) in items.iter().enumerate() {
                    commands.extend(self.storage_runtime_patch_commands(
                        &format!("{}[{}]", target_path, index),
                        item,
                    )?);
                }
                Ok(commands)
            }
            Expression::Map(entries) => {
                let mut commands = Vec::new();
                for (key, value) in entries {
                    commands.extend(self.storage_runtime_patch_commands(
                        &storage_child_path(target_path, key),
                        value,
                    )?);
                }
                Ok(commands)
            }
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => Ok(Vec::new()),
            Expression::Identifier(name)
                if self
                    .storage_symbols()
                    .compile_time_constants
                    .contains_key(name) =>
            {
                Ok(Vec::new())
            }
            _ => self.storage_leaf_set_commands(target_path, expr),
        }
    }

    fn storage_leaf_set_commands(
        &mut self,
        target_path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.storage_data_pack().namespace.clone();
        match expr {
            Expression::Identifier(name) => {
                if let Some(source_path) = self.storage_symbols().variable_storage_paths.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if self.is_storage_macro_param(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global args.{}",
                        namespace, target_path, namespace, name
                    )]);
                }
                if let Some(objective) = self.storage_scoreboard_objective(name) {
                    return Ok(vec![format!(
                        "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                        namespace, target_path, name, objective
                    )]);
                }
                Err(format!(
                    "Identifier '{}' cannot be stored in SNBT here; use a literal, const, runtime score, storage-backed value, or function parameter",
                    name
                ).into())
            }
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::Array(_)
            | Expression::Map(_) => self.storage_value_commands(target_path, expr),
            Expression::None => self.storage_static_snbt(expr).map(|_| Vec::new()),
            _ => {
                let id = self.storage_next_temp_id();
                let temp_score = format!("#cobble_value_{}", id);
                let mut commands = self.storage_eval_to_target(expr, &temp_score)?;
                commands.push(format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    namespace, target_path, temp_score
                ));
                Ok(commands)
            }
        }
    }
}

fn storage_child_path(base: &str, key: &str) -> String {
    if !key.is_empty()
        && key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '+')
    {
        format!("{}.{}", base, key)
    } else {
        let escaped = escape_quoted(key);
        format!("{}.\"{}\"", base, escaped)
    }
}

impl StorageValueOps for Transpiler {
    fn storage_next_temp_id(&mut self) -> u32 {
        self.next_temp_id()
    }

    fn is_storage_macro_param(&self, name: &str) -> bool {
        self.is_current_macro_param(name)
    }

    fn storage_scoreboard_objective(&self, name: &str) -> Option<&str> {
        if self.symbols.scoreboard_variables.contains(name)
            || self.symbols.variable_objectives.contains_key(name)
            || self.symbols.variables.contains_key(name)
        {
            Some(
                self.symbols
                    .variable_objectives
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("temp"),
            )
        } else {
            None
        }
    }

    fn storage_static_snbt(&self, expr: &Expression) -> Result<String, TranspileError> {
        self.serialize_to_snbt(expr)
    }
}

impl StoragePathOps for Transpiler {
    fn storage_symbols(&self) -> &Symbols {
        &self.symbols
    }

    fn storage_current_params(&self) -> &[String] {
        &self.current_context.params
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
    ) -> Result<Vec<String>, TranspileError> {
        self.evaluate_expression_to_target(expr, target)
    }
}

impl<'a> StoragePathOps for ExpressionEvaluator<'a> {
    fn storage_symbols(&self) -> &Symbols {
        self.symbols
    }

    fn storage_current_params(&self) -> &[String] {
        self.current_params
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
    ) -> Result<Vec<String>, TranspileError> {
        self.evaluate_expression_to_target(expr, target)
    }
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) struct DynamicStoragePathArg {
    pub(in crate::transpiler) placeholder: String,
    pub(in crate::transpiler) expr: Expression,
    pub(in crate::transpiler) kind: DynamicStoragePathArgKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum DynamicStoragePathArgKind {
    Index,
    Key,
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) enum StoragePathPlan {
    Static(String),
    Dynamic {
        template: String,
        args: Vec<DynamicStoragePathArg>,
    },
}

impl StoragePathPlan {
    pub(in crate::transpiler) fn static_path(path: impl Into<String>) -> Self {
        Self::Static(path.into())
    }

    pub(in crate::transpiler) fn as_static(&self) -> Option<&str> {
        match self {
            Self::Static(path) => Some(path),
            Self::Dynamic { .. } => None,
        }
    }

    pub(in crate::transpiler) fn template(&self) -> &str {
        match self {
            Self::Static(path) => path,
            Self::Dynamic { template, .. } => template,
        }
    }

    pub(in crate::transpiler) fn dynamic_args(&self) -> &[DynamicStoragePathArg] {
        match self {
            Self::Static(_) => &[],
            Self::Dynamic { args, .. } => args,
        }
    }

    pub(in crate::transpiler) fn append_field(self, field: &str) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}.{}", path, field)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}.{}", template, field),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_static_index(self, index: i32) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}[{}]", path, index)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}[{}]", template, index),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_dynamic_index(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("idx{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Index,
        });
        Self::Dynamic {
            template: format!("{}[$({})]", template, placeholder),
            args,
        }
    }

    pub(in crate::transpiler) fn append_dynamic_key(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("key{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Key,
        });
        Self::Dynamic {
            template: format!("{}.$({})", template, placeholder),
            args,
        }
    }
}
