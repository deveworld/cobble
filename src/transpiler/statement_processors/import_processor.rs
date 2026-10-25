use crate::ast::{Expression, Import, Program, Statement};
use crate::transpiler::{TranspileError, Transpiler, MAX_IMPORT_DEPTH};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Files being compiled and the chain of imports that led to them.
pub(in crate::transpiler) struct ImportState {
    /// Files already imported, to compile each one once.
    pub(in crate::transpiler) files: HashSet<PathBuf>,
    /// The current import chain, to report circular imports.
    pub(in crate::transpiler) stack: Vec<PathBuf>,
    pub(in crate::transpiler) current_file: Option<PathBuf>,
    /// Folder of the current file, for resolving relative imports.
    pub(in crate::transpiler) current_dir: PathBuf,
}

impl Default for ImportState {
    fn default() -> Self {
        Self {
            files: HashSet::new(),
            stack: Vec::new(),
            current_file: None,
            current_dir: PathBuf::from("."),
        }
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn process_import(
        &mut self,
        import: &Import,
    ) -> Result<(), TranspileError> {
        // Handle stdlib imports
        if import.module == "stdlib" {
            if import.items.is_empty() {
                // `import stdlib` activates every module (0.7.x compatibility).
                self.activate_all_stdlib_modules();
            } else {
                // `from stdlib import text, score, ...` activates listed modules.
                for item in &import.items {
                    self.activate_stdlib_module(item)?;
                }
            }
            return Ok(());
        }

        let (import_path, canonical_path) = self.resolve_import_path(&import.module)?;

        // A later `import module` may follow an earlier `from module import ...`.
        // Re-read just enough metadata to enable qualified `module.func()` calls
        // without processing the imported statements twice.
        if self.imports.files.contains(&canonical_path) {
            if import.items.is_empty() {
                let (_, located) = Self::read_import(&import_path, &import.module)?;
                self.register_qualified_function_aliases(&import.module, &located.program);
            }
            return Ok(());
        }

        self.imports.stack.push(canonical_path.clone());
        self.imports.files.insert(canonical_path.clone());
        let (source, located) = Self::read_import(&import_path, &import.module)?;
        let crate::parser::LocatedProgram { program, spans } = located;

        // Nested imports resolve against the imported file's folder.
        let saved_dir = self.imports.current_dir.clone();
        let saved_file = self.imports.current_file.clone();
        if let Some(parent) = import_path.parent() {
            self.imports.current_dir = parent.to_path_buf();
        }
        self.imports.current_file = Some(canonical_path);
        self.register_source_locations_from_spans(&import_path, &source, &spans);

        // Nested imports first, then the imported statements.
        for import in &program.imports {
            self.process_import(import)?;
        }
        self.register_program_function_metadata(&program);
        if import.items.is_empty() {
            self.register_qualified_function_aliases(&import.module, &program);
        }
        for statement in &program.statements {
            self.process_statement(statement)?;
        }

        self.imports.current_dir = saved_dir;
        self.imports.current_file = saved_file;
        self.imports.stack.pop();
        Ok(())
    }

    /// The path of the module file `module` imported from the current file,
    /// and its canonical form. Rejects names that would leave the folder,
    /// import chains that are too deep or circular, and missing files.
    fn resolve_import_path(&self, module: &str) -> Result<(PathBuf, PathBuf), TranspileError> {
        if module.contains("..") || module.contains('/') || module.contains('\\') {
            return Err(format!(
                "Invalid module name '{}': Cannot contain path separators or '..'\n\
                Module names must be simple identifiers (e.g., 'utils', 'helpers')",
                module
            )
            .into());
        }

        if self.imports.stack.len() >= MAX_IMPORT_DEPTH {
            return Err(format!(
                "Maximum import depth exceeded ({}).\n\
                This usually indicates a circular or excessively deep import chain.",
                MAX_IMPORT_DEPTH
            )
            .into());
        }

        let import_path = self.imports.current_dir.join(format!("{}.cbl", module));
        let canonical_path = import_path.canonicalize().unwrap_or(import_path.clone());

        if self.imports.stack.contains(&canonical_path) {
            return Err(format!(
                "Circular import detected: {}\n\
                Import cycles are not supported because they make initialization order ambiguous.",
                self.format_import_chain(&canonical_path)
            )
            .into());
        }

        if !canonical_path.exists() && !import_path.exists() {
            return Err(format!(
                "Cannot import '{}' from '{}': file '{}' not found",
                module,
                self.current_importer_display(),
                import_path.display()
            )
            .into());
        }
        Ok((import_path, canonical_path))
    }

    fn read_import(
        import_path: &Path,
        module: &str,
    ) -> Result<(String, crate::parser::LocatedProgram), TranspileError> {
        let source = std::fs::read_to_string(import_path).map_err(|e| {
            format!(
                "Failed to read import file '{}': {}",
                import_path.display(),
                e
            )
        })?;
        let located = crate::parser::parse_located_with_diagnostics(&source).map_err(|errors| {
            format!(
                "Parse failed for '{}': {}",
                module,
                errors
                    .iter()
                    .map(|error| format!(
                        "{}:{} {}",
                        error.span.line, error.span.column, error.message
                    ))
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        Ok((source, located))
    }

    pub(in crate::transpiler) fn current_importer_display(&self) -> String {
        self.imports
            .stack
            .last()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.imports.current_dir.display().to_string())
    }

    pub(in crate::transpiler) fn format_import_chain(&self, next_path: &Path) -> String {
        let mut chain: Vec<String> = self
            .imports
            .stack
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        chain.push(next_path.display().to_string());
        chain.join(" -> ")
    }

    pub(in crate::transpiler) fn register_program_function_metadata(&mut self, program: &Program) {
        for statement in &program.statements {
            if let Statement::FunctionDef(func) = statement {
                let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
                self.functions.params.insert(func.name.clone(), param_names);
                self.functions.returns.insert(
                    func.name.clone(),
                    Self::analyze_function_return_kind(&func.body),
                );
            }
        }
    }

    pub(in crate::transpiler) fn register_qualified_function_aliases(
        &mut self,
        module: &str,
        program: &Program,
    ) {
        for statement in &program.statements {
            if let Statement::FunctionDef(func) = statement {
                self.functions
                    .qualified_aliases
                    .insert(format!("{}.{}", module, func.name), func.name.clone());
            }
        }
    }

    pub(in crate::transpiler) fn resolve_user_function_call_name<'a>(
        &'a self,
        func: &'a Expression,
    ) -> Option<&'a str> {
        match func {
            Expression::Identifier(name) if self.functions.params.contains_key(name) => {
                Some(name.as_str())
            }
            Expression::Attribute(obj, method) => {
                let module_name = Self::expression_path(obj)?;
                let key = format!("{}.{}", module_name, method);
                self.functions
                    .qualified_aliases
                    .get(&key)
                    .map(String::as_str)
            }
            _ => None,
        }
    }
}
