use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_selector_def(
        &mut self,
        selector_def: &SelectorDef,
    ) -> Result<(), String> {
        // Store the selector alias
        // @Player -> @a[type=player,gamemode=survival]
        self.selector_aliases
            .insert(selector_def.name.clone(), selector_def.selector.clone());

        Ok(())
    }
}
