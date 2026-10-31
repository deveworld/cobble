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
        // A function named by itself, as in `schedule.once(tick, "1t")`.
        let is_variable = self.symbols.scoreboard_variables.contains(name)
            || self.symbols.variable_storage_paths.contains_key(name)
            || self.symbols.variables.contains_key(name);
        if !is_variable && self.functions.params.contains_key(name) {
            return Ok(name.to_string());
        }
        let value = if name.starts_with("_cobble_") {
            "a value computed when the function runs".to_string()
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
        for piece in placeholders::pieces(text) {
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
