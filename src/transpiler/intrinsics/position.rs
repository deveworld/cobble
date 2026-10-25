use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn position_helper_string(
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
        if module_name != "position" {
            return Ok(None);
        }

        self.require_stdlib_module("position")?;
        let position = match method.as_str() {
            "here" => {
                if !args.is_empty() {
                    return Err("position.here() takes 0 arguments".into());
                }
                "~ ~ ~".to_string()
            }
            "absolute" => self.format_position_args(method, args, "")?,
            "relative" => self.format_position_args(method, args, "~")?,
            "local" => self.format_position_args(method, args, "^")?,
            _ => return Ok(None),
        };
        Ok(Some(position))
    }

    pub(in crate::transpiler) fn format_position_args(
        &self,
        method: &str,
        args: &[Expression],
        prefix: &str,
    ) -> Result<String, TranspileError> {
        if args.len() != 3 {
            return Err(format!("position.{}() takes 3 arguments", method).into());
        }
        args.iter()
            .map(|arg| self.format_position_component(arg, prefix))
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(" "))
    }

    pub(in crate::transpiler) fn format_position_component(
        &self,
        expr: &Expression,
        prefix: &str,
    ) -> Result<String, TranspileError> {
        let value = self.expr_to_plain_arg(expr, "position component")?;
        if prefix.is_empty() {
            Ok(value)
        } else if value == "0" {
            Ok(prefix.to_string())
        } else {
            Ok(format!("{}{}", prefix, value))
        }
    }

    pub(in crate::transpiler) fn process_position_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("position".to_string())),
            method.to_string(),
        );
        if self.position_helper_string(&func, args)?.is_some() {
            return Err(format!(
                "position.{}() returns a coordinate string; pass it to a helper that accepts a position",
                method
            ).into());
        }
        Err(format!("Unknown position function: position.{}", method).into())
    }
}
