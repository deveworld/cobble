use crate::ast::{BinaryOp, Expression};
use crate::transpiler::command_processor::CommandProcessor;
use crate::transpiler::{TranspileError, Transpiler};

/// Literal strings remain literal when a generated command reads its
/// internal placeholders. Formatted fields are conversion calls in the AST,
/// so protecting literal chunks doesn't protect the fields they produce.
pub(in crate::transpiler) fn protect_literal_strings(expr: &Expression) -> Expression {
    let protect = |value: &Expression| Box::new(protect_literal_strings(value));
    match expr {
        Expression::String(text) => {
            Expression::String(crate::transpiler::placeholders::escape_braces(text))
        }
        Expression::Array(values) => {
            Expression::Array(values.iter().map(protect_literal_strings).collect())
        }
        Expression::Map(entries) => Expression::Map(
            entries
                .iter()
                .map(|(key, value)| (key.clone(), protect_literal_strings(value)))
                .collect(),
        ),
        Expression::Binary(left, op, right) => {
            Expression::Binary(protect(left), op.clone(), protect(right))
        }
        Expression::Call(func, args) => Expression::Call(
            func.clone(),
            args.iter().map(protect_literal_strings).collect(),
        ),
        other => other.clone(),
    }
}

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
            Expression::Identifier(name)
                if self.symbols.variable_storage_paths.contains_key(name)
                    || self.symbols.scoreboard_variables.contains(name) =>
            {
                Ok(serde_json::json!({ "text": format!("{{{name}}}") }))
            }
            Expression::Binary(..)
                if crate::transpiler::desugar::has_format_field(expr)
                    || self.compile_time_text(expr).is_some() =>
            {
                Ok(serde_json::json!({"text": self.expr_to_shown_text(expr, "text component")?}))
            }
            _ => self.expr_to_json_value(expr),
        }
    }

    /// Internal component template text. Literal source strings have their
    /// braces protected by `protect_literal_strings`; formatted fields and
    /// parameters become placeholders for the command processor.
    pub(in crate::transpiler) fn expr_to_shown_text(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        match expr {
            Expression::String(s) => Ok(s.clone()),
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Ok(format!("{{{name}}}"))
            }
            Expression::Identifier(name)
                if self.symbols.variable_storage_paths.contains_key(name)
                    || self.symbols.scoreboard_variables.contains(name) =>
            {
                Ok(format!("{{{name}}}"))
            }
            Expression::Binary(left, BinaryOp::Add, right)
                if crate::transpiler::desugar::has_format_field(expr)
                    || self.compile_time_text(expr).is_some() =>
            {
                Ok(self.expr_to_shown_text(left, label)?
                    + &self.expr_to_shown_text(right, label)?)
            }
            Expression::Call(func, args)
                if matches!(&**func, Expression::Identifier(name) if name == "_cobble_format")
                    && args.len() == 1 =>
            {
                if let Some(text) = self.compile_time_text(expr) {
                    return Ok(text);
                }
                match &args[0] {
                    Expression::Identifier(name) => Ok(format!("{{{name}}}")),
                    _ => Err("An f-string field needs a name.".into()),
                }
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
                // A parameter stays a placeholder, which the command that
                // shows the component fills in as a macro argument.
                let color = match &args[1] {
                    Expression::Identifier(name) if self.current_context.is_param(name) => {
                        format!("{{{name}}}")
                    }
                    color => {
                        let color = self.expr_to_plain_arg(color, "text color")?;
                        validate_text_color(&color)?;
                        color
                    }
                };
                serde_json::json!({ "text": text, "color": color })
            }
            "score" => {
                if args.len() != 2 {
                    return Err("text.score() takes 2 arguments".into());
                }
                let name = self.expr_to_selector_arg(&args[0], "score component name")?;
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
        if matches!(method, "tellraw" | "title" | "subtitle" | "actionbar")
            && matches!(&args[1], Expression::Identifier(name)
                if self.current_context.is_param(name) || self.symbols.variable_storage_paths.contains_key(name))
        {
            let helper = format!("_cobble_text_show_{method}");
            self.require_prelude(&helper)?;
            return self.process_expression(&Expression::Call(
                Box::new(Expression::Identifier(helper)),
                args.to_vec(),
            ));
        }

        // The target is filled in already; the command processor reads the
        // placeholders of the text, so the target's braces are escaped again.
        let target = crate::transpiler::placeholders::escape_braces(
            &self.expr_to_selector_arg(&args[0], "text target")?,
        );
        let component = self.expr_to_text_component(&protect_literal_strings(&args[1]))?;
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
            safe_plain_text: self.current_context.safe_plain_text,
            symbols: &self.symbols,
        };
        let mut commands = processor.template_copy_commands(&command)?;
        let processed = processor.process_template_string(&command)?;
        if !commands.is_empty() {
            self.data_pack.track_objective("temp");
        }
        commands.extend(self.quoted_hover_commands(&command, processed));
        self.push_current_commands(commands)
    }
}
