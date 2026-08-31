use crate::ast::Import;

use super::reporting::{FileSourceDiagnostics, ParsedSourceFile, SourceDiagnostic};
use super::text::{column_from_byte, is_ident_char, mask_non_code, single_equals_index};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

// Keep diagnostics import traversal aligned with the transpiler guard so
// preflight validation cannot recurse deeply enough to exhaust the stack.
pub(super) const MAX_IMPORT_DEPTH: usize = crate::transpiler::MAX_IMPORT_DEPTH;

pub fn analyze_in_memory_imports(source: &str, imports: &[Import]) -> Vec<SourceDiagnostic> {
    let mut diagnostics = Vec::new();

    for import in imports {
        if import.module == "stdlib" {
            continue;
        }

        let (line, column) = find_import_location(source, import).unwrap_or((1, 1));
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
            Err(mut file_diagnostics) => diagnostics.append(&mut file_diagnostics),
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
        &program.imports,
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

fn analyze_import_tree(
    current_path: &Path,
    current_source: &str,
    imports: &[Import],
    visited: &mut HashSet<PathBuf>,
    stack: &mut Vec<PathBuf>,
    output: &mut Vec<FileSourceDiagnostics>,
    parsed_imports: &mut Vec<ParsedSourceFile>,
) {
    let current_dir = current_path.parent().unwrap_or_else(|| Path::new("."));

    for import in imports {
        if import.module == "stdlib" {
            continue;
        }

        let (line, column) = find_import_location(current_source, import).unwrap_or((1, 1));

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
            if let Some(diagnostics) =
                validate_import_items_from_path(current_path, current_source, import, &import_path)
            {
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

        if let Some(diagnostics) =
            validate_import_items(current_path, current_source, import, &imported_source)
        {
            output.push(diagnostics);
            continue;
        }

        visited.insert(canonical_path.clone());
        stack.push(canonical_path.clone());
        analyze_import_tree(
            &import_path,
            &imported_source,
            &imported_program.imports,
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
    let mut signatures = HashMap::new();
    let mut duplicate_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();

    for (file_index, file) in files.iter().enumerate() {
        for signature in collect_file_function_signatures(&file.path, &file.source) {
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
                signatures.insert(signature.name.clone(), signature);
            }
        }
    }

    if !duplicate_diagnostics.is_empty() {
        return file_diagnostics_from_map(files, duplicate_diagnostics);
    }

    let duplicate_symbol_diagnostics = analyze_cross_file_named_symbols(files);
    if !duplicate_symbol_diagnostics.is_empty() {
        return duplicate_symbol_diagnostics;
    }

    let mut call_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();
    for (file_index, file) in files.iter().enumerate() {
        collect_cross_file_call_diagnostics(file_index, file, &signatures, &mut call_diagnostics);
    }

    file_diagnostics_from_map(files, call_diagnostics)
}

fn analyze_cross_file_named_symbols(files: &[ParsedSourceFile]) -> Vec<FileSourceDiagnostics> {
    let mut symbols = HashMap::new();
    let mut duplicate_diagnostics: BTreeMap<usize, Vec<SourceDiagnostic>> = BTreeMap::new();

    for (file_index, file) in files.iter().enumerate() {
        for symbol in collect_file_named_symbols(&file.path, &file.source) {
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

fn validate_import_items_from_path(
    current_path: &Path,
    current_source: &str,
    import: &Import,
    import_path: &Path,
) -> Option<FileSourceDiagnostics> {
    if import.items.is_empty() || import.module == "stdlib" {
        return None;
    }

    let imported_source = fs::read_to_string(import_path).ok()?;
    validate_import_items(current_path, current_source, import, &imported_source)
}

fn validate_import_items(
    current_path: &Path,
    current_source: &str,
    import: &Import,
    imported_source: &str,
) -> Option<FileSourceDiagnostics> {
    if import.items.is_empty() || import.module == "stdlib" {
        return None;
    }

    let exported_symbol_kinds = collect_exported_symbol_kinds(imported_source);
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

                for (line, column) in super::raw_command_placeholder_locations(current_source, item)
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
                let (line, column) = find_import_item_location(current_source, import, item)
                    .unwrap_or_else(|| {
                        find_import_location(current_source, import).unwrap_or((1, 1))
                    });

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

fn collect_exported_symbol_kinds(source: &str) -> HashMap<String, ExportedSymbolKind> {
    let mut symbols = HashSet::new();
    let mut symbol_kinds = HashMap::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if super::should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(signature) = super::parse_function_signature(trimmed, line_number, indent) {
            symbol_kinds.insert(signature.name, ExportedSymbolKind::Function);
            continue;
        }

        if let Some(name) = selector_alias_name(trimmed) {
            symbol_kinds.insert(name.to_string(), ExportedSymbolKind::SelectorAlias);
            continue;
        }

        if let Some(name) = entity_definition_name(trimmed) {
            symbol_kinds.insert(name.to_string(), ExportedSymbolKind::EntityTemplate);
            continue;
        }

        collect_assignment_export(trimmed, &mut symbols);
        for name in symbols.drain() {
            symbol_kinds.insert(name, ExportedSymbolKind::Value);
        }
        collect_global_export(trimmed, &mut symbols);
        for name in symbols.drain() {
            symbol_kinds.insert(name, ExportedSymbolKind::Value);
        }
    }

    symbol_kinds
}

fn selector_alias_name(trimmed: &str) -> Option<&str> {
    if !super::looks_like_selector_definition(trimmed) {
        return None;
    }
    let left = trimmed.split_once('=')?.0.trim();
    left.strip_prefix('@')
        .filter(|name| is_simple_module_name(name))
}

fn entity_definition_name(trimmed: &str) -> Option<&str> {
    let rest = trimmed.strip_prefix("define ")?.trim_start();
    let name = rest.split_whitespace().next()?.strip_prefix('@')?;
    if is_simple_module_name(name) {
        Some(name)
    } else {
        None
    }
}

fn collect_assignment_export(trimmed: &str, symbols: &mut HashSet<String>) {
    if super::should_skip_assignment_symbol_scan(trimmed) {
        return;
    }

    let Some(equals_index) = single_equals_index(trimmed) else {
        return;
    };
    let raw_target = trimmed[..equals_index].trim();
    let target = raw_target
        .strip_prefix("const ")
        .unwrap_or(raw_target)
        .trim();
    if is_simple_module_name(target) {
        symbols.insert(target.to_string());
    }
}

fn collect_global_export(trimmed: &str, symbols: &mut HashSet<String>) {
    let Some(rest) = trimmed.strip_prefix("global ") else {
        return;
    };
    for name in rest.trim_end_matches(':').split(',') {
        let name = name.trim();
        if is_simple_module_name(name) {
            symbols.insert(name.to_string());
        }
    }
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
    signatures: &HashMap<String, FileFunctionSignature>,
    output: &mut BTreeMap<usize, Vec<SourceDiagnostic>>,
) {
    let mut active_docstring_quote = None;
    let mut multiline_expression_depth = 0usize;
    let imported_modules = file
        .program
        .imports
        .iter()
        .filter(|import| import.module != "stdlib" && import.items.is_empty())
        .map(|import| import.module.as_str())
        .collect::<HashSet<_>>();
    for (line_index, line) in file.source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if super::should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if super::should_skip_call_argument_scan(trimmed) {
            super::update_delimiter_depth_for_indentation(trimmed, &mut multiline_expression_depth);
            continue;
        }

        let inside_multiline_expression = multiline_expression_depth > 0;
        let indent = masked.len() - trimmed.len();
        if !inside_multiline_expression {
            for call in super::function_calls_in_expression(trimmed) {
                if call.name == "range" {
                    continue;
                }

                let qualified_signature = qualified_imported_function_signature(
                    &call.name,
                    &imported_modules,
                    signatures,
                );
                let Some(signature) = signatures.get(&call.name).or(qualified_signature) else {
                    if !super::is_user_function_call_statement(trimmed, &call) {
                        continue;
                    }

                    if let Some((module, method)) = super::split_module_call_name(&call.name) {
                        if let Some(message) =
                            super::value_only_module_call_message(module, method, &call.name)
                        {
                            output.entry(file_index).or_default().push(
                                SourceDiagnostic::error(
                                    "unsupported-function-call-expression",
                                    line_number,
                                    column_from_byte(&masked, indent + call.offset),
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
                                line_number,
                                column_from_byte(&masked, indent + call.offset),
                                format!("Unknown helper function `{}`", call.name),
                            )
                            .with_help(
                                "Use a documented Cobble helper module/function, define a Cobble function, or use a raw Minecraft command for external behavior.",
                            ),
                        );
                        continue;
                    }

                    if !super::is_known_non_user_function_call(&call.name) {
                        output.entry(file_index).or_default().push(
                            SourceDiagnostic::error(
                                "undefined-function",
                                line_number,
                                column_from_byte(&masked, indent + call.offset),
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
                            line_number,
                            column_from_byte(&masked, indent + call.offset),
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
                    continue;
                }
            }
        }
        super::update_delimiter_depth_for_indentation(trimmed, &mut multiline_expression_depth);
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

fn collect_file_function_signatures(path: &Path, source: &str) -> Vec<FileFunctionSignature> {
    let mut signatures = Vec::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if super::should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(signature) = super::parse_function_signature(trimmed, line_number, indent) {
            signatures.push(FileFunctionSignature {
                path: path.to_path_buf(),
                name: signature.name,
                params: signature.params,
                line: signature.line,
                column: signature.column,
            });
        }
    }

    signatures
}

fn collect_file_named_symbols(path: &Path, source: &str) -> Vec<FileNamedSymbol> {
    let mut symbols = Vec::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if super::should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(name) = selector_alias_name(trimmed) {
            symbols.push(FileNamedSymbol {
                path: path.to_path_buf(),
                name: name.to_string(),
                kind: FileSymbolKind::SelectorAlias,
                line: line_number,
                column: column_from_byte(line, indent),
            });
            continue;
        }

        if let Some(name) = entity_definition_name(trimmed) {
            let name_offset = trimmed.find(name).unwrap_or(0);
            symbols.push(FileNamedSymbol {
                path: path.to_path_buf(),
                name: name.to_string(),
                kind: FileSymbolKind::EntityTemplate,
                line: line_number,
                column: indent + column_from_byte(trimmed, name_offset.saturating_sub(1)),
            });
        }
    }

    symbols
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

fn is_simple_module_name(module: &str) -> bool {
    let mut chars = module.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_') && chars.all(is_ident_char)
}

fn find_import_location(source: &str, import: &Import) -> Option<(usize, usize)> {
    for (line_index, line) in source.lines().enumerate() {
        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        let indent = masked.len() - trimmed.len();

        if import.items.is_empty() {
            let Some(rest) = trimmed.strip_prefix("import ") else {
                continue;
            };
            let module = rest
                .split(|ch: char| ch.is_whitespace() || ch == ',')
                .next()
                .unwrap_or("");
            if module == import.module {
                return Some((
                    line_index + 1,
                    column_from_byte(&masked, indent + "import ".len()),
                ));
            }
        } else {
            let Some(rest) = trimmed.strip_prefix("from ") else {
                continue;
            };
            let Some((module, _items)) = rest.split_once(" import ") else {
                continue;
            };
            if module.trim() == import.module {
                return Some((
                    line_index + 1,
                    column_from_byte(&masked, indent + "from ".len()),
                ));
            }
        }
    }

    None
}

fn find_import_item_location(source: &str, import: &Import, item: &str) -> Option<(usize, usize)> {
    for (line_index, line) in source.lines().enumerate() {
        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        let indent = masked.len() - trimmed.len();

        let Some(rest) = trimmed.strip_prefix("from ") else {
            continue;
        };
        let Some((module, items)) = rest.split_once(" import ") else {
            continue;
        };
        if module.trim() != import.module {
            continue;
        }

        let items_offset = indent + "from ".len() + module.len() + " import ".len();
        let mut search_from = 0;
        while search_from < items.len() {
            let item_text = items[search_from..]
                .split_once(',')
                .map(|(head, _)| head)
                .unwrap_or(&items[search_from..]);
            let leading = item_text.len() - item_text.trim_start().len();
            let candidate = item_text.trim().trim_end_matches(':');
            if candidate == item {
                return Some((
                    line_index + 1,
                    column_from_byte(&masked, items_offset + search_from + leading),
                ));
            }
            search_from += item_text.len() + 1;
        }
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
