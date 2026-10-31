use crate::ast::Expression;
use crate::transpiler::command_processor::CommandProcessor;
use crate::transpiler::{TranspileError, Transpiler};

pub(in crate::transpiler) fn validate_text_color(color: &str) -> Result<(), TranspileError> {
    const NAMED_COLORS: &[&str] = &[
        "black",
        "dark_blue",
        "dark_green",
        "dark_aqua",
        "dark_red",
        "dark_purple",
        "gold",
        "gray",
        "dark_gray",
        "blue",
        "green",
        "aqua",
        "red",
        "light_purple",
        "yellow",
        "white",
    ];

    let valid_hex = color.len() == 7
        && color.starts_with('#')
        && color.chars().skip(1).all(|c| c.is_ascii_hexdigit());
    if NAMED_COLORS.contains(&color) || valid_hex {
        Ok(())
    } else {
        Err(format!(
            "Invalid text color '{}': use a Minecraft named color or #RRGGBB",
            color
        )
        .into())
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn expr_to_text_component(
        &self,
        expr: &Expression,
    ) -> Result<String, TranspileError> {
        let value = self.expr_to_text_component_value(expr)?;
        serde_json::to_string(&value)
            .map_err(|e| format!("Failed to encode text component: {}", e))
            .map_err(TranspileError::from)
    }

    pub(in crate::transpiler) fn expr_to_text_component_value(
        &self,
        expr: &Expression,
    ) -> Result<serde_json::Value, TranspileError> {
        match expr {
            Expression::String(s) => Ok(serde_json::json!({ "text": s })),
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Ok(serde_json::json!({ "text": format!("{{{name}}}") }))
            }
            _ => self.expr_to_json_value(expr),
        }
    }

    /// The text of a component: a string keeps its `{name}` placeholders,
    /// and a parameter becomes one, so the command shows them as text
    /// components as a raw `tellraw` does.
    fn expr_to_shown_text(&self, expr: &Expression, label: &str) -> Result<String, TranspileError> {
        match expr {
            Expression::String(s) => Ok(s.clone()),
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Ok(format!("{{{name}}}"))
            }
            _ => self.expr_to_plain_arg(expr, label),
        }
    }

    pub(in crate::transpiler) fn text_component_helper_json(
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
        if module_name != "text" {
            return Ok(None);
        }

        self.require_stdlib_module("text")?;
        let value = match method.as_str() {
            "plain" => {
                if args.len() != 1 {
                    return Err("text.plain() takes 1 argument".into());
                }
                let text = self.expr_to_shown_text(&args[0], "text.plain text")?;
                serde_json::json!({ "text": text })
            }
            "colored" => {
                if args.len() != 2 {
                    return Err("text.colored() takes 2 arguments".into());
                }
                let text = self.expr_to_shown_text(&args[0], "text.colored text")?;
                let color = self.expr_to_plain_arg(&args[1], "text color")?;
                validate_text_color(&color)?;
                serde_json::json!({ "text": text, "color": color })
            }
            "score" => {
                if args.len() != 2 {
                    return Err("text.score() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score component name")?;
                let objective = self.expr_to_plain_arg(&args[1], "score component objective")?;
                serde_json::json!({ "score": { "name": name, "objective": objective } })
            }
            "selector" => {
                if args.len() != 1 {
                    return Err("text.selector() takes 1 argument".into());
                }
                let selector = self.expr_to_selector_arg(&args[0], "selector component")?;
                serde_json::json!({ "selector": selector })
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    pub(in crate::transpiler) fn process_text_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if matches!(method, "plain" | "colored" | "score" | "selector") {
            return Err(format!(
                "text.{}() returns a JSON text component; pass it to text.tellraw(), text.title(), or another JSON value",
                method
            ).into());
        }
        if args.len() != 2 {
            return Err(format!("text.{}() takes 2 arguments", method).into());
        }

        let target = self.expr_to_selector_arg(&args[0], "text target")?;
        let component = self.expr_to_text_component(&args[1])?;
        let command = match method {
            "tellraw" => format!("tellraw {} {}", target, component),
            "title" | "subtitle" | "actionbar" => {
                format!("title {} {} {}", target, method, component)
            }
            _ => return Err(format!("Unknown text function: text.{}", method).into()),
        };
        // The text takes `{name}` placeholders as a raw `tellraw` does.
        let processor = CommandProcessor {
            namespace: self.data_pack.namespace.clone(),
            current_params: &self.current_context.params,
            symbols: &self.symbols,
        };
        let mut commands = processor.text_copy_commands(&command)?;
        commands.push(processor.process_command_string(&command)?);
        self.push_current_commands(commands)
    }
}
