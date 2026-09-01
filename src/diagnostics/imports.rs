use crate::ast::{Import, Program, SourceSpan, Statement};
use crate::parser::{LocatedProgram, ProgramSpanTable};

use super::is_simple_module_name;
use super::reporting::{FileSourceDiagnostics, ParsedSourceFile, SourceDiagnostic};
use super::semantic::{self, SemanticContext};
use super::text::{mask_non_code, source_position_at};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// Keep diagnostics import traversal aligned with the transpiler guard so
// preflight validation cannot recurse deeply enough to exhaust the stack.
pub(super) const MAX_IMPORT_DEPTH: usize = crate::transpiler::MAX_IMPORT_DEPTH;

pub fn analyze_in_memory_imports(source: &str, located: &LocatedProgram) -> Vec<SourceDiagnostic> {
    let mut diagnostics = Vec::new();

    let imports = &located.program.imports;
    let import_spans = &located.spans.imports;
    // The span table's import spans are built 1:1, in order, with the
    // parsed program's imports.
    debug_assert_eq!(imports.len(), import_spans.len());

    for (import_index, import) in imports.iter().enumerate() {
        if import.module == "stdlib" {
            continue;
        }

        let import_span = import_spans.get(import_index).copied();
        let (line, column) = import_site_position(source, import_span, import);
        diagnostics.push(
            SourceDiagnostic::error(
                "missing-import",
                line,
                column,
                format!("Cannot import '{}': no import file is available", import.module),
            )
            .with_help(
                "The browser compiler accepts one in-memory Cobble file. Use `stdlib` imports or compile multi-file projects with the CLI.",
            ),
        );
    }

    diagnostics
}

pub fn parse_source_file(path: &Path) -> Result<ParsedSourceFile, Vec<FileSourceDiagnostics>> {
    let parsed_files = parse_source_file_tree(path)?;
    let canonical_path = canonical_or_original(path);
    parsed_files
        .into_iter()
        .find(|file| canonical_or_original(&file.path) == canonical_path)
        .ok_or_else(|| {
            vec![FileSourceDiagnostics::new(
                path,
                "",
                vec![SourceDiagnostic::error(
                    "source-read",
                    1,
                    1,
                    "Parsed source file was not returned by the import tree",
                )],
            )]
        })
}

pub fn parse_source_files(
    paths: &[PathBuf],
) -> Result<Vec<ParsedSourceFile>, Vec<FileSourceDiagnostics>> {
    let mut roots = Vec::new();
    let mut all_files = Vec::new();
    let mut seen_files = HashSet::new();
    let mut seen_diagnostic_files = HashSet::new();
    let mut diagnostics = Vec::new();

    for path in paths {
        match parse_source_file_tree(path) {
            Ok(parsed_tree) => {
                let canonical_root = canonical_or_original(path);
                if let Some(root) = parsed_tree
                    .iter()
                    .find(|file| canonical_or_original(&file.path) == canonical_root)
                {
                    roots.push(root.clone());
                }

                for parsed_file in parsed_tree {
                    let canonical_path = canonical_or_original(&parsed_file.path);
                    if seen_files.insert(canonical_path) {
                        all_files.push(parsed_file);
                    }
                }
            }
            Err(file_diagnostics) => {
                // A file reachable both as its own entry point and through
                // another entry point's import tree is analyzed twice; report
                // each file's findings only once.
                for file_diagnostic in file_diagnostics {
                    if seen_diagnostic_files.insert(canonical_or_original(&file_diagnostic.path)) {
                        diagnostics.push(file_diagnostic);
                    }
                }
            }
        }
    }

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let cross_file_diagnostics = analyze_cross_file_functions(&all_files);
    if !cross_file_diagnostics.is_empty() {
        return Err(cross_file_diagnostics);
    }

    Ok(roots)
}

fn parse_source_file_tree(
    path: &Path,
) -> Result<Vec<ParsedSourceFile>, Vec<FileSourceDiagnostics>> {
    let source = fs::read_to_string(path).map_err(|error| {
        vec![FileSourceDiagnostics::new(
            path,
            "",
            vec![SourceDiagnostic::error(
                "source-read",
                1,
                1,
                format!("Failed to read source file: {error}"),
            )],
        )]
    })?;

    let located = super::parse_source_located(&source)
        .map_err(|diagnostics| vec![FileSourceDiagnostics::new(path, &source, diagnostics)])?;
    let program = located.program.clone();

    let canonical_path = canonical_or_original(path);
    let mut visited = HashSet::from([canonical_path.clone()]);
    let mut stack = vec![canonical_path];
    let mut diagnostics = Vec::new();
    let mut imported_files = Vec::new();
    analyze_import_tree(
        path,
        &source,
        &program,
        &located.spans,
        &mut visited,
        &mut stack,
        &mut diagnostics,
        &mut imported_files,
    );

    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    let mut parsed_files = imported_files;
    parsed_files.push(ParsedSourceFile {
        path: path.to_path_buf(),
        source: source.clone(),
        program,
        spans: located.spans,
    });

    let cross_file_diagnostics = analyze_cross_file_functions(&parsed_files);
    if !cross_file_diagnostics.is_empty() {
        return Err(cross_file_diagnostics);
    }

    Ok(parsed_files)
}

#[allow(clippy::too_many_arguments)]
fn analyze_import_tree(
    current_path: &Path,
    current_source: &str,
    current_program: &Program,
    current_spans: &ProgramSpanTable,
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    output: &mut Vec<FileSourceDiagnostics>,
    parsed_imports: &mut Vec<ParsedSourceFile>,
) {
    let current_dir = current_path.parent().unwrap_or_else(|| Path::new("."));
    let imports = &current_program.imports;
    let import_spans = &current_spans.imports;
    // The span table's import spans are built 1:1, in order, with the
    // parsed program's imports.
    debug_assert_eq!(imports.len(), import_spans.len());

    for (import_index, import) in imports.iter().enumerate() {
        if import.module == "stdlib" {
            continue;
        }

        let import_span = import_spans.get(import_index).copied();
        let (line, column) = import_site_position(current_source, import_span, import);

        if !is_simple_module_name(&import.module) {
            output.push(FileSourceDiagnostics::new(
                current_path,
                current_source,
                vec![SourceDiagnostic::error(
                    "unsupported-import",
                    line,
                    column,
                    format!("Invalid module name `{}`", import.module),
                )
                .with_help(
                    "Module names must be simple identifiers such as `helpers` or `utils`.",
                )],
            ));
            continue;
        }

        let import_path = current_dir.join(format!("{}.cbl", import.module));
        let canonical_path = canonical_or_original(&import_path);

        if stack.contains(&canonical_path) {
            output.push(FileSourceDiagnostics::new(
                current_path,
                current_source,
                vec![
                    SourceDiagnostic::error(
                        "circular-import",
                        line,
                        column,
                        format!("Circular import detected while importing `{}`", import.module),
                    )
                    .with_help(format!(
                        "Import chain: {}. Import cycles are not supported because they make initialization order ambiguous.",
                        format_import_chain(stack, &canonical_path)
                    )),
                ],
            ));
            continue;
        }

        if visited.contains(&canonical_path) {
            if let Some(diagnostics) = validate_import_items_from_path(
                current_path,
                current_source,
                current_program,
                current_spans,
                import,
                import_span,
                &import_path,
            ) {
                output.push(diagnostics);
            }
            continue;
        }

        if stack.len() >= MAX_IMPORT_DEPTH {
            output.push(FileSourceDiagnostics::new(
                current_path,
                current_source,
                vec![SourceDiagnostic::error(
                    "maximum-import-depth",
                    line,
                    column,
                    format!("Maximum import depth exceeded ({MAX_IMPORT_DEPTH})"),
                )
                .with_help(format!(
                    "Import chain: {}. This usually indicates a circular or excessively deep import chain.",
                    format_import_chain(stack, &canonical_path)
                ))],
            ));
            continue;
        }

        if !canonical_path.exists() && !import_path.exists() {
            output.push(FileSourceDiagnostics::new(
                current_path,
                current_source,
                vec![SourceDiagnostic::error(
                    "missing-import",
                    line,
                    column,
                    format!(
                        "Cannot import '{}': file '{}' was not found",
                        import.module,
                        import_path.display()
                    ),
                )
                .with_help(format!(
                    "Importing file: {}. Create the missing `.cbl` file or remove the import.",
                    current_path.display()
                ))],
            ));
            continue;
        }

        let imported_source = match fs::read_to_string(&import_path) {
            Ok(source) => source,
            Err(error) => {
                output.push(FileSourceDiagnostics::new(
                    current_path,
                    current_source,
                    vec![SourceDiagnostic::error(
                        "import-read",
                        line,
                        column,
                        format!(
                            "Failed to read imported module `{}` from `{}`: {error}",
                            import.module,
                            import_path.display()
                        ),
                    )
                    .with_help(format!("Importing file: {}", current_path.display()))],
                ));
                continue;
            }
        };

        let imported_located = match super::parse_source_located(&imported_source) {
            Ok(located) => located,
            Err(mut diagnostics) => {
                add_import_chain_help(&mut diagnostics, stack, &canonical_path);
                output.push(FileSourceDiagnostics::new(
                    &import_path,
                    imported_source,
                    diagnostics,
                ));
                continue;
            }
        };
        let imported_program = imported_located.program.clone();

        if let Some(diagnostics) = validate_import_items(
            current_path,
            current_source,
            current_program,
            current_spans,
            import,
            import_span,
            &imported_located.program,
        ) {
            output.push(diagnostics);
            continue;
        }

        visited.insert(canonical_path.clone());
        stack.push(canonical_path.clone());
        analyze_import_tree(
            &import_path,
            &imported_source,
            &imported_program,
            &imported_located.spans,
            visited,
            stack,
            output,
            parsed_imports,
        );
        stack.pop();

        parsed_imports.push(ParsedSourceFile {
            path: import_path,
            source: imported_source,
            program: imported_program,
            spans: imported_located.spans,
        });
    }
}

fn analyze_cross_file_functions(files: &[ParsedSourceFile]) -> Vec<FileSourceDiagnostics> {
    // One AST/span context per file; every cross-file pass below walks the
    // parsed programs through these instead of re-scanning source lines.
    let contexts = files
        .iter()
        .map(|file| SemanticContext::new(&file.source, &file.program, &file.spans))
        .collect::<Vec<_>>();

    let mut signatures = HashMap::new();
    let mut duplicate_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();

    for (file_index, (file, context)) in files.iter().zip(&contexts).enumerate() {
        for signature in semantic::collect_function_definitions(context) {
            if let Some(first) = signatures.get(&signature.name) {
                duplicate_diagnostics.entry(file_index).or_default().push(
                    SourceDiagnostic::error(
                        "duplicate-function",
                        signature.line,
                        signature.column,
                        format!(
                            "Duplicate function definition `{}` across imported files",
                            signature.name
                        ),
                    )
                    .with_help(format!(
                        "First definition is at {}. Rename one function or split the generated function names.",
                        format_file_signature_location(first)
                    )),
                );
            } else {
                signatures.insert(
                    signature.name.clone(),
                    FileFunctionSignature {
                        path: file.path.clone(),
                        name: signature.name,
                        params: signature.params,
                        line: signature.line,
                        column: signature.column,
                    },
                );
            }
        }
    }

    if !duplicate_diagnostics.is_empty() {
        return file_diagnostics_from_map(files, duplicate_diagnostics);
    }

    let duplicate_symbol_diagnostics = analyze_cross_file_named_symbols(files, &contexts);
    if !duplicate_symbol_diagnostics.is_empty() {
        return duplicate_symbol_diagnostics;
    }

    let mut call_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();
    for (file_index, (file, context)) in files.iter().zip(&contexts).enumerate() {
        let call_sites = semantic::collect_call_sites(context);
        collect_cross_file_call_diagnostics(
            file_index,
            file,
            &call_sites,
            &signatures,
            &mut call_diagnostics,
        );
    }

    file_diagnostics_from_map(files, call_diagnostics)
}

fn analyze_cross_file_named_symbols(
    files: &[ParsedSourceFile],
    contexts: &[SemanticContext<'_>],
) -> Vec<FileSourceDiagnostics> {
    let mut symbols = HashMap::new();
    let mut duplicate_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();

    for (file_index, (file, context)) in files.iter().zip(contexts).enumerate() {
        for definition in semantic::collect_named_symbol_defs(context) {
            let symbol = FileNamedSymbol {
                path: file.path.clone(),
                name: definition.name,
                kind: if definition.is_entity_template {
                    FileSymbolKind::EntityTemplate
                } else {
                    FileSymbolKind::SelectorAlias
                },
                line: definition.line,
                column: definition.column,
            };
            let key = (symbol.kind, symbol.name.clone());
            if let Some(first) = symbols.get(&key) {
                duplicate_diagnostics.entry(file_index).or_default().push(
                    SourceDiagnostic::error(
                        "duplicate-symbol",
                        symbol.line,
                        symbol.column,
                        format!(
                            "Duplicate {} `{}` across imported files",
                            symbol.kind.label(),
                            symbol.display_name()
                        ),
                    )
                    .with_help(format!(
                        "First definition is at {}. Rename one definition so imports do not overwrite each other.",
                        format_file_symbol_location(first)
                    )),
                );
            } else {
                symbols.insert(key, symbol);
            }
        }
    }

    file_diagnostics_from_map(files, duplicate_diagnostics)
}

#[allow(clippy::too_many_arguments)]
fn validate_import_items_from_path(
    current_path: &Path,
    current_source: &str,
    current_program: &Program,
    current_spans: &ProgramSpanTable,
    import: &Import,
    import_span: Option<SourceSpan>,
    import_path: &Path,
) -> Option<FileSourceDiagnostics> {
    if import.items.is_empty() || import.module == "stdlib" {
        return None;
    }

    // A module that cannot be read (or re-parsed) here is silently skipped
    // (`.ok()?`), preserving this re-validation pass's long-standing
    // behavior: the module was read and parsed successfully when it was
    // first visited in this run, so a failure here is a mid-run race.
    let imported_source = fs::read_to_string(import_path).ok()?;
    let imported_program = crate::parser::parse(&imported_source).ok()?;
    validate_import_items(
        current_path,
        current_source,
        current_program,
        current_spans,
        import,
        import_span,
        &imported_program,
    )
}

#[allow(clippy::too_many_arguments)]
fn validate_import_items(
    current_path: &Path,
    current_source: &str,
    current_program: &Program,
    current_spans: &ProgramSpanTable,
    import: &Import,
    import_span: Option<SourceSpan>,
    imported_program: &Program,
) -> Option<FileSourceDiagnostics> {
    if import.items.is_empty() || import.module == "stdlib" {
        return None;
    }

    // The importing file's own AST/span context locates raw command
    // placeholders that reference non-value imported symbols.
    let current_context = SemanticContext::new(current_source, current_program, current_spans);

    let exported_symbol_kinds = collect_exported_symbol_kinds(imported_program);
    let exported_symbols = exported_symbol_kinds
        .keys()
        .cloned()
        .collect::<HashSet<_>>();
    let mut diagnostics = Vec::new();
    for item in &import.items {
        match exported_symbol_kinds.get(item) {
            Some(kind) => {
                if kind.is_command_placeholder_value() {
                    continue;
                }

                for (line, column) in
                    semantic::raw_command_placeholder_locations(&current_context, item)
                {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-placeholder-symbol",
                            line,
                            column,
                            format!(
                                "Imported {} `{}` cannot be used as a command placeholder",
                                kind.label(),
                                item
                            ),
                        )
                        .with_help(
                            "Command placeholders such as `{score}` can only reference function parameters, loop variables, or value symbols backed by assignments, consts, or globals.",
                        ),
                    );
                }
            }
            None => {
                let (line, column) = import_item_position(current_source, import_span, item)
                    .unwrap_or_else(|| import_site_position(current_source, import_span, import));

                diagnostics.push(
                    SourceDiagnostic::error(
                        "missing-import-item",
                        line,
                        column,
                        format!(
                            "Cannot import `{}` from `{}`: symbol was not found",
                            item, import.module
                        ),
                    )
                    .with_help(format_missing_import_item_help(
                        &import.module,
                        &exported_symbols,
                    )),
                );
            }
        }
    }

    if diagnostics.is_empty() {
        None
    } else {
        Some(FileSourceDiagnostics::new(
            current_path,
            current_source,
            diagnostics,
        ))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ExportedSymbolKind {
    Value,
    Function,
    SelectorAlias,
    EntityTemplate,
}

impl ExportedSymbolKind {
    fn is_command_placeholder_value(self) -> bool {
        matches!(self, Self::Value)
    }

    fn label(self) -> &'static str {
        match self {
            Self::Value => "value",
            Self::Function => "function",
            Self::SelectorAlias => "selector alias",
            Self::EntityTemplate => "entity template",
        }
    }
}

/// Importable symbols a module defines, keyed by name, collected from its
/// parsed AST (nested statements included — the old line scan had no indent
/// filter either). A later definition of the same name overrides the
/// earlier kind, mirroring the old scan's insert order.
fn collect_exported_symbol_kinds(program: &Program) -> HashMap<String, ExportedSymbolKind> {
    let mut symbol_kinds = HashMap::new();

    semantic::for_each_statement(program, &mut |statement| match statement {
        Statement::FunctionDef(function_def) => {
            symbol_kinds.insert(function_def.name.clone(), ExportedSymbolKind::Function);
        }
        Statement::SelectorDef(selector_def) if is_simple_module_name(&selector_def.name) => {
            symbol_kinds.insert(selector_def.name.clone(), ExportedSymbolKind::SelectorAlias);
        }
        Statement::EntityDef(entity_def) if is_simple_module_name(&entity_def.name) => {
            symbol_kinds.insert(entity_def.name.clone(), ExportedSymbolKind::EntityTemplate);
        }
        // `target` is the raw target text (`x`, `x[0]`, `x.y`); the simple-
        // name filter keeps plain names only, like the old line scan.
        Statement::Assignment(assignment) if is_simple_module_name(&assignment.target) => {
            symbol_kinds.insert(assignment.target.clone(), ExportedSymbolKind::Value);
        }
        Statement::ConstAssignment(assignment) if is_simple_module_name(&assignment.target) => {
            symbol_kinds.insert(assignment.target.clone(), ExportedSymbolKind::Value);
        }
        Statement::Global(names) => {
            for name in names {
                if is_simple_module_name(name) {
                    symbol_kinds.insert(name.clone(), ExportedSymbolKind::Value);
                }
            }
        }
        _ => {}
    });

    symbol_kinds
}

fn format_missing_import_item_help(module: &str, exported_symbols: &HashSet<String>) -> String {
    if exported_symbols.is_empty() {
        return format!("Module `{module}` does not define importable symbols.");
    }

    let mut symbols = exported_symbols.iter().cloned().collect::<Vec<_>>();
    symbols.sort();
    format!(
        "Check the imported name or add it to `{module}.cbl`. Available symbols: {}",
        symbols.join(", ")
    )
}

fn collect_cross_file_call_diagnostics(
    file_index: usize,
    file: &ParsedSourceFile,
    call_sites: &[semantic::CallSite],
    signatures: &HashMap<String, FileFunctionSignature>,
    output: &mut BTreeMap<usize, Vec<SourceDiagnostic>>,
) {
    let imported_modules = file
        .program
        .imports
        .iter()
        .filter(|import| import.module != "stdlib" && import.items.is_empty())
        .map(|import| import.module.as_str())
        .collect::<HashSet<_>>();

    for call in call_sites {
        if call.name == "range" {
            continue;
        }

        let qualified_signature =
            qualified_imported_function_signature(&call.name, &imported_modules, signatures);
        let Some(signature) = signatures.get(&call.name).or(qualified_signature) else {
            // Unresolved names are only reported when the call is an entire
            // expression statement, matching the old scan's
            // `is_user_function_call_statement` gate.
            if !call.is_statement_expression {
                continue;
            }

            if let Some((module, method)) = super::split_module_call_name(&call.name) {
                if let Some(message) =
                    super::value_only_module_call_message(module, method, &call.name)
                {
                    output.entry(file_index).or_default().push(
                        SourceDiagnostic::error(
                            "unsupported-function-call-expression",
                            call.line,
                            call.column,
                            message,
                        )
                        .with_help(
                            "Pass the returned value to another helper or JSON resource value instead.",
                        ),
                    );
                    continue;
                }

                if super::is_known_module_call(module, method) {
                    continue;
                }

                output.entry(file_index).or_default().push(
                    SourceDiagnostic::error(
                        "undefined-function",
                        call.line,
                        call.column,
                        format!("Unknown helper function `{}`", call.name),
                    )
                    .with_help(
                        "Use a documented Cobble helper module/function, define a Cobble function, or use a raw Minecraft command for external behavior.",
                    ),
                );
                continue;
            }

            if !super::is_allowed_value_function_call(&call.name) {
                output.entry(file_index).or_default().push(
                    SourceDiagnostic::error(
                        "undefined-function",
                        call.line,
                        call.column,
                        format!("Undefined function `{}`", call.name),
                    )
                    .with_help(
                        "Define the Cobble function, import a file that defines it, or use a raw `/function namespace:path` command for external Minecraft functions.",
                    ),
                );
            }
            continue;
        };

        if call.arg_count != signature.params.len() {
            output.entry(file_index).or_default().push(
                SourceDiagnostic::error(
                    "function-argument-count",
                    call.line,
                    call.column,
                    format!(
                        "Function `{}` expects {} argument(s), but {} provided",
                        signature.name,
                        signature.params.len(),
                        call.arg_count
                    ),
                )
                .with_help(format!(
                    "Expected parameters: ({}). Definition is at {}.",
                    signature.params.join(", "),
                    format_file_signature_location(signature)
                )),
            );
        }
    }
}

fn qualified_imported_function_signature<'a>(
    call_name: &str,
    imported_modules: &HashSet<&str>,
    signatures: &'a HashMap<String, FileFunctionSignature>,
) -> Option<&'a FileFunctionSignature> {
    let (module, method) = super::split_module_call_name(call_name)?;
    imported_modules
        .contains(module)
        .then(|| signatures.get(method))
        .flatten()
}

fn file_diagnostics_from_map(
    files: &[ParsedSourceFile],
    diagnostics_by_file: BTreeMap<usize, Vec<SourceDiagnostic>>,
) -> Vec<FileSourceDiagnostics> {
    diagnostics_by_file
        .into_iter()
        .map(|(file_index, diagnostics)| {
            let file = &files[file_index];
            FileSourceDiagnostics::new(&file.path, &file.source, diagnostics)
        })
        .collect()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileFunctionSignature {
    path: PathBuf,
    name: String,
    params: Vec<String>,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum FileSymbolKind {
    SelectorAlias,
    EntityTemplate,
}

impl FileSymbolKind {
    fn label(self) -> &'static str {
        match self {
            FileSymbolKind::SelectorAlias => "selector alias",
            FileSymbolKind::EntityTemplate => "entity template",
        }
    }

    fn prefix(self) -> &'static str {
        match self {
            FileSymbolKind::SelectorAlias | FileSymbolKind::EntityTemplate => "@",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FileNamedSymbol {
    path: PathBuf,
    name: String,
    kind: FileSymbolKind,
    line: usize,
    column: usize,
}

impl FileNamedSymbol {
    fn display_name(&self) -> String {
        format!("{}{}", self.kind.prefix(), self.name)
    }
}

fn format_file_signature_location(signature: &FileFunctionSignature) -> String {
    format!(
        "{}:{}:{}",
        signature.path.display(),
        signature.line,
        signature.column
    )
}

fn format_file_symbol_location(symbol: &FileNamedSymbol) -> String {
    format!(
        "{}:{}:{}",
        symbol.path.display(),
        symbol.line,
        symbol.column
    )
}

fn canonical_or_original(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Position of the module name inside an `import`/`from` header, derived
/// from the header's source span (the first non-whitespace byte after the
/// leading keyword). Every entry point threads the span table's import
/// spans here (the wasm in-memory path included), so a missing span is a
/// span-builder failure on parseable source — reported by the alignment
/// `debug_assert`s at the call sites — and degrades to the 1:1 fallback
/// position instead of a whole-source line scan.
fn import_site_position(source: &str, span: Option<SourceSpan>, import: &Import) -> (usize, usize) {
    let Some(span) = span else {
        return (1, 1);
    };
    let slice = source.get(span.start..span.end).unwrap_or("");
    let keyword = if import.items.is_empty() {
        "import"
    } else {
        "from"
    };
    let rest = slice.strip_prefix(keyword).unwrap_or(slice);
    let offset = (slice.len() - rest.len()) + (rest.len() - rest.trim_start().len());
    source_position_at(source, span.start + offset)
}

/// Position of one imported item inside a `from module import items`
/// header, walking the comma-separated item list of the header's span
/// slice.
fn import_item_position(
    source: &str,
    span: Option<SourceSpan>,
    item: &str,
) -> Option<(usize, usize)> {
    let span = span?;
    let slice = source.get(span.start..span.end)?;
    let masked = mask_non_code(slice);
    let items_offset = masked.find(" import ")? + " import ".len();
    let items = &masked[items_offset..];

    let mut search_from = 0;
    while search_from < items.len() {
        let item_text = items[search_from..]
            .split_once(',')
            .map(|(head, _)| head)
            .unwrap_or(&items[search_from..]);
        let leading = item_text.len() - item_text.trim_start().len();
        let candidate = item_text.trim().trim_end_matches(':');
        if candidate == item {
            return Some(source_position_at(
                source,
                span.start + items_offset + search_from + leading,
            ));
        }
        search_from += item_text.len() + 1;
    }

    None
}

fn add_import_chain_help(
    diagnostics: &mut [SourceDiagnostic],
    stack: &[PathBuf],
    next_path: &Path,
) {
    let chain_help = format!("Import chain: {}", format_import_chain(stack, next_path));
    for diagnostic in diagnostics {
        diagnostic.help = Some(match diagnostic.help.take() {
            Some(help) => format!("{help}\n{chain_help}"),
            None => chain_help.clone(),
        });
    }
}

fn format_import_chain(stack: &[PathBuf], next_path: &Path) -> String {
    let mut parts = stack
        .iter()
        .map(|path| path.display().to_string())
        .collect::<Vec<_>>();
    parts.push(next_path.display().to_string());
    parts.join(" -> ")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn simple_module_names_are_identifier_like() {
        assert!(is_simple_module_name("helpers"));
        assert!(is_simple_module_name("_helpers2"));
        assert!(!is_simple_module_name("helpers.extra"));
        assert!(!is_simple_module_name("2helpers"));
    }
}
