use crate::ast::{CobbleType, Expression};
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
        let mut evaluator = ExpressionEvaluator {
            data_pack: &mut self.data_pack,
            symbols: &self.symbols,
            functions: &mut self.functions,
            current_params: &self.current_context.params,
            temp_counter: &mut self.temp_counter,
            source: self.current_statement_source.clone(),
            caller_recursive,
            warnings: &self.warnings,
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
            Expression::String(s) | Expression::Identifier(s) => Ok(s.clone()),
            Expression::Number(n) if n.fract() == 0.0 => Ok((*n as i32).to_string()),
            Expression::Number(n) => Ok(n.to_string()),
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
        match expr {
            Expression::Number(n) => Ok(*n as i32),
            Expression::Identifier(name) => self
                .symbols
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32)
                .ok_or_else(|| format!("{} must be a literal integer or const", label))
                .map_err(TranspileError::from),
            _ => Err(format!("{} must be a literal integer or const", label).into()),
        }
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

    pub(in crate::transpiler) fn dynamic_storage_command<F>(
        &mut self,
        plan: &StoragePathPlan,
        kind: GeneratedCommandKind,
        build_helper_commands: F,
    ) -> Result<Vec<String>, TranspileError>
    where
        F: FnOnce(&str) -> Vec<String>,
    {
        let id = self.next_temp_id();
        let (mut commands, args_path) = self.prepare_dynamic_storage_path(id, plan)?;
        let helper_name = format!("_cobble_storage_dyn_{}", id);
        let helper_commands = build_helper_commands(plan.template());
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
        match expr {
            Expression::Number(n) if n.fract() == 0.0 => Ok(serde_json::json!(*n as i32)),
            Expression::Number(n) => Ok(serde_json::json!(n)),
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
                    if value.fract() == 0.0 {
                        Ok(serde_json::json!(*value as i32))
                    } else {
                        Ok(serde_json::json!(value))
                    }
                } else {
                    Err(format!(
                        "Identifier '{}' cannot be serialized to JSON here; use a literal or const",
                        name
                    )
                    .into())
                }
            }
            _ => Err(format!("Unsupported expression in JSON value: {:?}", expr).into()),
        }
    }
}
