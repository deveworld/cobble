use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn item_component_helper_json(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<serde_json::Value>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "item_component" {
            return Ok(None);
        }

        self.require_stdlib_module("item_component")?;
        let value = match method.as_str() {
            "components" => {
                if args.is_empty() {
                    return Err("item_component.components() takes at least 1 argument"
                        .to_string()
                        .into());
                }
                let mut merged = serde_json::Map::new();
                for arg in args {
                    let component_map = self.expr_to_item_component_map(arg)?;
                    for (key, value) in component_map {
                        if merged.contains_key(&key) {
                            return Err(format!(
                                "item_component.components() duplicate component '{}'",
                                key
                            )
                            .into());
                        }
                        merged.insert(key, value);
                    }
                }
                serde_json::Value::Object(merged)
            }
            "custom_name" => {
                if args.len() != 1 {
                    return Err("item_component.custom_name() takes 1 argument"
                        .to_string()
                        .into());
                }
                serde_json::json!({
                    "minecraft:custom_name": self.expr_to_text_component_value(&args[0])?
                })
            }
            "lore" => {
                if args.len() != 1 {
                    return Err("item_component.lore() takes 1 argument".into());
                }
                let Expression::Array(entries) = &args[0] else {
                    return Err("item_component.lore() argument must be an array"
                        .to_string()
                        .into());
                };
                let lore = entries
                    .iter()
                    .map(|entry| self.expr_to_text_component_value(entry))
                    .collect::<Result<Vec<_>, _>>()?;
                serde_json::json!({ "minecraft:lore": lore })
            }
            "unbreakable" => {
                if !args.is_empty() {
                    return Err("item_component.unbreakable() takes 0 arguments"
                        .to_string()
                        .into());
                }
                serde_json::json!({ "minecraft:unbreakable": {} })
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    pub(in crate::transpiler) fn expr_to_item_component_map(
        &self,
        expr: &Expression,
    ) -> Result<serde_json::Map<String, serde_json::Value>, TranspileError> {
        let value = self.expr_to_json_value(expr)?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| {
                "item_component.components() arguments must be component objects".to_string()
            })
            .map_err(TranspileError::from)
    }

    pub(in crate::transpiler) fn process_item_component_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("item_component".to_string())),
            method.to_string(),
        );
        if self.item_component_helper_json(&func, args)?.is_some() {
            return Err(format!(
                "item_component.{}() returns an item component JSON object; pass it to datapack.item_modifier() or another JSON value",
                method
            ).into());
        }
        Err(format!("Unknown item component function: item_component.{}", method).into())
    }
}
