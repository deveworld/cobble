use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_selector_def(
        &mut self,
        selector_def: &SelectorDef,
    ) -> Result<(), String> {
        // Validate selector name doesn't conflict with built-in selectors
        const BUILTIN_SELECTORS: &[&str] = &["a", "p", "r", "e", "s"];
        if BUILTIN_SELECTORS.contains(&selector_def.name.as_str()) {
            return Err(format!(
                "Cannot redefine built-in selector '@{}'. Use a different name.\n\
                Built-in selectors: @a (all players), @p (nearest player), @r (random player), \
                @e (all entities), @s (executing entity)",
                selector_def.name
            ));
        }

        // Validate selector syntax (must start with @)
        if !selector_def.selector.starts_with('@') {
            return Err(format!(
                "Invalid selector syntax: '{}'. Selectors must start with '@'.\n\
                Example: @Player = @a[type=player]",
                selector_def.selector
            ));
        }

        // Check for duplicate definition (warning only)
        if self.selector_aliases.contains_key(&selector_def.name) {
            eprintln!(
                "⚠️  Warning: Selector '@{}' redefined. Previous definition will be overwritten.",
                selector_def.name
            );
        }

        // Store the selector alias
        // @Player -> @a[type=player,gamemode=survival]
        self.selector_aliases
            .insert(selector_def.name.clone(), selector_def.selector.clone());

        Ok(())
    }
}
