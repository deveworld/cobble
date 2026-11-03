//! Stdlib module calls, one file per module.

mod bossbar;
mod datapack;
mod entity;
mod item_component;
mod math;
mod position;
mod resource_pack;
mod schedule;
mod score;
mod selector;
mod storage;
mod team;
pub(in crate::transpiler) mod text;

use crate::ast::Expression;
use crate::transpiler::placeholders::{self, Piece};
use crate::transpiler::{TranspileError, Transpiler};

/// Whether `name` reads as a Python name, which refers to a value. Other
/// identifier tokens, such as the selector `@s`, the holder `#count`, or the
/// id `minecraft:speed`, are text.
pub(in crate::transpiler) fn is_value_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && chars.all(|ch| ch.is_alphanumeric() || ch == '_')
}

/// Whether a helper argument holds a macro argument, whose value is only
/// known when the function runs.
pub(in crate::transpiler) fn is_macro_arg(value: &str) -> bool {
    value.contains("$(")
}

impl Transpiler {
    /// The text a helper writes for the name `name`: a parameter becomes
    /// the macro argument `$(name)`, which makes the helper's command a
    /// macro line, and a const its value. Any other value is only known when
    /// the function runs, so it is an error; a helper never writes a
    /// variable's own name.
    pub(in crate::transpiler) fn helper_name_arg(
        &self,
        name: &str,
        label: &str,
    ) -> Result<String, TranspileError> {
        if !is_value_name(name) {
            return Ok(name.to_string());
        }
        if self.current_context.is_param(name) {
            return Ok(format!("$({})", name));
        }
        if let Some(value) = self.symbols.compile_time_constants.get(name) {
            return Ok(if value.fract() == 0.0 {
                format!("{}", *value as i64)
            } else {
                value.to_string()
            });
        }
        let is_variable = self.symbols.scoreboard_variables.contains(name)
            || self.symbols.variable_storage_paths.contains_key(name)
            || self.symbols.variables.contains_key(name);
        let value = if name.starts_with("_cobble_") {
            "a value computed when the function runs".to_string()
        } else if !is_variable && self.functions.params.contains_key(name) {
            // Only `schedule.once` and `schedule.clear` take a function by
            // its name; see `Self::expr_to_function_arg`.
            format!(
                "the function `{}`. Write its name as a string, \"{}\"",
                name, name
            )
        } else {
            format!("the variable `{}`", name)
        };
        Err(format!(
            "{label} must be a literal, a const, or a parameter of this function, not {value}. \
             To use a value known only when the function runs, pass it to a function parameter \
             and call the helper there."
        )
        .into())
    }

    /// A resource file is written when the pack is built, so text that a
    /// `text` or `item_component` helper wrote for a parameter or a
    /// variable, as `text.plain(p)` writes `{p}`, has no value to show.
    pub(in crate::transpiler) fn check_resource_json_placeholders(
        &self,
        json: &serde_json::Value,
    ) -> Result<(), TranspileError> {
        let is_runtime = |name: &str| {
            self.current_context.is_param(name)
                || self.symbols.scoreboard_variables.contains(name)
                || self.symbols.variable_storage_paths.contains_key(name)
        };
        match placeholders::runtime_placeholder_in_json(json, &is_runtime) {
            None => Ok(()),
            Some(raw) => Err(format!(
                "A resource file is written when the pack is built, so `{raw}` has no value to show \
                 here: a parameter or a variable is known only when the function runs. Write the \
                 text, or a const, in the resource."
            )
            .into()),
        }
    }

    /// A function helper argument, as `schedule.once(tick, "1t")` takes:
    /// a function named by itself names that function.
    pub(in crate::transpiler) fn expr_to_function_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        if let Expression::Identifier(name) = expr {
            let is_variable = self.symbols.scoreboard_variables.contains(name)
                || self.symbols.variable_storage_paths.contains_key(name)
                || self.symbols.variables.contains_key(name)
                || self.current_context.is_param(name);
            if !is_variable && self.functions.params.contains_key(name) {
                return Ok(name.to_string());
            }
        }
        self.expr_to_plain_arg(expr, label)
    }

    /// A whole-number helper argument: a literal, a const, or a parameter,
    /// which becomes the macro argument `$(name)`.
    pub(in crate::transpiler) fn expr_to_int_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        match expr {
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Ok(format!("$({})", name))
            }
            _ => self
                .expr_to_i32(expr, label)
                .map(|value| value.to_string())
                .map_err(|_| {
                    format!(
                        "{label} must be a literal integer, a const, or a parameter of this function. \
                         To use a value known only when the function runs, pass it to a function \
                         parameter and call the helper there."
                    )
                    .into()
                }),
        }
    }

    /// A text component helper argument, such as a team's display name,
    /// written into a command: a parameter, or a `{name}` in a string,
    /// becomes a macro argument, as in a raw command.
    pub(in crate::transpiler) fn expr_to_helper_text_component(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        if let Expression::Identifier(name) = expr {
            if name.starts_with("_cobble_component_") && self.current_context.is_param(name) {
                return Ok(format!("$({name})"));
            }
        }
        let component = self.expr_to_text_component(&text::protect_literal_strings(expr))?;
        self.fill_helper_placeholders(&component, label)
    }

    /// `text` with its `{name}` placeholders filled in as
    /// [`Self::helper_name_arg`] fills a name, and `{{` and `}}` read as
    /// literal braces, as in a raw command.
    pub(in crate::transpiler) fn fill_helper_placeholders(
        &self,
        text: &str,
        label: &str,
    ) -> Result<String, TranspileError> {
        if !text.contains(['{', '}']) {
            return Ok(text.to_string());
        }
        let mut filled = String::with_capacity(text.len());
        for piece in placeholders::checked_pieces(text)? {
            match piece {
                Piece::Text { text, .. } => filled.push_str(&text),
                Piece::Field { name, .. } if is_value_name(name) => {
                    filled.push_str(&self.helper_name_arg(name, label)?)
                }
                Piece::Field { raw, .. } => filled.push_str(raw),
            }
        }
        Ok(filled)
    }
}
