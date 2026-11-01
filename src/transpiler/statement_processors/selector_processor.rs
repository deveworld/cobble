use crate::ast::*;
use crate::transpiler::placeholders::{self, Piece};
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_selector_def(
        &mut self,
        selector_def: &SelectorDef,
    ) -> Result<(), TranspileError> {
        // Validate selector name doesn't conflict with built-in selectors
        const BUILTIN_SELECTORS: &[&str] = &["a", "p", "r", "e", "s"];
        if BUILTIN_SELECTORS.contains(&selector_def.name.as_str()) {
            return Err(format!(
                "Cannot redefine built-in selector '@{}'. Use a different name.\n\
                Built-in selectors: @a (all players), @p (nearest player), @r (random player), \
                @e (all entities), @s (executing entity)",
                selector_def.name
            )
            .into());
        }

        // Validate selector syntax (must start with @)
        if !selector_def.selector.starts_with('@') {
            return Err(format!(
                "Invalid selector syntax: '{}'. Selectors must start with '@'.\n\
                Example: @Player = @a[type=player]",
                selector_def.selector
            )
            .into());
        }

        // A `{CONST}` in the selector is filled in here, so the alias reads
        // the same in a raw command, a text component, and a header.
        let selector = self.fill_alias_constants(selector_def)?;

        // Re-processing an imported file as a directory entry can encounter the
        // same alias again. Identical definitions are idempotent.
        if self
            .symbols
            .selector_aliases
            .get(&selector_def.name)
            .is_some_and(|existing| existing == &selector)
        {
            return Ok(());
        }

        if self
            .symbols
            .selector_aliases
            .contains_key(&selector_def.name)
        {
            self.warn(
                "selector-redefined",
                format!(
                    "`@{}` is already defined. This definition replaces the earlier one.",
                    selector_def.name
                ),
                None,
            );
        }

        // Store the selector alias
        // @Player -> @a[type=player,gamemode=survival]
        self.symbols
            .selector_aliases
            .insert(selector_def.name.clone(), selector);

        Ok(())
    }

    /// The alias's selector with each `{CONST}` replaced by its value. An
    /// alias is defined once for the whole pack, so any other placeholder
    /// has no value to take.
    fn fill_alias_constants(&self, selector_def: &SelectorDef) -> Result<String, TranspileError> {
        let mut filled = String::with_capacity(selector_def.selector.len());
        for piece in placeholders::checked_pieces(&selector_def.selector)? {
            match piece {
                Piece::Text { text, .. } => filled.push_str(&text),
                Piece::Field { raw, name, .. } => {
                    let Some(value) = self.symbols.compile_time_constants.get(name) else {
                        return Err(format!(
                            "Selector alias `@{}` uses `{raw}`, but `{name}` is not a const defined above it. \
                             An alias takes only const placeholders; write the value, or define `const {name} = ...` before the alias.",
                            selector_def.name
                        )
                        .into());
                    };
                    if value.fract() == 0.0 {
                        filled.push_str(&format!("{}", *value as i64));
                    } else {
                        filled.push_str(&value.to_string());
                    }
                }
            }
        }
        Ok(filled)
    }
}
