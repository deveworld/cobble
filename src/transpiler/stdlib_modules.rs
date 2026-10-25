use crate::transpiler::{TranspileError, TranspileErrorKind, Transpiler, ALL_STDLIB_MODULES};
use std::collections::HashSet;

pub(in crate::transpiler) struct StdlibState {
    /// Stdlib version declared in `cobble.toml`. `1` keeps 0.7.x behavior
    /// (all modules always active). `2` (default) requires per-module opt-in
    /// via `from stdlib import ...`.
    pub(in crate::transpiler) version: u8,
    /// Stdlib modules activated by imports. When `version == 1` or
    /// `import stdlib` is used, every module in `ALL_STDLIB_MODULES` is
    /// inserted here.
    pub(in crate::transpiler) active_modules: HashSet<String>,
    /// Whether the `[stdlib] version = 1` deprecation warning was already
    /// printed for this build. It is printed once per build.
    pub(in crate::transpiler) v1_warning_emitted: bool,
}

impl Default for StdlibState {
    fn default() -> Self {
        Self {
            version: 2,
            active_modules: HashSet::new(),
            v1_warning_emitted: false,
        }
    }
}

impl Transpiler {
    /// Set the stdlib version from `cobble.toml` (`[stdlib] version`).
    ///
    /// `version = 1` keeps 0.7.x behavior where all modules are always
    /// active. `version = 2` (default) requires per-module opt-in.
    pub(in crate::transpiler) fn set_stdlib_version(&mut self, version: u8) {
        self.stdlib.version = version;
        if version == 1 {
            self.stdlib.active_modules.clear();
            for module in ALL_STDLIB_MODULES {
                self.stdlib.active_modules.insert((*module).to_string());
            }
        }
        self.data_pack.stdlib_version = Some(version);
    }

    /// Returns the active stdlib version.
    pub fn stdlib_version(&self) -> u8 {
        self.stdlib.version
    }

    /// Returns the set of modules activated by stdlib imports.
    pub fn active_stdlib_modules(&self) -> &HashSet<String> {
        &self.stdlib.active_modules
    }

    /// Sync the active stdlib module set into the data pack so it is
    /// recorded in the build manifest. Called after imports are processed
    /// and before the data pack is written.
    pub fn sync_stdlib_modules_to_manifest(&mut self) {
        let mut modules: Vec<String> = self.stdlib.active_modules.iter().cloned().collect();
        modules.sort();
        self.data_pack.active_stdlib_modules = modules;
    }

    /// Activate every stdlib module. Used by `import stdlib` and by
    /// `[stdlib] version = 1`.
    pub(in crate::transpiler) fn activate_all_stdlib_modules(&mut self) {
        for module in ALL_STDLIB_MODULES {
            self.stdlib.active_modules.insert((*module).to_string());
        }
    }

    /// Activate a single stdlib module by name. Returns an error if the
    /// module name is not recognized.
    pub(in crate::transpiler) fn activate_stdlib_module(
        &mut self,
        name: &str,
    ) -> Result<(), TranspileError> {
        if !ALL_STDLIB_MODULES.contains(&name) {
            return Err(TranspileError::new(
                TranspileErrorKind::UnknownStdlibModule,
                format!(
                    "Unknown stdlib module '{}'. Available modules: {}",
                    name,
                    ALL_STDLIB_MODULES.join(", ")
                ),
            ));
        }
        self.stdlib.active_modules.insert(name.to_string());
        Ok(())
    }

    /// Check that a stdlib module is active before calling one of its
    /// helpers. Returns an error in `version = 2` when the module was not
    /// imported. In `version = 1` every module is always active so this
    /// always succeeds.
    pub(in crate::transpiler) fn require_stdlib_module(
        &self,
        module: &str,
    ) -> Result<(), TranspileError> {
        if self.stdlib.active_modules.contains(module) {
            Ok(())
        } else {
            Err(TranspileError::new(
                TranspileErrorKind::MissingStdlibModule,
                format!(
                    "module '{module}' not imported.\n\
                 Add `from stdlib import {module}` or use `import stdlib` to enable all modules.\n\
                 See https://deveworld.github.io/cobble/docs/stdlib/modules.html for the import and versioning model."
                ),
            ))
        }
    }

    /// Emit the `[stdlib] version = 1` deprecation warning once per build.
    pub(in crate::transpiler) fn maybe_emit_stdlib_v1_warning(&mut self) {
        if self.stdlib.version == 1 && !self.stdlib.v1_warning_emitted {
            self.stdlib.v1_warning_emitted = true;
            eprintln!(
                "warning: [stdlib] version = 1 is deprecated.\n\
                 Migrate to version = 2 and use `from stdlib import ...` for per-module opt-in.\n\
                 See https://deveworld.github.io/cobble/docs/stdlib/modules.html for how to migrate."
            );
        }
    }
}
