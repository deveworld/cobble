use crate::ast::{Expression, Import, Program, Statement};
use crate::transpiler::function_context::called_functions;
use crate::transpiler::{SourceLocation, TranspileError, Transpiler, MAX_IMPORT_DEPTH};
use std::collections::{BTreeSet, HashMap, HashSet};
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
    /// Module variables holding a list, a map, or text, with their file and
    /// kind, declared in a file whose module code is compiled after the
    /// current one, as the file that imports it. Until then a function of
    /// another file would read the name as a score.
    pub(in crate::transpiler) later_variables: HashMap<String, (PathBuf, &'static str)>,
}

impl Default for ImportState {
    fn default() -> Self {
        Self {
            files: HashSet::new(),
            stack: Vec::new(),
            current_file: None,
            current_dir: PathBuf::from("."),
            later_variables: HashMap::new(),
        }
    }
}

/// The module variables of `program` that hold a list, a map, or text,
/// with that kind.
fn storage_module_variables(program: &Program) -> Vec<(&str, &'static str)> {
    crate::transpiler::module_statements(&program.statements)
        .into_iter()
        .filter_map(|statement| match statement {
            Statement::Assignment(assign) if assign.assigns_name() => {
                let kind = match &assign.value {
                    Expression::Array(_) => "list",
                    Expression::Map(_) => "map",
                    Expression::String(_) => "string",
                    _ => return None,
                };
                Some((assign.target.as_str(), kind))
            }
            _ => None,
        })
        .collect()
}

impl Transpiler {
    /// Marks the list, map, and text module variables of `program`, the
    /// current file, as not yet compiled, for [`Self::check_later_variables`].
    pub(in crate::transpiler) fn hold_later_variables(&mut self, program: &Program) {
        let Some(file) = self.imports.current_file.clone() else {
            return;
        };
        for (name, kind) in storage_module_variables(program) {
            self.imports
                .later_variables
                .entry(name.to_string())
                .or_insert_with(|| (file.clone(), kind));
        }
    }

    /// Marks the list, map, and text module variables of the file at `path`
    /// as not yet compiled. A build of several files marks every file
    /// first, so a function of a file compiled earlier can't read them as
    /// scores.
    pub fn record_module_variables(&mut self, path: &Path, program: &Program) {
        self.record_potential_real_globals(program);
        let file = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        for (name, kind) in storage_module_variables(program) {
            self.imports
                .later_variables
                .entry(name.to_string())
                .or_insert_with(|| (file.clone(), kind));
        }
    }

    /// The module code of `program` is compiled: its variables are known.
    pub(in crate::transpiler) fn release_later_variables(&mut self, program: &Program) {
        for (name, _) in storage_module_variables(program) {
            self.imports.later_variables.remove(name);
        }
    }

    /// Refuses a function's read of a list, map, or text module variable
    /// that another file declares and that isn't compiled yet, as one of the
    /// file importing this one: Cobble would read it as a score, where
    /// Python raises `NameError`.
    pub(in crate::transpiler) fn check_later_variables(
        &self,
        statement: &Statement,
    ) -> Result<(), TranspileError> {
        if self.imports.later_variables.is_empty() || self.current_function.is_none() {
            return Ok(());
        }
        let mut names: Vec<&str> = Vec::new();
        if let Statement::Global(globals) = statement {
            names.extend(globals.iter().map(String::as_str));
        }
        fn reads<'a>(expr: &'a Expression, names: &mut Vec<&'a str>) {
            match expr {
                Expression::Identifier(name) => names.push(name),
                // `f(x)` names a function, not a variable.
                Expression::Call(func, args) => {
                    if !matches!(&**func, Expression::Identifier(_)) {
                        reads(func, names);
                    }
                    for arg in args {
                        reads(arg, names);
                    }
                }
                _ => {
                    for part in crate::transpiler::desugar::sub_expressions(expr) {
                        reads(part, names);
                    }
                }
            }
        }
        for expr in crate::transpiler::statement_expressions(statement) {
            reads(expr, &mut names);
        }
        for name in names {
            let Some((file, kind)) = self.imports.later_variables.get(name) else {
                continue;
            };
            if self.imports.current_file.as_ref() == Some(file)
                || self.current_context.is_param(name)
                || self.current_context.is_local(name)
            {
                continue;
            }
            let file_name = |path: &Path| {
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |name| name.to_string_lossy().into_owned(),
                )
            };
            let this_file = self
                .imports
                .current_file
                .as_deref()
                .map(file_name)
                .unwrap_or_default();
            return Err(format!(
                "`{name}` is a {kind} declared in {}, which this file doesn't import, so this function would read it as a number (Python raises NameError). Pass it to the function as a parameter, or declare it in {this_file} or a file it imports.",
                file_name(file),
            )
            .into());
        }
        Ok(())
    }
}

impl Transpiler {
    /// Processes the imports of the current file. An error without a
    /// location points at the import that failed.
    pub(in crate::transpiler) fn process_imports(
        &mut self,
        program: &Program,
    ) -> Result<(), TranspileError> {
        for (index, import) in program.imports.iter().enumerate() {
            let location = self.import_source(index);
            self.process_import(import)
                .map_err(|error| error.with_location_if_missing(location))?;
        }
        Ok(())
    }

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
        self.record_container_writes(&program);
        self.hold_later_variables(&program);
        self.process_imports(&program)?;
        self.register_program_function_metadata(&program);
        if import.items.is_empty() {
            self.register_qualified_function_aliases(&import.module, &program);
        }
        for statement in &program.statements {
            self.process_top_level_statement(statement)?;
        }
        self.release_later_variables(&program);

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
        let source = crate::diagnostics::read_source_file(import_path).map_err(|e| {
            format!(
                "Failed to read import file '{}': {}",
                import_path.display(),
                e
            )
        })?;
        let located = crate::parser::parse_located_with_diagnostics(&source).map_err(|errors| {
            let location = errors.first().map(|error| SourceLocation {
                file: import_path
                    .canonicalize()
                    .unwrap_or_else(|_| import_path.to_path_buf()),
                line: error.span.line,
                column: error.span.column,
            });
            TranspileError::from(format!(
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
            ))
            .with_location_if_missing(location)
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

    /// Marks the functions of `program` that can call themselves. Modules
    /// can't import each other in a cycle, so every call cycle stays inside
    /// one file.
    fn register_recursive_functions(&mut self, program: &Program) {
        let calls: HashMap<&str, BTreeSet<String>> = program
            .statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::FunctionDef(func) => {
                    Some((func.name.as_str(), called_functions(&func.body)))
                }
                _ => None,
            })
            .collect();
        // A function can call itself when it calls itself directly or is in
        // a cycle of calls: a strongly connected component of more than
        // one function. Tarjan's algorithm finds them in linear time, where
        // a search from each function took quadratic time on a long chain.
        let names: Vec<&str> = calls.keys().copied().collect();
        let index_of: HashMap<&str, usize> = names
            .iter()
            .enumerate()
            .map(|(index, name)| (*name, index))
            .collect();
        let edges: Vec<Vec<usize>> = names
            .iter()
            .map(|name| {
                calls[name]
                    .iter()
                    .filter_map(|callee| index_of.get(callee.as_str()).copied())
                    .collect()
            })
            .collect();
        for (node, callees) in edges.iter().enumerate() {
            if callees.contains(&node) {
                self.functions.recursive.insert(names[node].to_string());
            }
        }
        const UNSEEN: usize = usize::MAX;
        let mut order = vec![UNSEEN; names.len()];
        let mut low = vec![0; names.len()];
        let mut on_stack = vec![false; names.len()];
        let mut stack: Vec<usize> = Vec::new();
        let mut next_order = 0;
        for root in 0..names.len() {
            if order[root] != UNSEEN {
                continue;
            }
            // (node, index of the next callee to visit)
            let mut work: Vec<(usize, usize)> = vec![(root, 0)];
            order[root] = next_order;
            low[root] = next_order;
            next_order += 1;
            stack.push(root);
            on_stack[root] = true;
            while let Some(&mut (node, ref mut next)) = work.last_mut() {
                if let Some(&callee) = edges[node].get(*next) {
                    *next += 1;
                    if order[callee] == UNSEEN {
                        order[callee] = next_order;
                        low[callee] = next_order;
                        next_order += 1;
                        stack.push(callee);
                        on_stack[callee] = true;
                        work.push((callee, 0));
                    } else if on_stack[callee] {
                        low[node] = low[node].min(order[callee]);
                    }
                    continue;
                }
                work.pop();
                if let Some(&(parent, _)) = work.last() {
                    low[parent] = low[parent].min(low[node]);
                }
                if low[node] == order[node] {
                    let mut component = Vec::new();
                    while let Some(member) = stack.pop() {
                        on_stack[member] = false;
                        component.push(member);
                        if member == node {
                            break;
                        }
                    }
                    if component.len() > 1 {
                        for member in component {
                            self.functions.recursive.insert(names[member].to_string());
                        }
                    }
                }
            }
        }
    }

    pub(in crate::transpiler) fn register_program_function_metadata(&mut self, program: &Program) {
        self.register_recursive_functions(program);
        let loop_variables = crate::transpiler::module_loop_variables(&program.statements);
        for name in &loop_variables {
            self.symbols.module_variables.insert(name.clone());
        }
        self.module_loop_variables.extend(loop_variables);
        self.register_module_declarations(program);
        let mut passes = Vec::new();
        for statement in &program.statements {
            if let Statement::Assignment(assign) = statement {
                if assign.assigns_name() {
                    self.symbols.module_variables.insert(assign.target.clone());
                }
            }
            if let Statement::FunctionDef(func) = statement {
                let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
                let mut body = Vec::new();
                crate::transpiler::function_context::collect_statements(&func.body, &mut body);
                let safe_text = body.iter().any(|statement| matches!(statement,
                    Statement::MinecraftCommand(command) if crate::transpiler::command_processor::CommandProcessor::has_plain_parameter_message(command, &param_names)));
                if safe_text && !self.functions.safe_text_variants.contains_key(&func.name) {
                    let variant = format!("_cobble_text_entry_{}", self.next_temp_id());
                    self.functions
                        .safe_text_variants
                        .insert(func.name.clone(), variant);
                }
                self.functions.params.insert(func.name.clone(), param_names);
                if !self.in_prelude
                    && Self::analyze_function_return_kind(&func.body)
                        != super::super::types::FunctionReturnKind::None
                {
                    self.functions.numeric_returns.insert(func.name.clone());
                }
                let (used, passed) =
                    crate::transpiler::function_context::params_read_or_passed(func);
                self.functions
                    .params_used_as_values
                    .insert(func.name.clone(), used);
                self.functions.params_in_integer_commands.insert(
                    func.name.clone(),
                    crate::transpiler::function_context::params_in_integer_commands(func),
                );
                passes.extend(
                    passed
                        .into_iter()
                        .map(|(param, callee, index)| (func.name.clone(), param, callee, index)),
                );
                self.functions.returns.insert(
                    func.name.clone(),
                    Self::analyze_function_return_kind(&func.body),
                );
            }
        }
        // A parameter passed on whole to another function's parameter is
        // used as a value when that one is, or when the callee isn't a
        // function of the pack.
        loop {
            let mut changed = false;
            for (function, param, callee, index) in &passes {
                let integer = self
                    .functions
                    .params
                    .get(callee)
                    .and_then(|params| params.get(*index))
                    .is_some_and(|callee_param| {
                        self.functions
                            .params_in_integer_commands
                            .get(callee)
                            .is_some_and(|used| used.contains(callee_param))
                    });
                if integer
                    && self
                        .functions
                        .params_in_integer_commands
                        .get_mut(function)
                        .is_some_and(|set| set.insert(param.clone()))
                {
                    changed = true;
                }
                let used = match self.functions.params.get(callee) {
                    Some(callee_params) => callee_params.get(*index).is_none_or(|callee_param| {
                        self.functions
                            .params_used_as_values
                            .get(callee)
                            .is_none_or(|used| used.contains(callee_param))
                    }),
                    None => true,
                };
                if used
                    && self
                        .functions
                        .params_used_as_values
                        .get_mut(function)
                        .is_some_and(|set| set.insert(param.clone()))
                {
                    changed = true;
                }
            }
            if !changed {
                break;
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
