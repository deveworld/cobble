use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{GeneratedCommandKind, StoragePathPlan, TranspileError, Transpiler};

impl Transpiler {
    /// Helper method to evaluate a complex expression into a target variable
    /// Returns the commands needed to compute the expression
    pub(in crate::transpiler) fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let caller_recursive = self.in_recursive_function();
        let numeric_backend = self.uses_storage_numbers();
        if numeric_backend {
            self.prepare_numeric_expression(expr)?;
        }
        let mut evaluator = ExpressionEvaluator {
            data_pack: &mut self.data_pack,
            symbols: &self.symbols,
            functions: &mut self.functions,
            current_params: &self.current_context.params,
            temp_counter: &mut self.temp_counter,
            source: self.current_statement_source.clone(),
            caller_recursive,
            warnings: &self.warnings,
            numeric_scope: self.current_function_name.clone(),
            numeric_locals: self.current_context.locals.to_vec(),
            numeric_backend,
        };
        evaluator.evaluate_expression_to_target(expr, target)
    }

    /// A list method that a storage command does directly.
    fn process_list_method(
        &mut self,
        base: &Expression,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let Some(plan) = self.resolve_storage_path_plan(base) else {
            return Err(format!("`{method}` needs a list held in a variable.").into());
        };
        let namespace = self.data_pack.namespace.clone();
        let expected = match method {
            "append" | "extend" => 1..=1,
            "pop" => 0..=1,
            _ => 0..=0,
        };
        if !expected.contains(&args.len()) {
            return Err(format!("`{method}` got {} argument(s).", args.len()).into());
        }
        let commands = match method {
            "clear" => {
                let empty = if self.value_read_type(base) == CobbleType::Map {
                    "{}"
                } else {
                    "[]"
                };
                self.path_commands(&plan, |path| {
                    format!("data modify storage {namespace}:global {path} set value {empty}")
                })?
            }
            "pop" => {
                let item = Expression::Subscript(
                    Box::new(base.clone()),
                    Box::new(args.first().cloned().unwrap_or(Expression::Number(-1.0))),
                );
                let Some(item_plan) = self.resolve_storage_path_plan(&item) else {
                    return Err("`pop` needs a list held in a variable.".into());
                };
                self.path_commands(&item_plan, |path| {
                    format!("data remove storage {namespace}:global {path}")
                })?
            }
            _ => {
                // `extend` appends every item of the value.
                let items = if method == "extend" { "[]" } else { "" };
                let (mut commands, value_path) = self.materialize_storage_value(&args[0])?;
                commands.extend(self.path_commands(&plan, |path| {
                    format!(
                        "data modify storage {namespace}:global {path} append from storage {namespace}:global {value_path}{items}"
                    )
                })?);
                commands.push(format!(
                    "data remove storage {namespace}:global {value_path}"
                ));
                commands
            }
        };
        self.extend_current_function(commands);
        Ok(())
    }

    fn value_read_type(&self, expr: &Expression) -> CobbleType {
        match expr {
            Expression::Subscript(..) | Expression::Attribute(..) => self.storage_read_type(expr),
            expr => self.infer_type(expr),
        }
    }

    pub(in crate::transpiler) fn process_expression(
        &mut self,
        expr: &Expression,
    ) -> Result<(), TranspileError> {
        match expr {
            Expression::Call(func, args) => {
                if self.runtime_intrinsic_call(func, args)? {
                    return Ok(());
                }
                // Handle both Identifier and Attribute (for stdlib.addEventListener)
                match &**func {
                    Expression::Identifier(func_name) => {
                        // Check for standalone addEventListener
                        if func_name == "addEventListener" {
                            self.require_stdlib_module("event")?;
                            self.process_add_event_listener(args)?;
                        } else if func_name.contains('.') {
                            // Method call on module (e.g., stdlib.addEventListener)
                            let parts: Vec<&str> = func_name.split('.').collect();
                            if parts.len() == 2
                                && parts[0] == "stdlib"
                                && parts[1] == "addEventListener"
                            {
                                self.require_stdlib_module("event")?;
                                self.process_add_event_listener(args)?;
                            } else {
                                return Err(format!("Unknown module method: {}", func_name).into());
                            }
                        } else {
                            // Regular function call - generate Minecraft function call
                            self.generate_function_call(func_name, args)?;
                        }
                    }
                    Expression::Attribute(obj, method)
                        if self.is_list_value(obj)
                            && (Self::is_list_command_method(method) || method == "pop") =>
                    {
                        self.process_list_method(obj, method, args)?;
                    }
                    Expression::Attribute(obj, method) => {
                        // Handle attribute access like stdlib.addEventListener
                        if let Some(module_name) = Self::expression_path(obj) {
                            if module_name == "stdlib" && method == "addEventListener" {
                                self.require_stdlib_module("event")?;
                                self.process_add_event_listener(args)?;
                            } else if module_name == "math" {
                                self.require_stdlib_module("math")?;
                                self.process_math_intrinsic(method, args)?;
                            } else if module_name == "text" {
                                self.require_stdlib_module("text")?;
                                self.process_text_intrinsic(method, args)?;
                            } else if module_name == "score" {
                                self.require_stdlib_module("score")?;
                                self.process_score_intrinsic(method, args)?;
                            } else if module_name == "score.objective" {
                                self.require_stdlib_module("score.objective")?;
                                self.process_score_objective_intrinsic(method, args)?;
                            } else if module_name == "random" {
                                self.require_stdlib_module("random")?;
                                self.process_random_intrinsic(method, args)?;
                            } else if module_name == "timer" {
                                self.require_stdlib_module("timer")?;
                                self.process_timer_intrinsic(method, args)?;
                            } else if module_name == "storage" {
                                self.require_stdlib_module("storage")?;
                                self.process_storage_intrinsic(method, args)?;
                            } else if module_name == "item_component" {
                                self.require_stdlib_module("item_component")?;
                                self.process_item_component_intrinsic(method, args)?;
                            } else if module_name == "schedule" {
                                self.require_stdlib_module("schedule")?;
                                self.process_schedule_intrinsic(method, args)?;
                            } else if module_name == "selector" {
                                self.require_stdlib_module("selector")?;
                                self.process_selector_intrinsic(method, args)?;
                            } else if module_name == "position" {
                                self.require_stdlib_module("position")?;
                                self.process_position_intrinsic(method, args)?;
                            } else if module_name == "bossbar" {
                                self.require_stdlib_module("bossbar")?;
                                self.process_bossbar_intrinsic(method, args)?;
                            } else if module_name == "team" {
                                self.require_stdlib_module("team")?;
                                self.process_team_intrinsic(method, args)?;
                            } else if module_name == "entity" {
                                self.require_stdlib_module("entity")?;
                                self.process_entity_intrinsic(method, args)?;
                            } else if module_name == "datapack" {
                                self.require_stdlib_module("datapack")?;
                                self.process_datapack_intrinsic(method, args)?;
                            } else if module_name == "resource_pack" {
                                self.require_stdlib_module("resource_pack")?;
                                self.process_resource_pack_intrinsic(method, args)?;
                            } else if let Some(func_name) = self
                                .resolve_user_function_call_name(func)
                                .map(str::to_string)
                            {
                                self.generate_function_call(&func_name, args)?;
                            } else {
                                return Err(format!(
                                    "Unknown module method: {}.{}",
                                    module_name, method
                                )
                                .into());
                            }
                        } else {
                            return Err("Complex attribute access not supported"
                                .to_string()
                                .into());
                        }
                    }
                    _ => {
                        return Err("Unsupported function call expression".into());
                    }
                }
            }
            _ => {
                // Other expressions (like standalone identifiers for docstrings) don't generate commands
            }
        }
        Ok(())
    }

    /// A storage-backed string variable's scoreboard mirror only holds the
    /// string length (`data get` on a string), so reading it as a score in
    /// arithmetic silently computes garbage. Reject it up front.
    pub(in crate::transpiler) fn reject_string_score_operand(
        &self,
        name: &str,
    ) -> Result<(), TranspileError> {
        if self.symbols.variable_storage_paths.contains_key(name)
            && matches!(
                self.symbols.variable_types.get(name),
                Some(CobbleType::String)
            )
        {
            return Err(format!(
                "Variable '{}' holds a storage-backed string and cannot be used in arithmetic; its scoreboard mirror only stores the string length. Use integer values instead.",
                name
            ).into());
        }
        Ok(())
    }

    pub(in crate::transpiler) fn expr_to_plain_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        match expr {
            // The text goes into the command as is, so a line break would
            // split it.
            Expression::String(s) if s.contains(['\n', '\r']) => {
                Err(format!("{} can't contain a line break", label).into())
            }
            Expression::String(s) => self.fill_helper_placeholders(s, label),
            Expression::Identifier(name) => self.helper_name_arg(name, label),
            Expression::Number(n) if n.fract() == 0.0 => Ok((*n as i32).to_string()),
            Expression::Number(n) => Ok(n.to_string()),
            Expression::Binary(..) | Expression::Unary(..)
                if self.try_eval_const(expr).is_some() =>
            {
                let value = self.try_eval_const(expr).unwrap();
                Ok(if value.fract() == 0.0 {
                    format!("{value:.0}")
                } else {
                    value.to_string()
                })
            }
            Expression::Call(func, args) => {
                if let Some(value) = self.storage_path_helper_string(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.selector_helper_string(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.position_helper_string(func, args)? {
                    Ok(value)
                } else {
                    Err(format!("{} must be a string, identifier, or number", label).into())
                }
            }
            _ => Err(format!("{} must be a string, identifier, or number", label).into()),
        }
    }

    pub(in crate::transpiler) fn expr_to_selector_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        let value = self.expr_to_plain_arg(expr, label)?;
        Ok(self.expand_selector_alias_arg(value))
    }

    pub(in crate::transpiler) fn expand_selector_alias_arg(&self, value: String) -> String {
        let Some(alias) = value.strip_prefix('@') else {
            return value;
        };
        self.symbols
            .selector_aliases
            .get(alias)
            .cloned()
            .unwrap_or(value)
    }

    pub(in crate::transpiler) fn expr_to_i32(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<i32, TranspileError> {
        let value = self
            .try_eval_const(expr)
            .filter(|value| {
                value.is_finite()
                    && value.fract() == 0.0
                    && *value >= f64::from(i32::MIN)
                    && *value <= f64::from(i32::MAX)
            })
            .ok_or_else(|| {
                TranspileError::from(format!(
                    "{label} must be a 32-bit integer literal or constant expression"
                ))
            })?;
        Ok(value as i32)
    }

    pub(in crate::transpiler) fn expr_to_bool(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<bool, TranspileError> {
        match expr {
            Expression::Boolean(value) => Ok(*value),
            _ => Err(format!("{} must be a literal boolean", label).into()),
        }
    }

    pub(in crate::transpiler) fn expr_to_bool_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        match expr {
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Ok(format!("$({name})"))
            }
            _ => self
                .expr_to_bool(expr, label)
                .map(|value| value.to_string()),
        }
    }

    pub(in crate::transpiler) fn dynamic_storage_command<F>(
        &mut self,
        plan: &StoragePathPlan,
        kind: GeneratedCommandKind,
        build_helper_commands: F,
    ) -> Result<Vec<String>, TranspileError>
    where
        F: Fn(&str) -> Vec<String>,
    {
        let id = self.next_temp_id();
        let (mut commands, args_path) = self.prepare_dynamic_storage_path(id, plan)?;
        let helper_name = format!("_cobble_storage_dyn_{}", id);
        let helper_commands = plan.helper_lines(id, build_helper_commands);
        self.add_generated_helper_function(helper_name.clone(), helper_commands, kind);
        commands.push(format!(
            "function {}:{} with storage {}:global {}",
            self.data_pack.namespace, helper_name, self.data_pack.namespace, args_path
        ));
        commands.push(format!(
            "data remove storage {}:global {}",
            self.data_pack.namespace, args_path
        ));
        Ok(commands)
    }

    pub(in crate::transpiler) fn qualify_namespaced_id(&self, value: &str) -> String {
        if value.contains(':') {
            value.to_string()
        } else {
            format!("{}:{}", self.data_pack.namespace, value)
        }
    }

    pub(in crate::transpiler) fn expr_to_json_value(
        &self,
        expr: &Expression,
    ) -> Result<serde_json::Value, TranspileError> {
        if let Some(text) = self.compile_time_text(expr) {
            return Ok(serde_json::Value::String(text));
        }
        if matches!(expr, Expression::Binary(..)) && super::desugar::has_format_field(expr) {
            return Ok(serde_json::Value::String(
                self.expr_to_shown_text(expr, "formatted text")?,
            ));
        }
        match expr {
            Expression::Number(n) => self.json_number(*n),
            Expression::Call(func, args) if matches!(&**func, Expression::Identifier(name) if name == "_cobble_float") && !args.is_empty() => self.expr_to_json_value(&args[0]),
            Expression::Unary(UnaryOp::Neg | UnaryOp::Pos, inner) if matches!(&**inner, Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if name == "_cobble_float")) => {
                let mut value = super::numbers::numeric_literal(inner).ok_or("Invalid number literal")?;
                if matches!(expr, Expression::Unary(UnaryOp::Neg, _)) { value = -value; }
                self.json_number(value)
            },
            Expression::Unary(UnaryOp::Neg | UnaryOp::Pos, _)
                if Self::signed_number(expr).is_some() =>
            {
                self.json_number(Self::signed_number(expr).unwrap_or_default())
            }
            Expression::String(s) => Ok(serde_json::json!(s)),
            Expression::Boolean(b) => Ok(serde_json::json!(b)),
            Expression::None => Ok(serde_json::Value::Null),
            Expression::Array(items) => items
                .iter()
                .map(|item| self.expr_to_json_value(item))
                .collect::<Result<Vec<_>, _>>()
                .map(serde_json::Value::Array),
            Expression::Map(entries) => {
                let mut object = serde_json::Map::new();
                for (key, value) in entries {
                    object.insert(key.clone(), self.expr_to_json_value(value)?);
                }
                Ok(serde_json::Value::Object(object))
            }
            Expression::Call(func, args) => {

                if let Some(value) = self.item_component_helper_json(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.text_component_helper_json(func, args)? {
                    Ok(value)
                } else {
                    Err("Only text.* component helpers and item_component.* helpers can be serialized to JSON here".into())
                }
            }
            Expression::Identifier(name) => {
                if let Some(value) = self.symbols.compile_time_constants.get(name) {
                    self.json_number(*value)
                } else if self.current_context.is_param(name) || self.symbols.variable_storage_paths.contains_key(name) {
                    Ok(serde_json::Value::String(format!("{{{name}}}")))
                } else {
                    Err(format!(
                        "Identifier '{}' cannot be serialized to JSON here; use a literal or const",
                        name
                    )
                    .into())
                }
            }
            // Text joined from literals is known when the pack is built.
            Expression::Binary(left, BinaryOp::Add, right)
                if matches!(
                    (left.as_ref(), right.as_ref()),
                    (Expression::String(_) | Expression::Binary(..), Expression::String(_))
                ) =>
            {
                match (self.expr_to_json_value(left), self.expr_to_json_value(right)) {
                    (Ok(serde_json::Value::String(left)), Ok(serde_json::Value::String(right))) => {
                        Ok(serde_json::Value::String(left + &right))
                    }
                    _ => Err(format!(
                        "`{}` can't be worked out in resource JSON; write the result as a literal",
                        Self::json_expression_text(expr)
                    )
                    .into()),
                }
            }
            Expression::Binary(..) | Expression::Unary(..) | Expression::Conditional(..) => {
                Err(format!(
                    "`{}` can't be worked out in resource JSON; write the result as a literal",
                    Self::json_expression_text(expr)
                )
                .into())
            }
            _ => Err(format!(
                "`{}` can't be written into resource JSON; use a literal, a const, or a text.* or item_component.* helper",
                Self::expression_source_key(expr)
            )
            .into()),
        }
    }

    /// `expr` as compact source text, with the spaces a word operator
    /// needs, as in `not True` and `1 in [1]`.
    fn json_expression_text(expr: &Expression) -> String {
        match expr {
            Expression::Unary(UnaryOp::Not, inner) => {
                format!("not {}", Self::json_expression_text(inner))
            }
            Expression::Binary(left, op, right)
                if matches!(
                    op,
                    BinaryOp::And | BinaryOp::Or | BinaryOp::In | BinaryOp::NotIn
                ) =>
            {
                let op = match op {
                    BinaryOp::NotIn => "not in",
                    op => Self::binary_op_source_key(op),
                };
                format!(
                    "{} {op} {}",
                    Self::json_expression_text(left),
                    Self::json_expression_text(right)
                )
            }
            Expression::Conditional(condition, value, otherwise) => format!(
                "{} if {} else {}",
                Self::json_expression_text(value),
                Self::json_expression_text(condition),
                Self::json_expression_text(otherwise)
            ),
            expr => Self::expression_source_key(expr),
        }
    }

    /// A number literal with any leading `-` and `+` applied, as in `-(2)`.
    pub(in crate::transpiler) fn signed_number(expr: &Expression) -> Option<f64> {
        match expr {
            Expression::Number(n) => Some(*n),
            Expression::Unary(UnaryOp::Neg, inner) => Self::signed_number(inner).map(|n| -n),
            Expression::Unary(UnaryOp::Pos, inner) => Self::signed_number(inner),
            _ => None,
        }
    }

    /// A number in resource JSON. A whole number keeps its exact value: up
    /// below 2^53 the number itself, and past that the integer written in the
    /// statement's source, since an `f64` can't hold every such integer.
    /// A number JSON can't hold is a compile error, never clamped.
    fn json_number(&self, n: f64) -> Result<serde_json::Value, TranspileError> {
        const EXACT: f64 = 9_007_199_254_740_992.0; // 2^53
        if !n.is_finite() {
            return Err(
                "A number in resource JSON is too large: JSON has no infinity"
                    .to_string()
                    .into(),
            );
        }
        if n.fract() != 0.0 {
            return Ok(serde_json::json!(n));
        }
        if n.abs() < EXACT {
            return Ok(serde_json::json!(n as i64));
        }
        match self.integer_literal_text(n) {
            // Written with a decimal point or an exponent: a float, as in
            // Python.
            Ok(None) => Ok(serde_json::json!(n)),
            Ok(Some(text)) => text
                .parse::<i64>()
                .map(serde_json::Number::from)
                .or_else(|_| text.parse::<u64>().map(serde_json::Number::from))
                .map(serde_json::Value::Number)
                .map_err(|_| {
                    format!(
                        "The integer {text} doesn't fit in 64 bits, so it can't be written into resource JSON exactly; Minecraft's longs go from -9223372036854775808 to 9223372036854775807"
                    )
                    .into()
                }),
            Err(candidates) if candidates.len() > 1 => Err(format!(
                "The integers {} in this declaration read as the same number past 2^53, so Cobble can't keep them apart; declare them in separate resources",
                candidates.join(" and ")
            )
            .into()),
            Err(_) => Err(format!(
                "The integer {n} is past 2^53, where Cobble needs its digits from the source; write it as a literal in this declaration"
            )
            .into()),
        }
    }
}
