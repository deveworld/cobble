use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn selector_helper_string(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<String>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "selector" {
            return Ok(None);
        }

        self.require_stdlib_module("selector")?;
        let selector = match method.as_str() {
            "current" => {
                if !args.is_empty() {
                    return Err("selector.current() takes 0 arguments".into());
                }
                "@s".to_string()
            }
            "nearest_player" => {
                if !args.is_empty() {
                    return Err("selector.nearest_player() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@p".to_string()
            }
            "all_players" => {
                if !args.is_empty() {
                    return Err("selector.all_players() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@a".to_string()
            }
            "all_entities" => {
                if !args.is_empty() {
                    return Err("selector.all_entities() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@e".to_string()
            }
            "tagged" | "players_tagged" => {
                if args.len() != 1 {
                    return Err(format!("selector.{}() takes 1 argument", method).into());
                }
                let tag = self.expr_to_plain_arg(&args[0], "selector tag")?;
                if method == "players_tagged" {
                    format!("@a[tag={}]", tag)
                } else {
                    format!("@e[tag={}]", tag)
                }
            }
            "entity_type" => {
                if args.len() != 1 {
                    return Err("selector.entity_type() takes 1 argument".into());
                }
                let entity_type = self.expr_to_plain_arg(&args[0], "selector entity type")?;
                format!("@e[type={}]", entity_type)
            }
            _ => return Ok(None),
        };
        Ok(Some(selector))
    }

    pub(in crate::transpiler) fn process_selector_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("selector".to_string())),
            method.to_string(),
        );
        if self.selector_helper_string(&func, args)?.is_some() {
            return Err(format!(
                "selector.{}() returns a selector string; pass it to a helper that accepts a selector",
                method
            ).into());
        }
        Err(format!("Unknown selector function: selector.{}", method).into())
    }
}
