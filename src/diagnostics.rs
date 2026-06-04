use crate::ast::{CobbleType, Import, Program};
use crate::parser::parse;
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiagnosticSeverity {
    Error,
    Warning,
}

impl DiagnosticSeverity {
    pub fn as_str(self) -> &'static str {
        match self {
            DiagnosticSeverity::Error => "error",
            DiagnosticSeverity::Warning => "warning",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceDiagnostic {
    pub severity: DiagnosticSeverity,
    pub kind: String,
    pub line: usize,
    pub column: usize,
    pub message: String,
    pub help: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileSourceDiagnostics {
    pub path: PathBuf,
    pub source: String,
    pub diagnostics: Vec<SourceDiagnostic>,
}

impl FileSourceDiagnostics {
    pub fn new(
        path: impl Into<PathBuf>,
        source: impl Into<String>,
        diagnostics: Vec<SourceDiagnostic>,
    ) -> Self {
        Self {
            path: path.into(),
            source: source.into(),
            diagnostics,
        }
    }

    pub fn format_compact(&self) -> String {
        format_diagnostics_with_source(
            &self.path.display().to_string(),
            &self.source,
            &self.diagnostics,
        )
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ParsedSourceFile {
    pub path: PathBuf,
    pub source: String,
    pub program: Program,
}

impl SourceDiagnostic {
    pub fn error(
        kind: impl Into<String>,
        line: usize,
        column: usize,
        message: impl Into<String>,
    ) -> Self {
        Self {
            severity: DiagnosticSeverity::Error,
            kind: kind.into(),
            line: line.max(1),
            column: column.max(1),
            message: message.into(),
            help: None,
        }
    }

    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    pub fn format_compact(&self, filename: &str) -> String {
        let mut output = format!(
            "{}:{}:{}: {}[{}] {}",
            filename,
            self.line,
            self.column,
            self.severity.as_str(),
            self.kind,
            self.message
        );
        if let Some(help) = &self.help {
            output.push_str("\n  help: ");
            output.push_str(help);
        }
        output
    }

    pub fn format_with_source(&self, filename: &str, source: &str) -> String {
        let mut output = self.format_header(filename);
        if let Some(snippet) = format_source_snippet(source, self.line, self.column) {
            output.push('\n');
            output.push_str(&snippet);
        }
        if let Some(help) = &self.help {
            output.push('\n');
            output.push_str(&format_help(help));
        }
        output
    }

    fn format_header(&self, filename: &str) -> String {
        format!(
            "{}:{}:{}: {}[{}] {}",
            filename,
            self.line,
            self.column,
            self.severity.as_str(),
            self.kind,
            self.message
        )
    }
}

pub fn format_diagnostics(filename: &str, diagnostics: &[SourceDiagnostic]) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.format_compact(filename))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_diagnostics_with_source(
    filename: &str,
    source: &str,
    diagnostics: &[SourceDiagnostic],
) -> String {
    diagnostics
        .iter()
        .map(|diagnostic| diagnostic.format_with_source(filename, source))
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn format_file_diagnostics(diagnostics: &[FileSourceDiagnostics]) -> String {
    diagnostics
        .iter()
        .map(FileSourceDiagnostics::format_compact)
        .collect::<Vec<_>>()
        .join("\n")
}

fn format_source_snippet(source: &str, line: usize, column: usize) -> Option<String> {
    let source_line = source.lines().nth(line.checked_sub(1)?)?;
    let gutter = line.to_string();
    let caret_padding = source_line
        .chars()
        .take(column.saturating_sub(1))
        .map(|ch| if ch == '\t' { '\t' } else { ' ' })
        .collect::<String>();
    let gutter_padding = " ".repeat(gutter.len());

    Some(format!(
        "{gutter_padding} |\n{gutter} | {source_line}\n{gutter_padding} | {caret_padding}^"
    ))
}

fn format_help(help: &str) -> String {
    let mut output = String::new();
    for (index, line) in help.lines().enumerate() {
        if index == 0 {
            output.push_str("  help: ");
        } else {
            output.push_str("\n        ");
        }
        output.push_str(line);
    }
    output
}

pub fn parse_source(source: &str) -> Result<Program, Vec<SourceDiagnostic>> {
    let diagnostics = analyze_source(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    parse(source).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| SourceDiagnostic::error("parse", 1, 1, format!("Parse error: {error}")))
            .collect()
    })
}

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

    let program = parse_source(&source)
        .map_err(|diagnostics| vec![FileSourceDiagnostics::new(path, &source, diagnostics)])?;

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
        program: program.clone(),
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

        let imported_program = match parse_source(&imported_source) {
            Ok(program) => program,
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

                for (line, column) in raw_command_placeholder_locations(current_source, item) {
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
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(signature) = parse_function_signature(line, trimmed, line_number, indent) {
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
    if !looks_like_selector_definition(trimmed) {
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
    if should_skip_assignment_symbol_scan(trimmed) {
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
    for (line_index, line) in file.source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if should_skip_call_argument_scan(trimmed) {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        for call in function_calls_in_expression(trimmed) {
            let Some(signature) = signatures.get(&call.name) else {
                if !is_user_function_call_statement(trimmed, &call) {
                    continue;
                }

                if let Some((module, method)) = split_module_call_name(&call.name) {
                    if is_value_only_module_call(module, method) {
                        output.entry(file_index).or_default().push(
                            SourceDiagnostic::error(
                                "unsupported-function-call-expression",
                                line_number,
                                column_from_byte(line, indent + call.offset),
                                format!(
                                    "`{}` returns a value and cannot be used as a standalone statement",
                                    call.name
                                ),
                            )
                            .with_help(
                                "Pass the returned value to another helper or JSON resource value instead.",
                            ),
                        );
                        continue;
                    }

                    if is_known_module_call(module, method) {
                        continue;
                    }

                    output.entry(file_index).or_default().push(
                        SourceDiagnostic::error(
                            "undefined-function",
                            line_number,
                            column_from_byte(line, indent + call.offset),
                            format!("Unknown helper function `{}`", call.name),
                        )
                        .with_help(
                            "Use a documented Cobble helper module/function, define a Cobble function, or use a raw Minecraft command for external behavior.",
                        ),
                    );
                    continue;
                }

                if !is_known_non_user_function_call(&call.name) {
                    output.entry(file_index).or_default().push(
                        SourceDiagnostic::error(
                            "undefined-function",
                            line_number,
                            column_from_byte(line, indent + call.offset),
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
                        column_from_byte(line, indent + call.offset),
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

            for argument in &call.arguments {
                let Some(nested_call) = function_calls_in_expression(&argument.text)
                    .into_iter()
                    .next()
                else {
                    continue;
                };

                output.entry(file_index).or_default().push(
                    SourceDiagnostic::error(
                        "unsupported-function-call-argument",
                        line_number,
                        column_from_byte(line, indent + argument.offset + nested_call.offset),
                        format!(
                            "Function `{}` arguments cannot contain function call expressions",
                            signature.name
                        ),
                    )
                    .with_help(format!(
                        "Call Cobble functions as standalone statements, or pass a literal, variable, selector, or storage-backed value. Definition is at {}.",
                        format_file_signature_location(signature)
                    )),
                );
            }
        }
    }
}

fn collect_file_function_signatures(path: &Path, source: &str) -> Vec<FileFunctionSignature> {
    let mut signatures = Vec::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(signature) = parse_function_signature(line, trimmed, line_number, indent) {
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
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
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
                column: column_from_byte(line, indent + name_offset.saturating_sub(1)),
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

pub fn analyze_source(source: &str) -> Vec<SourceDiagnostic> {
    let mut diagnostics = Vec::new();
    check_structural_syntax(source, &mut diagnostics);
    if !diagnostics.is_empty() {
        return diagnostics;
    }
    check_indentation_syntax(source, &mut diagnostics);
    if !diagnostics.is_empty() {
        return diagnostics;
    }

    let mut active_for_blocks: Vec<usize> = Vec::new();
    let mut active_docstring_quote: Option<char> = None;
    let mut multiline_expression_depth = 0usize;
    let mut function_defs: HashMap<String, FunctionSignature> = HashMap::new();

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        if let Some(quote) = active_docstring_quote {
            if find_triple_quote(line, quote, 0).is_some() {
                active_docstring_quote = None;
            }
            continue;
        }

        let raw_trimmed = line.trim_start();
        if let Some(quote) = leading_triple_quote(raw_trimmed) {
            if find_triple_quote(raw_trimmed, quote, 3).is_none() {
                active_docstring_quote = Some(quote);
            }
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if multiline_expression_depth > 0 {
            if !trimmed.starts_with('/') {
                update_delimiter_depth_for_indentation(&masked, &mut multiline_expression_depth);
            }
            continue;
        }

        let indent = masked.len() - trimmed.len();
        let trimmed_column = column_from_byte(line, indent);
        let is_for_else = starts_with_keyword(trimmed, "else")
            && active_for_blocks.last().copied() == Some(indent);

        while let Some(block_indent) = active_for_blocks.last().copied() {
            if indent < block_indent || (indent == block_indent && !is_for_else) {
                active_for_blocks.pop();
            } else {
                break;
            }
        }

        if trimmed.starts_with('/') {
            continue;
        }

        if is_for_else {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-control-flow",
                    line_number,
                    trimmed_column,
                    "`for ... else` blocks are not supported",
                )
                .with_help("Use an explicit flag variable and a normal `if` after the loop."),
            );
        }

        if starts_with_keyword(trimmed, "for") && trimmed.ends_with(':') {
            active_for_blocks.push(indent);
        }

        check_missing_block_colon(line, trimmed, line_number, &mut diagnostics);
        check_decorator(line, trimmed, line_number, trimmed_column, &mut diagnostics);
        check_unsupported_keywords(line, trimmed, line_number, indent, &mut diagnostics);
        check_imports(line, trimmed, line_number, indent, &mut diagnostics);
        check_function_definition(
            line,
            trimmed,
            line_number,
            indent,
            &mut function_defs,
            &mut diagnostics,
        );
        check_function_parameters(line, trimmed, line_number, indent, &mut diagnostics);
        check_return_statement(line, trimmed, line_number, indent, &mut diagnostics);
        check_compound_assignment(line, &masked, line_number, &mut diagnostics);
        check_assignment_target(line, trimmed, line_number, indent, &mut diagnostics);
        check_assignment_function_calls(line, trimmed, line_number, indent, &mut diagnostics);
        check_comprehension(line, &masked, line_number, &mut diagnostics);
        check_datapack_helper_argument_shapes(
            line,
            trimmed,
            raw_trimmed,
            line_number,
            indent,
            &mut diagnostics,
        );
        check_noop_expression_statement(
            line,
            trimmed,
            raw_trimmed,
            line_number,
            indent,
            &mut diagnostics,
        );

        if !trimmed.starts_with('/') {
            update_delimiter_depth_for_indentation(&masked, &mut multiline_expression_depth);
        }
    }

    if diagnostics.is_empty() {
        check_multiline_datapack_helper_argument_shapes(source, &mut diagnostics);
    }

    check_user_function_call_arguments(source, &function_defs, &mut diagnostics);
    if diagnostics.is_empty() && !has_non_stdlib_imports(source) {
        check_undefined_function_calls(source, &function_defs, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        let interpolation_symbols = collect_interpolation_symbols(source);
        check_raw_command_placeholders(source, &interpolation_symbols, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        check_unsupported_none_usage(source, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        check_storage_backed_access(source, &mut diagnostics);
        check_type_mismatches(source, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        let symbols = collect_defined_symbols(source, &function_defs);
        check_undefined_variable_references(source, &symbols, &mut diagnostics);
    }

    diagnostics
}

fn check_indentation_syntax(source: &str, diagnostics: &mut Vec<SourceDiagnostic>) {
    let mut indent_stack = vec![0usize];
    let mut delimiter_depth = 0usize;
    let mut previous_allows_indent = false;
    let mut active_docstring_quote: Option<char> = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();

        if let Some(quote) = active_docstring_quote {
            if find_triple_quote(line, quote, 0).is_some() {
                active_docstring_quote = None;
            }
            continue;
        }

        if raw_trimmed.is_empty() || raw_trimmed.starts_with('#') {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        if delimiter_depth == 0 {
            let indent = masked.len() - trimmed.len();
            let current_indent = *indent_stack.last().unwrap_or(&0);

            if indent > current_indent {
                if !previous_allows_indent {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unexpected-indentation",
                            line_number,
                            column_from_byte(line, indent),
                            "Unexpected indentation",
                        )
                        .with_help(
                            "Only indent after a block header ending with `:` or inside a multi-line expression.",
                        ),
                    );
                }
                indent_stack.push(indent);
            } else if indent < current_indent {
                while indent_stack.len() > 1 && *indent_stack.last().unwrap() > indent {
                    indent_stack.pop();
                }

                if *indent_stack.last().unwrap_or(&0) != indent {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "inconsistent-indentation",
                            line_number,
                            column_from_byte(line, indent),
                            "Indentation does not match a previous block level",
                        )
                        .with_help(format!(
                            "Use one of the active indentation levels: {}.",
                            format_indent_levels(&indent_stack)
                        )),
                    );
                    indent_stack.push(indent);
                }
            }
        }

        if !trimmed.starts_with('/') {
            update_delimiter_depth_for_indentation(&masked, &mut delimiter_depth);
        }

        previous_allows_indent =
            delimiter_depth == 0 && (trimmed.ends_with(':') || looks_like_block_header(trimmed));

        if let Some(quote) = leading_triple_quote(raw_trimmed) {
            let after_open = 3;
            if find_triple_quote(raw_trimmed, quote, after_open).is_none() {
                active_docstring_quote = Some(quote);
            }
        }
    }
}

fn update_delimiter_depth_for_indentation(masked_line: &str, depth: &mut usize) {
    for ch in masked_line.chars() {
        match ch {
            '(' | '[' | '{' => *depth += 1,
            ')' | ']' | '}' => {
                *depth = depth.saturating_sub(1);
            }
            _ => {}
        }
    }
}

fn should_skip_docstring_scan_line(
    line: &str,
    raw_trimmed: &str,
    active_quote: &mut Option<char>,
) -> bool {
    if let Some(quote) = *active_quote {
        if find_triple_quote(line, quote, 0).is_some() {
            *active_quote = None;
        }
        return true;
    }

    if let Some(quote) = leading_triple_quote(raw_trimmed) {
        if find_triple_quote(raw_trimmed, quote, 3).is_none() {
            *active_quote = Some(quote);
        }
        return true;
    }

    false
}

fn format_indent_levels(levels: &[usize]) -> String {
    levels
        .iter()
        .map(|level| level.to_string())
        .collect::<Vec<_>>()
        .join(", ")
}

fn check_structural_syntax(source: &str, diagnostics: &mut Vec<SourceDiagnostic>) {
    let mut delimiters = Vec::new();
    let mut triple_quote: Option<QuoteFrame> = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let mut index = 0;

        if let Some(active_quote) = triple_quote {
            let Some(close_index) = find_triple_quote(line, active_quote.quote, 0) else {
                continue;
            };
            index = close_index + 3;
            triple_quote = None;
        }

        if line.trim_start().starts_with('/') {
            continue;
        }

        while index < line.len() {
            let ch = line[index..].chars().next().unwrap();
            let next_index = index + ch.len_utf8();

            match ch {
                '#' => break,
                '"' | '\'' => {
                    if line[index..].starts_with(triple_quote_pattern(ch)) {
                        let after_open = index + 3;
                        if let Some(close_index) = find_triple_quote(line, ch, after_open) {
                            index = close_index + 3;
                        } else {
                            triple_quote = Some(QuoteFrame {
                                quote: ch,
                                line: line_number,
                                column: column_from_byte(line, index),
                            });
                            break;
                        }
                    } else if let Some(close_index) = find_string_end(line, ch, next_index) {
                        index = close_index;
                    } else {
                        diagnostics.push(
                            SourceDiagnostic::error(
                                "unterminated-string",
                                line_number,
                                column_from_byte(line, index),
                                "String literal is missing a closing quote",
                            )
                            .with_help("Close the string on the same line, or use a triple-quoted docstring when documenting a block."),
                        );
                        break;
                    }
                }
                '(' | '[' | '{' => {
                    delimiters.push(DelimiterFrame {
                        delimiter: ch,
                        line: line_number,
                        column: column_from_byte(line, index),
                    });
                    index = next_index;
                }
                ')' | ']' | '}' => {
                    let close_column = column_from_byte(line, index);
                    match delimiters.last().copied() {
                        Some(open) if matching_close_delimiter(open.delimiter) == ch => {
                            delimiters.pop();
                        }
                        Some(open) => {
                            diagnostics.push(
                                SourceDiagnostic::error(
                                    "unmatched-delimiter",
                                    line_number,
                                    close_column,
                                    format!("Unexpected closing delimiter `{ch}`"),
                                )
                                .with_help(format!(
                                    "The delimiter `{}` opened at line {}, column {} must close with `{}` before `{ch}`.",
                                    open.delimiter,
                                    open.line,
                                    open.column,
                                    matching_close_delimiter(open.delimiter)
                                )),
                            );
                            delimiters.pop();
                        }
                        None => diagnostics.push(
                            SourceDiagnostic::error(
                                "unmatched-delimiter",
                                line_number,
                                close_column,
                                format!("Unexpected closing delimiter `{ch}`"),
                            )
                            .with_help(format!(
                                "Remove `{ch}` or add a matching opening delimiter before it."
                            )),
                        ),
                    }
                    index = next_index;
                }
                _ => index = next_index,
            }
        }
    }

    if let Some(quote) = triple_quote {
        diagnostics.push(
            SourceDiagnostic::error(
                "unterminated-string",
                quote.line,
                quote.column,
                "Triple-quoted string is missing a closing delimiter",
            )
            .with_help(format!(
                "Add the closing `{}` before the end of the file.",
                triple_quote_pattern(quote.quote)
            )),
        );
    }

    for delimiter in delimiters {
        diagnostics.push(
            SourceDiagnostic::error(
                "unclosed-delimiter",
                delimiter.line,
                delimiter.column,
                format!("Opening delimiter `{}` is not closed", delimiter.delimiter),
            )
            .with_help(format!(
                "Add the matching `{}` before the expression ends.",
                matching_close_delimiter(delimiter.delimiter)
            )),
        );
    }
}

pub fn byte_offset_for_line_column(source: &str, line: usize, column: usize) -> usize {
    let target_line = line.max(1);
    let target_column = column.max(1);
    let mut offset = 0;

    for (index, source_line) in source.split_inclusive('\n').enumerate() {
        if index + 1 == target_line {
            let without_newline = source_line.strip_suffix('\n').unwrap_or(source_line);
            let without_line_ending = without_newline
                .strip_suffix('\r')
                .unwrap_or(without_newline);
            let column_offset = without_line_ending
                .char_indices()
                .nth(target_column.saturating_sub(1))
                .map(|(byte_index, _)| byte_index)
                .unwrap_or(without_line_ending.len());
            return offset + column_offset;
        }
        offset += source_line.len();
    }

    source.len()
}

fn check_decorator(
    line: &str,
    trimmed: &str,
    line_number: usize,
    column: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !trimmed.starts_with('@') || looks_like_selector_definition(trimmed) {
        return;
    }

    diagnostics.push(
        SourceDiagnostic::error(
            "unsupported-decorator",
            line_number,
            column,
            "Decorators are not supported",
        )
        .with_help(
            "Use explicit stdlib registration calls such as `stdlib.addEventListener(...)`.",
        ),
    );

    if trimmed.contains('=') && !line.contains(" = ") {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-assignment-target",
                line_number,
                column,
                "Selector aliases must use `@Name = @selector` syntax",
            )
            .with_help("Decorator arguments are not selector definitions."),
        );
    }
}

fn check_unsupported_keywords(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for (keyword, help) in [
        (
            "class",
            "Cobble has functions and compile-time helpers, but no classes.",
        ),
        ("try", "Minecraft functions do not have exception handling."),
        (
            "except",
            "Minecraft functions do not have exception handling.",
        ),
        (
            "finally",
            "Minecraft functions do not have exception handling.",
        ),
        (
            "with",
            "Use explicit function calls or resource declarations instead.",
        ),
        (
            "break",
            "Cobble loops compile to Minecraft function commands and do not support early loop exits.",
        ),
        (
            "continue",
            "Cobble loops compile to Minecraft function commands and do not support early loop continuation.",
        ),
        (
            "raise",
            "Minecraft functions do not have exception handling.",
        ),
        (
            "assert",
            "Use explicit `if` blocks and commands to report failed conditions.",
        ),
        (
            "del",
            "Cobble variables are generated scoreboard or storage state; delete statements are not supported.",
        ),
        (
            "nonlocal",
            "Cobble does not have Python-style nested runtime scopes.",
        ),
    ] {
        if starts_with_keyword(trimmed, keyword) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-python-syntax",
                    line_number,
                    column_from_byte(line, indent),
                    format!("`{keyword}` is not supported in Cobble"),
                )
                .with_help(help),
            );
        }
    }

    for (keyword, help) in [
        ("lambda", "Define a named Cobble function instead."),
        ("yield", "Cobble functions cannot yield runtime values."),
        ("await", "Cobble does not have async runtime semantics."),
    ] {
        if let Some(index) = find_word(trimmed, keyword) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-python-syntax",
                    line_number,
                    column_from_byte(line, indent + index),
                    format!("`{keyword}` is not supported in Cobble"),
                )
                .with_help(help),
            );
        }
    }

    if starts_with_keyword(trimmed, "async") {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-python-syntax",
                line_number,
                column_from_byte(line, indent),
                "`async` is not supported in Cobble",
            )
            .with_help("Cobble compiles to Minecraft functions and has no async runtime."),
        );
    }
}

fn check_missing_block_colon(
    line: &str,
    trimmed: &str,
    line_number: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if trimmed.ends_with(':') || !looks_like_block_header(trimmed) {
        return;
    }

    diagnostics.push(
        SourceDiagnostic::error(
            "missing-block-colon",
            line_number,
            column_from_byte(line, line.len()),
            "Block headers must end with `:`",
        )
        .with_help("Add `:` at the end of the line before the indented block."),
    );
}

fn looks_like_block_header(trimmed: &str) -> bool {
    if starts_with_keyword(trimmed, "def") {
        return trimmed.contains('(') && trimmed.contains(')');
    }

    for keyword in ["if", "elif", "else", "while", "for", "match", "case"] {
        if starts_with_keyword(trimmed, keyword) {
            return true;
        }
    }

    false
}

fn check_imports(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if let Some(rest) = trimmed.strip_prefix("import ") {
        let module = rest
            .split(|ch: char| ch.is_whitespace() || ch == ',')
            .next()
            .unwrap_or("");

        if let Some(index) = rest.find(',') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "import ".len() + index),
                    "Multiple modules in one import statement are not supported",
                )
                .with_help("Use one `import module` statement per line."),
            );
        }

        if let Some(index) = find_import_alias_keyword(rest) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "import ".len() + index),
                    "Import aliases are not supported",
                )
                .with_help(
                    "Use the original module name, or rename the source file/module explicitly.",
                ),
            );
        }

        if module.starts_with('.') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "import ".len()),
                    "Relative imports are not supported",
                )
                .with_help(
                    "Use simple module names such as `import helpers`; imports resolve relative to the importing file.",
                ),
            );
        } else if let Some(index) = module.find('.') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "import ".len() + index),
                    "Dotted imports are not supported",
                )
                .with_help(
                    "Use simple module names such as `import helpers`; imports resolve relative to the importing file.",
                ),
            );
        }
        return;
    }

    if let Some(rest) = trimmed.strip_prefix("from ") {
        let Some((module, items)) = rest.split_once(" import ") else {
            return;
        };

        let items_offset = indent + "from ".len() + module.len() + " import ".len();

        if module.starts_with('.') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "from ".len()),
                    "Relative imports are not supported",
                )
                .with_help(
                    "Use simple module names such as `from helpers import setup`; imports resolve relative to the importing file.",
                ),
            );
        } else if let Some(index) = module.find('.') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, indent + "from ".len() + index),
                    "Dotted imports are not supported",
                )
                .with_help(
                    "Use simple module names such as `from helpers import setup`; imports resolve relative to the importing file.",
                ),
            );
        }

        let leading = items.len() - items.trim_start().len();
        if items.trim_start().starts_with('*') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, items_offset + leading),
                    "Wildcard imports are not supported",
                )
                .with_help("Import explicit names such as `from helpers import setup`."),
            );
        }

        if let Some(index) = find_import_alias_keyword(items) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    column_from_byte(line, items_offset + index),
                    "Import aliases are not supported",
                )
                .with_help(
                    "Use the exported Cobble name directly; alias binding is not part of the language.",
                ),
            );
        }
    }
}

fn find_import_alias_keyword(text: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(relative_index) = text[search_from..].find("as") {
        let index = search_from + relative_index;
        let before = text[..index].chars().next_back();
        let after = text[index + "as".len()..].chars().next();

        if before.is_some_and(char::is_whitespace) && after.is_some_and(char::is_whitespace) {
            return Some(index);
        }

        search_from = index + "as".len();
    }

    None
}

fn check_function_parameters(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !starts_with_keyword(trimmed, "def") {
        return;
    }

    let Some(open) = trimmed.find('(') else {
        return;
    };
    let Some(close) = trimmed[open + 1..]
        .find(')')
        .map(|offset| open + 1 + offset)
    else {
        return;
    };

    let params = &trimmed[open + 1..close];
    if let Some(index) = params.find('*') {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-function-parameter",
                line_number,
                column_from_byte(line, indent + open + 1 + index),
                "`*args` and `**kwargs` parameters are not supported",
            )
            .with_help("List every Cobble function parameter explicitly."),
        );
    }

    if let Some(index) = params.find('=') {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-function-parameter",
                line_number,
                column_from_byte(line, indent + open + 1 + index),
                "Default parameter values are not supported",
            )
            .with_help("Use explicit arguments at each call site."),
        );
    }

    let mut seen_params: HashMap<String, usize> = HashMap::new();
    for param in split_top_level_arg_spans(params) {
        let name = param.text.trim();
        if !is_simple_module_name(name) {
            continue;
        }

        let column = column_from_byte(line, indent + open + 1 + param.offset);
        if let Some(first_column) = seen_params.insert(name.to_string(), column) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "duplicate-function-parameter",
                    line_number,
                    column,
                    format!("Duplicate function parameter `{name}`"),
                )
                .with_help(format!(
                    "First `{name}` parameter is at line {line_number}, column {first_column}. Rename one parameter."
                )),
            );
        }
    }
}

fn check_function_definition(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    function_defs: &mut HashMap<String, FunctionSignature>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !starts_with_keyword(trimmed, "def") {
        return;
    }

    let Some(signature) = parse_function_signature(line, trimmed, line_number, indent) else {
        return;
    };

    if let Some(first) = function_defs.insert(signature.name.clone(), signature.clone()) {
        let name = &signature.name;
        diagnostics.push(
            SourceDiagnostic::error(
                "duplicate-function",
                line_number,
                signature.column,
                format!("Duplicate function definition `{name}`"),
            )
            .with_help(format!(
                "First definition is at line {}, column {}. Rename one function or merge the implementations.",
                first.line, first.column
            )),
        );
    }
}

fn parse_function_signature(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
) -> Option<FunctionSignature> {
    let rest = trimmed.strip_prefix("def")?.trim_start();
    let name = rest
        .chars()
        .take_while(|ch| is_ident_char(*ch))
        .collect::<String>();
    if name.is_empty() {
        return None;
    }

    let open = trimmed.find('(')?;
    let close = trimmed[open + 1..]
        .find(')')
        .map(|offset| open + 1 + offset)?;
    let params = split_top_level_args(&trimmed[open + 1..close])
        .into_iter()
        .filter(|param| !param.trim().is_empty())
        .map(|param| param.trim().to_string())
        .collect::<Vec<_>>();

    Some(FunctionSignature {
        name: name.clone(),
        params,
        line: line_number,
        column: column_from_byte(line, indent + trimmed.find(&name).unwrap_or(0)),
    })
}

fn check_return_statement(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !starts_with_keyword(trimmed, "return") {
        return;
    }

    diagnostics.push(
        SourceDiagnostic::error(
            "unsupported-return",
            line_number,
            column_from_byte(line, indent),
            "Return statements are not supported",
        )
        .with_help(
            "Minecraft functions cannot return early or return values. Use if/else blocks, scoreboard state, or separate functions to structure control flow.",
        ),
    );
}

fn check_compound_assignment(
    line: &str,
    masked: &str,
    line_number: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for operator in ["+=", "-=", "*=", "/=", "%=", "^="] {
        if let Some(index) = masked.find(operator) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-assignment",
                    line_number,
                    column_from_byte(line, index),
                    format!("Compound assignment `{operator}` is not supported"),
                )
                .with_help("Write the assignment explicitly, for example `x = x + value`."),
            );
            return;
        }
    }
}

fn check_assignment_target(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "if")
        || starts_with_keyword(trimmed, "elif")
        || starts_with_keyword(trimmed, "while")
        || starts_with_keyword(trimmed, "for")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "define")
        || trimmed.starts_with('@')
    {
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

    if target.contains('.') || target.contains('[') || target.contains(']') || target.contains(',')
    {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-assignment-target",
                line_number,
                column_from_byte(line, indent),
                "Only simple identifier assignment targets are supported",
            )
            .with_help("Assign to a named variable, then call storage helpers for nested data."),
        );
    }
}

fn check_assignment_function_calls(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "if")
        || starts_with_keyword(trimmed, "elif")
        || starts_with_keyword(trimmed, "while")
        || starts_with_keyword(trimmed, "for")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "define")
        || trimmed.starts_with('@')
    {
        return;
    }

    let Some(equals_index) = single_equals_index(trimmed) else {
        return;
    };
    let rhs = &trimmed[equals_index + 1..];

    for call in function_calls_in_expression(rhs) {
        if let Some(diagnostic) = invalid_math_value_function_call(&call) {
            diagnostics.push(
                SourceDiagnostic::error(
                    diagnostic.kind,
                    line_number,
                    column_from_byte(line, indent + equals_index + 1 + call.offset),
                    diagnostic.message,
                )
                .with_help(diagnostic.help),
            );
            return;
        }

        if is_allowed_value_function_call(&call.name) {
            continue;
        }

        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-function-call-expression",
                line_number,
                column_from_byte(line, indent + equals_index + 1 + call.offset),
                "Function calls in expressions are not supported (except math intrinsics).",
            )
            .with_help("Call Cobble functions as standalone statements, or store results through scoreboard/storage state explicitly."),
        );
        return;
    }
}

fn check_user_function_call_arguments(
    source: &str,
    function_defs: &HashMap<String, FunctionSignature>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if function_defs.is_empty() {
        return;
    }

    let mut active_docstring_quote = None;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if should_skip_call_argument_scan(trimmed) {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        for call in function_calls_in_expression(trimmed) {
            let Some(signature) = function_defs.get(&call.name) else {
                continue;
            };
            if call.arg_count == signature.params.len() {
                for argument in &call.arguments {
                    let Some(nested_call) = function_calls_in_expression(&argument.text)
                        .into_iter()
                        .next()
                    else {
                        continue;
                    };

                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-function-call-argument",
                            line_number,
                            column_from_byte(
                                line,
                                indent + argument.offset + nested_call.offset,
                            ),
                            format!(
                                "Function `{}` arguments cannot contain function call expressions",
                                signature.name
                            ),
                        )
                        .with_help(
                            "Call Cobble functions as standalone statements, or pass a literal, variable, selector, or storage-backed value.",
                        ),
                    );
                }
                continue;
            }

            diagnostics.push(
                SourceDiagnostic::error(
                    "function-argument-count",
                    line_number,
                    column_from_byte(line, indent + call.offset),
                    format!(
                        "Function `{}` expects {} argument(s), but {} provided",
                        signature.name,
                        signature.params.len(),
                        call.arg_count
                    ),
                )
                .with_help(format!(
                    "Expected parameters: ({})",
                    signature.params.join(", ")
                )),
            );
        }
    }
}

fn check_undefined_function_calls(
    source: &str,
    function_defs: &HashMap<String, FunctionSignature>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut active_docstring_quote = None;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if should_skip_call_argument_scan(trimmed) {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        for call in function_calls_in_expression(trimmed) {
            if !is_user_function_call_statement(trimmed, &call) {
                continue;
            }

            if let Some((module, method)) = split_module_call_name(&call.name) {
                if is_value_only_module_call(module, method) {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-function-call-expression",
                            line_number,
                            column_from_byte(line, indent + call.offset),
                            format!(
                                "`{}` returns a value and cannot be used as a standalone statement",
                                call.name
                            ),
                        )
                        .with_help(
                            "Pass the returned value to another helper or JSON resource value instead.",
                        ),
                    );
                    continue;
                }

                if is_known_module_call(module, method) {
                    continue;
                }

                diagnostics.push(
                    SourceDiagnostic::error(
                        "undefined-function",
                        line_number,
                        column_from_byte(line, indent + call.offset),
                        format!("Unknown helper function `{}`", call.name),
                    )
                    .with_help(
                        "Use a documented Cobble helper module/function, define a Cobble function, or use a raw Minecraft command for external behavior.",
                    ),
                );
                continue;
            }

            if function_defs.contains_key(&call.name) || is_allowed_value_function_call(&call.name)
            {
                continue;
            }

            diagnostics.push(
                SourceDiagnostic::error(
                    "undefined-function",
                    line_number,
                    column_from_byte(line, indent + call.offset),
                    format!("Undefined function `{}`", call.name),
                )
                .with_help(
                    "Define the Cobble function, import a file that defines it, or use a raw `/function namespace:path` command for external Minecraft functions.",
                ),
            );
        }
    }
}

fn has_non_stdlib_imports(source: &str) -> bool {
    let mut active_docstring_quote = None;
    for line in source.lines() {
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if let Some(rest) = trimmed.strip_prefix("import ") {
            if rest.split(',').any(|module| {
                module
                    .split_whitespace()
                    .next()
                    .unwrap_or("")
                    .split('.')
                    .next()
                    .is_some_and(|module| module != "stdlib")
            }) {
                return true;
            }
        }

        if let Some(rest) = trimmed.strip_prefix("from ") {
            let module = rest.split_once(" import ").map(|(module, _)| module.trim());
            if module.is_some_and(|module| module != "stdlib") {
                return true;
            }
        }
    }

    false
}

fn is_user_function_call_statement(trimmed: &str, call: &FunctionCallSpan) -> bool {
    if !trimmed[..call.offset].trim().is_empty() {
        return false;
    }

    let Some(open_index) = trimmed[call.offset..]
        .find('(')
        .map(|offset| call.offset + offset)
    else {
        return false;
    };
    let Some(close_index) = matching_close_paren(trimmed, open_index) else {
        return false;
    };

    trimmed[close_index + 1..].trim().is_empty()
}

fn is_known_non_user_function_call(name: &str) -> bool {
    split_module_call_name(name)
        .is_some_and(|(module, method)| is_known_module_call(module, method))
        || is_allowed_value_function_call(name)
}

fn split_module_call_name(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('.')
}

fn is_known_module_call(module: &str, method: &str) -> bool {
    match module {
        "stdlib" => method == "addEventListener",
        "math" => matches!(method, "sqrt" | "abs" | "min" | "max"),
        "text" => matches!(
            method,
            "plain"
                | "colored"
                | "score"
                | "selector"
                | "tellraw"
                | "title"
                | "subtitle"
                | "actionbar"
        ),
        "score" => matches!(
            method,
            "set" | "add" | "remove" | "reset" | "copy" | "operation"
        ),
        "score.objective" => matches!(method, "add" | "remove" | "display"),
        "random" => matches!(method, "int" | "bool"),
        "timer" => matches!(method, "set" | "tick" | "done" | "reset"),
        "storage" => matches!(
            method,
            "set"
                | "merge"
                | "remove"
                | "copy"
                | "append"
                | "prepend"
                | "insert"
                | "get"
                | "read_score"
                | "copy_from"
        ),
        "schedule" => matches!(method, "once" | "clear"),
        "bossbar" => matches!(
            method,
            "add"
                | "remove"
                | "set_value"
                | "set_max"
                | "set_name"
                | "set_color"
                | "set_style"
                | "set_visible"
                | "set_players"
        ),
        "team" => matches!(method, "add" | "remove" | "join" | "leave" | "modify"),
        "entity" => matches!(
            method,
            "tag_add"
                | "tag_remove"
                | "effect_give"
                | "effect_clear"
                | "attribute_get"
                | "attribute_base_set"
        ),
        "datapack" => {
            datapack_json_resource_type(method).is_some() || datapack_tag_type(method).is_some()
        }
        _ => false,
    }
}

fn is_value_only_module_call(module: &str, method: &str) -> bool {
    module == "text" && matches!(method, "plain" | "colored" | "score" | "selector")
}

fn check_unsupported_none_usage(source: &str, diagnostics: &mut Vec<SourceDiagnostic>) {
    let mut active_docstring_quote = None;
    let mut active_datapack_json_expression: Option<MappedSourceExpression> = None;
    let mut datapack_json_depth = 0usize;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if let Some(expression) = active_datapack_json_expression.as_mut() {
            expression.push_line_fragment(line_number, line, &masked, indent);
            update_delimiter_depth_for_indentation(trimmed, &mut datapack_json_depth);
            if datapack_json_depth == 0 {
                if let Some(expression) = active_datapack_json_expression.take() {
                    check_none_usage_in_mapped_expression(&expression, diagnostics);
                }
            }
            continue;
        }

        if starts_datapack_json_resource_call(trimmed) {
            let mut expression = MappedSourceExpression::default();
            expression.push_line_fragment(line_number, line, &masked, indent);
            update_delimiter_depth_for_indentation(trimmed, &mut datapack_json_depth);
            if datapack_json_depth == 0 {
                check_none_usage_in_mapped_expression(&expression, diagnostics);
            } else {
                active_datapack_json_expression = Some(expression);
            }
            continue;
        }

        check_none_usage_in_line(line, trimmed, line_number, indent, diagnostics);
    }
}

fn check_none_usage_in_mapped_expression(
    expression: &MappedSourceExpression,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let allowed_ranges = datapack_json_value_ranges(&expression.masked);
    for token in nullish_token_spans(&expression.masked) {
        if token.name == "None"
            && allowed_ranges
                .iter()
                .any(|range| token.offset >= range.start && token.offset < range.end)
        {
            continue;
        }

        let location = expression.location_at(token.offset);
        push_unsupported_none_diagnostic(diagnostics, location.line, location.column, token.name);
    }
}

fn check_none_usage_in_line(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for token in nullish_token_spans(trimmed) {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-none",
                line_number,
                column_from_byte(line, indent + token.offset),
                "None/null is only supported in data pack JSON resource helper values",
            )
            .with_help(
                "Minecraft SNBT/NBT storage has no null type. Use `None` only as a JSON null in datapack resource helpers, use `None` instead of lowercase `null`, or choose an explicit sentinel value.",
            ),
        );
    }
}

fn push_unsupported_none_diagnostic(
    diagnostics: &mut Vec<SourceDiagnostic>,
    line: usize,
    column: usize,
    _token: &str,
) {
    diagnostics.push(
        SourceDiagnostic::error(
            "unsupported-none",
            line,
            column,
            "None/null is only supported in data pack JSON resource helper values",
        )
        .with_help(
            "Minecraft SNBT/NBT storage has no null type. Use `None` only as a JSON null in datapack resource helpers, use `None` instead of lowercase `null`, or choose an explicit sentinel value.",
        ),
    );
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct NullishTokenSpan {
    name: &'static str,
    offset: usize,
}

fn nullish_token_spans(expression: &str) -> Vec<NullishTokenSpan> {
    let mut spans = Vec::new();
    for name in ["None", "null"] {
        let mut search_from = 0usize;
        while let Some(relative_offset) = expression[search_from..].find(name) {
            let offset = search_from + relative_offset;
            let before = expression[..offset].chars().next_back();
            let after = expression[offset + name.len()..].chars().next();
            if before.is_none_or(|ch| !is_ident_char(ch))
                && after.is_none_or(|ch| !is_ident_char(ch))
            {
                spans.push(NullishTokenSpan { name, offset });
            }
            search_from = offset + name.len();
        }
    }
    spans.sort_by_key(|span| span.offset);
    spans
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceRange {
    start: usize,
    end: usize,
}

fn datapack_json_value_ranges(expression: &str) -> Vec<SourceRange> {
    function_calls_in_expression(expression)
        .into_iter()
        .filter_map(|call| {
            let helper = call.name.strip_prefix("datapack.")?;
            datapack_json_resource_type(helper)?;
            let json_arg = call.arguments.get(1)?;
            Some(SourceRange {
                start: json_arg.offset,
                end: json_arg.offset + json_arg.text.len(),
            })
        })
        .collect()
}

fn starts_datapack_json_resource_call(trimmed: &str) -> bool {
    if function_calls_in_expression(trimmed)
        .into_iter()
        .any(|call| {
            call.name
                .strip_prefix("datapack.")
                .is_some_and(|helper| datapack_json_resource_type(helper).is_some())
        })
    {
        return true;
    }

    let Some(rest) = trimmed.strip_prefix("datapack.") else {
        return false;
    };
    let helper = rest
        .split_once('(')
        .map(|(helper, _)| helper.trim())
        .unwrap_or(rest.trim());
    datapack_json_resource_type(helper).is_some()
}

fn check_storage_backed_access(source: &str, diagnostics: &mut Vec<SourceDiagnostic>) {
    let mut module_types: HashMap<String, CobbleType> = HashMap::new();
    let mut module_constants: HashMap<String, f64> = HashMap::new();
    let mut current_function: Option<DiagnosticFunctionScope> = None;
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if current_function
            .as_ref()
            .is_some_and(|scope| indent <= scope.indent)
        {
            current_function = None;
        }

        if starts_with_keyword(trimmed, "def") {
            current_function = Some(DiagnosticFunctionScope {
                indent,
                types: module_types.clone(),
                constants: module_constants.clone(),
            });
            continue;
        }

        let Some(assignment) = assignment_span_for_type_check(line, trimmed, indent) else {
            continue;
        };

        let (type_env, constant_env) = match current_function.as_mut() {
            Some(scope) => (&mut scope.types, &mut scope.constants),
            None => (&mut module_types, &mut module_constants),
        };

        if let Some(diagnostic) =
            unsupported_storage_access_in_expression(&assignment.value, type_env, constant_env)
        {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-storage-access",
                    line_number,
                    column_from_byte(line, indent + assignment.value_offset + diagnostic.offset),
                    diagnostic.message,
                )
                .with_help(diagnostic.help),
            );
            continue;
        }

        if assignment.is_const {
            if let Some(value) =
                evaluate_numeric_const_for_diagnostics(&assignment.value, constant_env)
            {
                constant_env.insert(assignment.target.clone(), value);
            } else {
                constant_env.remove(&assignment.target);
            }
        } else {
            constant_env.remove(&assignment.target);
        }

        if let Some(new_type) = infer_expression_type_for_diagnostics(&assignment.value, type_env) {
            match type_env.get(&assignment.target) {
                Some(existing_type) if *existing_type != new_type => {}
                _ => {
                    type_env.insert(assignment.target, new_type);
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
struct DiagnosticFunctionScope {
    indent: usize,
    types: HashMap<String, CobbleType>,
    constants: HashMap<String, f64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageAccessDiagnostic {
    offset: usize,
    message: String,
    help: String,
}

fn unsupported_storage_access_in_expression(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
    constant_env: &HashMap<String, f64>,
) -> Option<StorageAccessDiagnostic> {
    let masked = mask_non_code(expression);

    if let Some(diagnostic) = unsupported_subscript_access(&masked, type_env, constant_env) {
        return Some(diagnostic);
    }

    unsupported_attribute_access(&masked, type_env)
}

fn unsupported_subscript_access(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
    constant_env: &HashMap<String, f64>,
) -> Option<StorageAccessDiagnostic> {
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }

        let Some((base, base_start)) = identifier_before(expression, index) else {
            index += 1;
            continue;
        };
        if is_builtin_symbol(base) {
            index += 1;
            continue;
        }

        let Some(close_index) = matching_close_bracket(expression, index) else {
            index += 1;
            continue;
        };
        let index_expression = expression[index + 1..close_index].trim();
        let Some(base_type) = type_env.get(base) else {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!("Cannot resolve storage-backed subscript access `{base}[...]`"),
                help: "Assign a list, map, or string literal to the base variable before using storage-backed access.".to_string(),
            });
        };

        if !matches!(
            base_type,
            CobbleType::List | CobbleType::Map | CobbleType::String
        ) {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!(
                    "Variable `{base}` is type {}, which does not support subscript access",
                    base_type.name()
                ),
                help: "Use subscript access only on storage-backed list, map, or string variables."
                    .to_string(),
            });
        }

        if !is_literal_storage_index(index_expression)
            && !is_constant_storage_index(index_expression, constant_env)
        {
            return Some(StorageAccessDiagnostic {
                offset: index + 1,
                message: "Dynamic storage-backed subscript indexes are not supported".to_string(),
                help: "Use a numeric/string literal index or a numeric compile-time constant such as `items[0]`, `config[\"chance\"]`, or `items[INDEX]`.".to_string(),
            });
        }

        index = close_index + 1;
    }

    None
}

fn unsupported_attribute_access(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
) -> Option<StorageAccessDiagnostic> {
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'.' {
            index += 1;
            continue;
        }

        let Some((base, base_start)) = identifier_before(expression, index) else {
            index += 1;
            continue;
        };
        let Some((_field, _field_end)) = identifier_after(expression, index + 1) else {
            index += 1;
            continue;
        };
        if is_builtin_symbol(base) {
            index += 1;
            continue;
        }

        let Some(base_type) = type_env.get(base) else {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!("Cannot resolve storage-backed attribute access `{base}.`"),
                help: "Assign a map literal to the base variable before using storage-backed attribute access.".to_string(),
            });
        };

        if *base_type != CobbleType::Map {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!(
                    "Variable `{base}` is type {}, which does not support attribute access",
                    base_type.name()
                ),
                help: "Use attribute access only on storage-backed map variables.".to_string(),
            });
        }

        index += 1;
    }

    None
}

fn identifier_before(text: &str, before: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut end = before;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_continue_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start == end || !is_ident_start_byte(bytes[start]) {
        return None;
    }
    Some((&text[start..end], start))
}

fn identifier_after(text: &str, after: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut start = after;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() || !is_ident_start_byte(bytes[start]) {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() && is_ident_continue_byte(bytes[end]) {
        end += 1;
    }
    Some((&text[start..end], end))
}

fn matching_close_bracket(expression: &str, open_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn is_literal_storage_index(index_expression: &str) -> bool {
    let index_expression = index_expression.trim();
    is_numeric_literal(index_expression)
        || (index_expression.len() >= 2
            && matches!(index_expression.as_bytes()[0], b'"' | b'\'')
            && index_expression
                .as_bytes()
                .last()
                .is_some_and(|last| *last == index_expression.as_bytes()[0]))
}

fn is_constant_storage_index(index_expression: &str, constant_env: &HashMap<String, f64>) -> bool {
    let index_expression = index_expression.trim();
    is_simple_module_name(index_expression) && constant_env.contains_key(index_expression)
}

fn evaluate_numeric_const_for_diagnostics(
    expression: &str,
    constant_env: &HashMap<String, f64>,
) -> Option<f64> {
    let expression = strip_balanced_outer_parens(expression.trim());
    if expression.is_empty() {
        return None;
    }

    if let Ok(value) = expression.parse::<f64>() {
        return Some(value);
    }
    if is_simple_module_name(expression) {
        return constant_env.get(expression).copied();
    }

    if let Some(rest) = expression.strip_prefix('+') {
        return evaluate_numeric_const_for_diagnostics(rest, constant_env);
    }
    if let Some(rest) = expression.strip_prefix('-') {
        return evaluate_numeric_const_for_diagnostics(rest, constant_env).map(|value| -value);
    }

    let operator_groups: [&[&str]; 3] = [&["+", "-"], &["*", "/", "%"], &["^"]];
    for operators in operator_groups {
        if let Some((left, operator, right)) =
            split_top_level_binary_operator(expression, operators)
        {
            let left = evaluate_numeric_const_for_diagnostics(left, constant_env)?;
            let right = evaluate_numeric_const_for_diagnostics(right, constant_env)?;
            return match operator {
                "+" => Some(left + right),
                "-" => Some(left - right),
                "*" => Some(left * right),
                "/" if right != 0.0 => Some(left / right),
                "%" if right != 0.0 => Some(((left as i32) % (right as i32)) as f64),
                "^" => Some((left as i32).checked_pow(right as u32)? as f64),
                _ => None,
            };
        }
    }

    None
}

fn strip_balanced_outer_parens(expression: &str) -> &str {
    let mut expression = expression.trim();
    loop {
        if !expression.starts_with('(') || !expression.ends_with(')') {
            return expression;
        }
        let Some(close_index) = matching_close_paren(expression, 0) else {
            return expression;
        };
        if close_index != expression.len() - 1 {
            return expression;
        }
        expression = expression[1..expression.len() - 1].trim();
    }
}

fn split_top_level_binary_operator<'a>(
    expression: &'a str,
    operators: &[&'static str],
) -> Option<(&'a str, &'static str, &'a str)> {
    let bytes = expression.as_bytes();
    let mut delimiter_depth = 0usize;

    for index in (0..bytes.len()).rev() {
        match bytes[index] {
            b')' | b']' | b'}' => {
                delimiter_depth += 1;
                continue;
            }
            b'(' | b'[' | b'{' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if delimiter_depth != 0 {
            continue;
        }

        for operator in operators {
            if !expression[index..].starts_with(operator) {
                continue;
            }
            if matches!(*operator, "+" | "-") && is_unary_sign(expression, index) {
                continue;
            }
            let right_start = index + operator.len();
            if expression[..index].trim().is_empty() || expression[right_start..].trim().is_empty()
            {
                continue;
            }
            return Some((&expression[..index], *operator, &expression[right_start..]));
        }
    }

    None
}

fn is_unary_sign(expression: &str, index: usize) -> bool {
    expression[..index]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_none_or(|ch| matches!(ch, '(' | '[' | '{' | '+' | '-' | '*' | '/' | '%' | '^'))
}

fn check_type_mismatches(source: &str, diagnostics: &mut Vec<SourceDiagnostic>) {
    let mut module_types: HashMap<String, CobbleType> = HashMap::new();
    let mut current_function: Option<(usize, HashMap<String, CobbleType>)> = None;
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if current_function
            .as_ref()
            .is_some_and(|(function_indent, _)| indent <= *function_indent)
        {
            current_function = None;
        }

        if starts_with_keyword(trimmed, "def") {
            current_function = Some((indent, module_types.clone()));
            continue;
        }

        let Some(assignment) = assignment_span_for_type_check(line, trimmed, indent) else {
            continue;
        };

        let type_env = current_function
            .as_mut()
            .map(|(_, types)| types)
            .unwrap_or(&mut module_types);
        let new_type = infer_expression_type_for_diagnostics(&assignment.value, type_env);
        let Some(new_type) = new_type else {
            continue;
        };

        if let Some(existing_type) = type_env.get(&assignment.target) {
            if *existing_type != new_type {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "type-mismatch",
                        line_number,
                        assignment.column,
                        format!("Type mismatch for variable '{}'.", assignment.target),
                    )
                    .with_help(format!(
                        "Variable was previously defined as type: {}\nCannot reassign to type: {}\nUse a different variable name or keep all assignments to '{}' the same type.",
                        existing_type.name(),
                        new_type.name(),
                        assignment.target
                    )),
                );
                continue;
            }
        }

        type_env.insert(assignment.target, new_type);
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeAssignmentSpan {
    target: String,
    value: String,
    value_offset: usize,
    column: usize,
    is_const: bool,
}

fn assignment_span_for_type_check(
    line: &str,
    masked_trimmed: &str,
    indent: usize,
) -> Option<TypeAssignmentSpan> {
    if should_skip_assignment_symbol_scan(masked_trimmed) {
        return None;
    }

    let equals_index = single_equals_index(masked_trimmed)?;
    let raw_target = masked_trimmed[..equals_index].trim();
    let is_const = raw_target.starts_with("const ");
    let target = raw_target
        .strip_prefix("const ")
        .unwrap_or(raw_target)
        .trim();
    if !is_simple_module_name(target) {
        return None;
    }

    let raw_trimmed = line.trim_start();
    let raw_rhs = raw_trimmed.get(equals_index + 1..)?;
    let stripped_rhs = strip_comment_preserving_strings(raw_rhs);
    let trim_start = stripped_rhs.len() - stripped_rhs.trim_start().len();
    let value = stripped_rhs.trim().to_string();
    if value.is_empty() {
        return None;
    }

    Some(TypeAssignmentSpan {
        target: target.to_string(),
        value,
        value_offset: equals_index + 1 + trim_start,
        column: column_from_byte(line, indent + raw_target.find(target).unwrap_or(0)),
        is_const,
    })
}

fn infer_expression_type_for_diagnostics(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
) -> Option<CobbleType> {
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }

    if expression.starts_with('"') || expression.starts_with('\'') {
        return Some(CobbleType::String);
    }
    if expression == "True" || expression == "False" {
        return Some(CobbleType::Boolean);
    }
    if expression.starts_with('[') {
        return Some(CobbleType::List);
    }
    if expression.starts_with('{') {
        return Some(CobbleType::Map);
    }
    let function_calls = function_calls_in_expression(expression);
    if expression.starts_with("math.") && function_calls.len() == 1 {
        let call = &function_calls[0];
        if math_value_function_arity(&call.name).is_some_and(|arity| arity == call.arg_count)
            && expression_references_are_known(expression, type_env)
        {
            return Some(CobbleType::Integer);
        }
    }
    if is_numeric_literal(expression) {
        return Some(CobbleType::Integer);
    }
    if is_simple_module_name(expression) {
        return type_env.get(expression).cloned();
    }
    if expression_contains_boolean_operator(expression)
        && expression_references_are_known(expression, type_env)
    {
        return Some(CobbleType::Boolean);
    }
    if expression_contains_arithmetic_operator(expression)
        && expression_references_are_known(expression, type_env)
    {
        return Some(CobbleType::Integer);
    }

    None
}

fn expression_references_are_known(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
) -> bool {
    identifier_references_in_expression(expression)
        .into_iter()
        .all(|reference| {
            is_builtin_symbol(&reference.name) || type_env.contains_key(&reference.name)
        })
}

fn strip_comment_preserving_strings(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut quote = None;
    let mut escaped = false;

    for ch in text.chars() {
        if let Some(active_quote) = quote {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                output.push(ch);
            }
            '#' => break,
            _ => output.push(ch),
        }
    }

    output
}

fn is_numeric_literal(expression: &str) -> bool {
    let expression = expression.trim();
    if expression.is_empty() {
        return false;
    }

    let digits = expression.strip_prefix('-').unwrap_or(expression);
    digits.parse::<f64>().is_ok()
}

fn expression_contains_boolean_operator(expression: &str) -> bool {
    contains_top_level_operator(expression, &["==", "!=", "<=", ">=", "<", ">"])
        || find_word(expression, "and").is_some()
        || find_word(expression, "or").is_some()
        || expression
            .trim_start()
            .strip_prefix("not ")
            .is_some_and(|rest| !rest.trim().is_empty())
}

fn expression_contains_arithmetic_operator(expression: &str) -> bool {
    contains_top_level_operator(expression, &["+", "-", "*", "/", "%", "^"])
        || expression
            .trim_start()
            .strip_prefix('-')
            .is_some_and(|rest| !rest.trim().is_empty())
        || expression
            .trim_start()
            .strip_prefix('+')
            .is_some_and(|rest| !rest.trim().is_empty())
}

fn contains_top_level_operator(expression: &str, operators: &[&str]) -> bool {
    let bytes = expression.as_bytes();
    let mut delimiter_depth = 0usize;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => {
                delimiter_depth += 1;
                index += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                index += 1;
                continue;
            }
            _ => {}
        }

        if delimiter_depth == 0 {
            for operator in operators {
                if expression[index..].starts_with(operator) {
                    if *operator == "-" || *operator == "+" {
                        let previous = previous_non_whitespace_byte(expression, index);
                        if previous.is_none_or(|byte| {
                            matches!(
                                byte,
                                b'=' | b'!'
                                    | b'<'
                                    | b'>'
                                    | b'+'
                                    | b'-'
                                    | b'*'
                                    | b'/'
                                    | b'%'
                                    | b'^'
                                    | b'('
                                    | b'['
                                    | b'{'
                            )
                        }) {
                            continue;
                        }
                    }
                    return true;
                }
            }
        }

        index += 1;
    }

    false
}

#[derive(Debug, Clone, Default)]
struct DefinedSymbols {
    names: HashSet<String>,
}

impl DefinedSymbols {
    fn contains(&self, name: &str) -> bool {
        self.names.contains(name) || is_builtin_symbol(name)
    }

    fn insert(&mut self, name: impl Into<String>) {
        self.names.insert(name.into());
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpressionSpan {
    text: String,
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentifierReference {
    name: String,
    offset: usize,
}

fn collect_defined_symbols(
    source: &str,
    function_defs: &HashMap<String, FunctionSignature>,
) -> DefinedSymbols {
    let mut symbols = DefinedSymbols::default();

    for signature in function_defs.values() {
        symbols.insert(signature.name.clone());
    }

    let mut active_docstring_quote = None;
    for line in source.lines() {
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        collect_import_symbols(trimmed, &mut symbols);
        collect_assignment_symbol(trimmed, &mut symbols);
        collect_global_symbols(trimmed, &mut symbols);
        collect_for_target_symbol(trimmed, &mut symbols);
    }

    symbols
}

fn collect_interpolation_symbols(source: &str) -> DefinedSymbols {
    let mut symbols = DefinedSymbols::default();
    let mut active_docstring_quote = None;

    for line in source.lines() {
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        collect_interpolation_import_symbols(trimmed, &mut symbols);
    }

    symbols
}

fn check_raw_command_placeholders(
    source: &str,
    symbols: &DefinedSymbols,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut available_symbols = symbols.clone();
    let mut current_function: Option<(usize, HashSet<String>)> = None;
    let mut loop_scopes: Vec<(usize, String)> = Vec::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if current_function
            .as_ref()
            .is_some_and(|(function_indent, _)| indent <= *function_indent)
        {
            current_function = None;
        }

        while loop_scopes
            .last()
            .is_some_and(|(loop_indent, _)| indent <= *loop_indent)
        {
            loop_scopes.pop();
        }

        if starts_with_keyword(trimmed, "def") {
            if let Some(signature) = parse_function_signature(line, trimmed, line_number, indent) {
                current_function =
                    Some((indent, signature.params.into_iter().collect::<HashSet<_>>()));
            }
            continue;
        }

        if starts_with_keyword(trimmed, "for") {
            if let Some(target) = for_loop_target(trimmed) {
                loop_scopes.push((indent, target));
            }
        }

        if !trimmed.starts_with('/') {
            if let Some(name) = assignment_symbol_name(trimmed) {
                if let Some((_function_indent, local_names)) = current_function.as_mut() {
                    local_names.insert(name);
                } else {
                    available_symbols.insert(name);
                }
            }

            for name in global_symbol_names(trimmed) {
                if let Some((_function_indent, local_names)) = current_function.as_mut() {
                    local_names.insert(name);
                } else {
                    available_symbols.insert(name);
                }
            }
            continue;
        }

        let local_names = active_local_names(&current_function, &loop_scopes);
        for placeholder in raw_command_placeholders(line, indent) {
            if placeholder.name.is_empty() {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "unclosed-placeholder",
                        line_number,
                        placeholder.column,
                        "Command placeholder or brace expression is not closed",
                    )
                    .with_help(
                        "Close the brace expression, or write `{{name}}` for literal braces.",
                    ),
                );
                continue;
            }

            if !is_simple_module_name(&placeholder.name) {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "invalid-placeholder",
                        line_number,
                        placeholder.column,
                        format!("Invalid command placeholder `{}`", placeholder.name),
                    )
                    .with_help(
                        "Use identifier placeholders such as `{player_name}`, or write doubled braces like `{{literal}}` for literal text.",
                    ),
                );
                continue;
            }

            if available_symbols.names.contains(&placeholder.name)
                || local_names.contains(&placeholder.name)
            {
                continue;
            }

            diagnostics.push(
                SourceDiagnostic::error(
                    "undefined-placeholder",
                    line_number,
                    placeholder.column,
                    format!("Undefined command placeholder `{}`", placeholder.name),
                )
                .with_help(format!(
                    "Define `{}` before using it, pass it as a function parameter, or write `{{{{{}}}}}` for literal braces.",
                    placeholder.name, placeholder.name
                )),
            );
        }
    }
}

fn raw_command_placeholder_locations(source: &str, name: &str) -> Vec<(usize, usize)> {
    let mut locations = Vec::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if !trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        for placeholder in raw_command_placeholders(line, indent) {
            if placeholder.name == name {
                locations.push((line_number, placeholder.column));
            }
        }
    }

    locations
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandPlaceholder {
    name: String,
    column: usize,
}

fn raw_command_placeholders(line: &str, indent: usize) -> Vec<CommandPlaceholder> {
    let mut placeholders = Vec::new();
    let mut index = indent;
    let mut quote = None;
    let mut escaped = false;

    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                index = next_index;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                index = next_index;
                continue;
            }
            if ch == '{' {
                if line[index..].starts_with("{{") {
                    if let Some(close_index) = find_escaped_brace_end(line, index + 2) {
                        index = close_index + 2;
                    } else {
                        index += 2;
                    }
                    continue;
                }
                if let Some(close_index) = placeholder_close_before_quote(line, index, active_quote)
                {
                    let content = line[index + 1..close_index].trim();
                    if is_potential_command_placeholder(content) {
                        placeholders.push(CommandPlaceholder {
                            name: content.to_string(),
                            column: column_from_byte(line, index),
                        });
                        index = close_index + 1;
                        continue;
                    }
                }
            }
            if ch == active_quote {
                quote = None;
            }
            index = next_index;
            continue;
        }

        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            escaped = false;
            index = next_index;
            continue;
        }

        if ch != '{' {
            index = next_index;
            continue;
        }

        if line[index..].starts_with("{{") {
            if let Some(close_index) = find_escaped_brace_end(line, index + 2) {
                index = close_index + 2;
            } else {
                index += 2;
            }
            continue;
        }

        let Some(close_index) = matching_brace_quote_aware(line, index) else {
            placeholders.push(CommandPlaceholder {
                name: String::new(),
                column: column_from_byte(line, index),
            });
            index = next_index;
            continue;
        };

        let content = line[index + 1..close_index].trim();
        if is_potential_command_placeholder(content) {
            placeholders.push(CommandPlaceholder {
                name: content.to_string(),
                column: column_from_byte(line, index),
            });
            index = close_index + 1;
        } else {
            index = next_index;
        }
    }

    placeholders
}

fn is_potential_command_placeholder(content: &str) -> bool {
    !content.is_empty()
        && !content.contains(':')
        && !content.contains(',')
        && !content.contains('{')
        && !content.contains('}')
        && !content.contains('"')
        && !content.contains('\'')
}

fn matching_brace_quote_aware(text: &str, open_index: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = open_index;
    let mut quote = None;
    let mut escaped = false;

    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            index = next_index;
            continue;
        }

        match ch {
            '"' | '\'' => quote = Some(ch),
            '{' => depth += 1,
            '}' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index = next_index;
    }

    None
}

fn placeholder_close_before_quote(text: &str, open_index: usize, quote: char) -> Option<usize> {
    let mut index = open_index + 1;
    let mut escaped = false;

    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return None;
        } else if ch == '}' {
            return Some(index);
        }
        index = next_index;
    }

    None
}

fn find_escaped_brace_end(text: &str, start: usize) -> Option<usize> {
    text.get(start..)?.find("}}").map(|offset| start + offset)
}

fn collect_import_symbols(trimmed: &str, symbols: &mut DefinedSymbols) {
    if let Some(rest) = trimmed.strip_prefix("import ") {
        for module in rest.split(',') {
            let module = module
                .split_whitespace()
                .next()
                .unwrap_or("")
                .split('.')
                .next()
                .unwrap_or("");
            if is_simple_module_name(module) {
                symbols.insert(module.to_string());
            }
        }
        return;
    }

    let Some(rest) = trimmed.strip_prefix("from ") else {
        return;
    };
    let Some((_module, items)) = rest.split_once(" import ") else {
        return;
    };
    for item in items.trim_end_matches(':').split(',') {
        let item = item
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('.')
            .next()
            .unwrap_or("");
        if is_simple_module_name(item) {
            symbols.insert(item.to_string());
        }
    }
}

fn collect_interpolation_import_symbols(trimmed: &str, symbols: &mut DefinedSymbols) {
    let Some(rest) = trimmed.strip_prefix("from ") else {
        return;
    };
    let Some((_module, items)) = rest.split_once(" import ") else {
        return;
    };
    for item in items.trim_end_matches(':').split(',') {
        let item = item
            .split_whitespace()
            .next()
            .unwrap_or("")
            .split('.')
            .next()
            .unwrap_or("");
        if is_simple_module_name(item) {
            symbols.insert(item.to_string());
        }
    }
}

fn collect_assignment_symbol(trimmed: &str, symbols: &mut DefinedSymbols) {
    if let Some(name) = assignment_symbol_name(trimmed) {
        symbols.insert(name);
    }
}

fn assignment_symbol_name(trimmed: &str) -> Option<String> {
    if should_skip_assignment_symbol_scan(trimmed) {
        return None;
    }

    let equals_index = single_equals_index(trimmed)?;
    let raw_target = trimmed[..equals_index].trim();
    let target = raw_target
        .strip_prefix("const ")
        .unwrap_or(raw_target)
        .trim();
    if is_simple_module_name(target) {
        Some(target.to_string())
    } else {
        None
    }
}

fn should_skip_assignment_symbol_scan(trimmed: &str) -> bool {
    starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "if")
        || starts_with_keyword(trimmed, "elif")
        || starts_with_keyword(trimmed, "while")
        || starts_with_keyword(trimmed, "for")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "define")
        || starts_with_keyword(trimmed, "create")
        || starts_with_keyword(trimmed, "end")
        || trimmed.starts_with('@')
}

fn collect_global_symbols(trimmed: &str, symbols: &mut DefinedSymbols) {
    for name in global_symbol_names(trimmed) {
        symbols.insert(name);
    }
}

fn global_symbol_names(trimmed: &str) -> Vec<String> {
    let Some(rest) = trimmed.strip_prefix("global ") else {
        return Vec::new();
    };
    rest.trim_end_matches(':')
        .split(',')
        .map(str::trim)
        .filter(|name| is_simple_module_name(name))
        .map(ToOwned::to_owned)
        .collect()
}

fn collect_for_target_symbol(trimmed: &str, symbols: &mut DefinedSymbols) {
    if !starts_with_keyword(trimmed, "for") {
        return;
    }
    let rest = trimmed["for".len()..].trim_start();
    let Some((target, _iter)) = rest.split_once(" in ") else {
        return;
    };
    let target = target.trim();
    if is_simple_module_name(target) {
        symbols.insert(target.to_string());
    }
}

fn check_undefined_variable_references(
    source: &str,
    symbols: &DefinedSymbols,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut current_function: Option<(usize, HashSet<String>)> = None;
    let mut loop_scopes: Vec<(usize, String)> = Vec::new();
    let mut reported = HashSet::new();
    let mut active_docstring_quote = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }

        let indent = masked.len() - trimmed.len();
        if current_function
            .as_ref()
            .is_some_and(|(function_indent, _)| indent <= *function_indent)
        {
            current_function = None;
        }

        while loop_scopes
            .last()
            .is_some_and(|(loop_indent, _)| indent <= *loop_indent)
        {
            loop_scopes.pop();
        }

        if starts_with_keyword(trimmed, "def") {
            if let Some(signature) = parse_function_signature(line, trimmed, line_number, indent) {
                current_function =
                    Some((indent, signature.params.into_iter().collect::<HashSet<_>>()));
            }
            continue;
        }

        if starts_with_keyword(trimmed, "for") {
            if let Some(target) = for_loop_target(trimmed) {
                loop_scopes.push((indent, target));
            }
        }

        let local_names = active_local_names(&current_function, &loop_scopes);
        for expression in expression_spans_for_undefined_scan(trimmed) {
            if expression_has_unsupported_reference_shape(&expression.text) {
                continue;
            }

            for reference in identifier_references_in_expression(&expression.text) {
                if symbols.contains(&reference.name) || local_names.contains(&reference.name) {
                    continue;
                }

                if !reported.insert((line_number, reference.name.clone())) {
                    continue;
                }

                diagnostics.push(
                    SourceDiagnostic::error(
                        "undefined-variable",
                        line_number,
                        column_from_byte(line, indent + expression.offset + reference.offset),
                        format!("Undefined variable `{}`", reference.name),
                    )
                    .with_help(format!(
                        "Define or import `{}`, or pass it as a function parameter.",
                        reference.name
                    )),
                );
            }
        }
    }
}

fn active_local_names(
    current_function: &Option<(usize, HashSet<String>)>,
    loop_scopes: &[(usize, String)],
) -> HashSet<String> {
    let mut names = current_function
        .as_ref()
        .map(|(_, params)| params.clone())
        .unwrap_or_default();
    for (_indent, target) in loop_scopes {
        names.insert(target.clone());
    }
    names
}

fn expression_spans_for_undefined_scan(trimmed: &str) -> Vec<ExpressionSpan> {
    if should_skip_expression_reference_scan(trimmed) {
        return Vec::new();
    }

    let mut expressions = Vec::new();
    if let Some(equals_index) = single_equals_index(trimmed) {
        let rhs = &trimmed[equals_index + 1..];
        let trim_start = rhs.len() - rhs.trim_start().len();
        let expression = rhs.trim();
        if !expression.is_empty() {
            expressions.push(ExpressionSpan {
                text: expression.to_string(),
                offset: equals_index + 1 + trim_start,
            });
        }
        return expressions;
    }

    for keyword in ["if", "elif", "while", "match"] {
        if starts_with_keyword(trimmed, keyword) {
            let rest_offset = keyword.len();
            let rest = trimmed[rest_offset..].trim_start();
            let trim_start = trimmed[rest_offset..].len() - rest.len();
            let expression = rest.trim_end_matches(':').trim_end();
            if !expression.is_empty() {
                expressions.push(ExpressionSpan {
                    text: expression.to_string(),
                    offset: rest_offset + trim_start,
                });
            }
            return expressions;
        }
    }

    if starts_with_keyword(trimmed, "for") {
        let rest_offset = "for".len();
        let rest = trimmed[rest_offset..].trim_start();
        let rest_trim_start = trimmed[rest_offset..].len() - rest.len();
        let Some((target, iter_and_step)) = rest.split_once(" in ") else {
            return expressions;
        };
        let iter_offset = rest_offset + rest_trim_start + target.len() + " in ".len();
        let iter_and_step = iter_and_step.trim_end_matches(':').trim_end();
        if let Some((iter, step)) = iter_and_step.split_once(" by ") {
            let iter = iter.trim();
            if !iter.is_empty() {
                expressions.push(ExpressionSpan {
                    text: iter.to_string(),
                    offset: iter_offset,
                });
            }
            let step_trim_start = step.len() - step.trim_start().len();
            let step = step.trim();
            if !step.is_empty() {
                expressions.push(ExpressionSpan {
                    text: step.to_string(),
                    offset: iter_offset + iter.len() + " by ".len() + step_trim_start,
                });
            }
        } else if !iter_and_step.is_empty() {
            expressions.push(ExpressionSpan {
                text: iter_and_step.to_string(),
                offset: iter_offset,
            });
        }
    }

    if expressions.is_empty() {
        for call in function_calls_in_expression(trimmed) {
            for argument in call.arguments {
                if !should_scan_call_argument_references(&argument.text)
                    || !function_calls_in_expression(&argument.text).is_empty()
                {
                    continue;
                }

                expressions.push(ExpressionSpan {
                    text: argument.text,
                    offset: argument.offset,
                });
            }
        }
    }

    expressions
}

fn should_scan_call_argument_references(argument: &str) -> bool {
    let trimmed = argument.trim();
    if trimmed.is_empty() {
        return false;
    }

    !matches!(trimmed.chars().next(), Some('[' | '{'))
}

fn should_skip_expression_reference_scan(trimmed: &str) -> bool {
    starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "define")
        || starts_with_keyword(trimmed, "create")
        || starts_with_keyword(trimmed, "end")
        || trimmed.starts_with('@')
}

fn for_loop_target(trimmed: &str) -> Option<String> {
    if !starts_with_keyword(trimmed, "for") {
        return None;
    }
    let rest = trimmed["for".len()..].trim_start();
    let (target, _iter) = rest.split_once(" in ")?;
    let target = target.trim();
    if is_simple_module_name(target) {
        Some(target.to_string())
    } else {
        None
    }
}

fn identifier_references_in_expression(expression: &str) -> Vec<IdentifierReference> {
    let bytes = expression.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        if !is_ident_start_byte(bytes[index]) {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < bytes.len() && is_ident_continue_byte(bytes[index]) {
            index += 1;
        }

        let name = &expression[start..index];
        if should_skip_identifier_reference(expression, start, index, name) {
            continue;
        }

        refs.push(IdentifierReference {
            name: name.to_string(),
            offset: start,
        });
    }

    refs
}

fn expression_has_unsupported_reference_shape(expression: &str) -> bool {
    if expression.contains('[') || expression.contains(']') {
        return true;
    }

    let bytes = expression.as_bytes();
    let mut index = 0;
    while let Some(dot_offset) = expression[index..].find('.') {
        let dot = index + dot_offset;
        if identifier_around_dot(expression, dot).is_some_and(|base| base != "math") {
            return true;
        }
        index = dot + 1;
        if index >= bytes.len() {
            break;
        }
    }

    false
}

fn identifier_around_dot(expression: &str, dot: usize) -> Option<&str> {
    let bytes = expression.as_bytes();
    let mut left_start = dot;
    while left_start > 0 && is_ident_continue_byte(bytes[left_start - 1]) {
        left_start -= 1;
    }
    let mut right_end = dot + 1;
    while right_end < bytes.len() && is_ident_continue_byte(bytes[right_end]) {
        right_end += 1;
    }

    if left_start == dot || right_end == dot + 1 {
        return None;
    }
    Some(&expression[left_start..dot])
}

fn should_skip_identifier_reference(
    expression: &str,
    start: usize,
    end: usize,
    name: &str,
) -> bool {
    if name.chars().all(|ch| ch == '_') {
        return true;
    }

    if is_builtin_symbol(name) || is_expression_keyword(name) {
        return true;
    }

    let previous = previous_non_whitespace_byte(expression, start);
    let next = next_non_whitespace_byte(expression, end);

    matches!(previous, Some(b'.' | b'@'))
        || matches!(next, Some(b'(' | b':'))
        || name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn previous_non_whitespace_byte(text: &str, before: usize) -> Option<u8> {
    text.as_bytes()
        .get(..before)?
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn next_non_whitespace_byte(text: &str, after: usize) -> Option<u8> {
    text.as_bytes()
        .get(after..)?
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn is_ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

fn is_ident_continue_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_'
}

fn is_expression_keyword(name: &str) -> bool {
    matches!(name, "and" | "or" | "not" | "in" | "by" | "to")
}

fn is_builtin_symbol(name: &str) -> bool {
    matches!(
        name,
        "True"
            | "False"
            | "None"
            | "range"
            | "math"
            | "datapack"
            | "stdlib"
            | "event"
            | "score"
            | "text"
            | "random"
            | "timer"
            | "storage"
            | "schedule"
            | "bossbar"
            | "team"
            | "entity"
    )
}

fn should_skip_call_argument_scan(trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('@')
        || starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "define")
        || starts_with_keyword(trimmed, "create")
        || starts_with_keyword(trimmed, "end")
}

fn check_comprehension(
    line: &str,
    masked: &str,
    line_number: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for (open, close) in [('[', ']'), ('{', '}')] {
        if let Some(index) = comprehension_for_index(masked, open, close) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-comprehension",
                    line_number,
                    column_from_byte(line, index),
                    "List and dict comprehensions are not supported",
                )
                .with_help("Use explicit loops or storage helpers."),
            );
            return;
        }
    }
}

fn comprehension_for_index(masked: &str, open: char, close: char) -> Option<usize> {
    let mut search_from = 0;
    while let Some(open_offset) = masked[search_from..].find(open) {
        let open_index = search_from + open_offset;
        let close_offset = masked[open_index + 1..].find(close)?;
        let close_index = open_index + 1 + close_offset;
        let body = &masked[open_index + 1..close_index];
        if let Some(for_index) = find_word(body, "for") {
            if find_word(&body[for_index + "for".len()..], "in").is_some() {
                return Some(open_index + 1 + for_index);
            }
        }
        search_from = close_index + 1;
    }

    None
}

fn check_datapack_helper_argument_shapes(
    line: &str,
    trimmed: &str,
    raw_trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !trimmed.contains("datapack.") {
        return;
    }

    for call in function_calls_in_expression(trimmed) {
        let Some(helper) = call.name.strip_prefix("datapack.") else {
            continue;
        };

        if let Some(resource_type) = datapack_json_resource_type(helper) {
            if call.arg_count != 2 {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line_number,
                        column_from_byte(line, indent + call.offset),
                        format!("datapack.{resource_type}() takes 2 arguments"),
                    )
                    .with_help("Pass a resource name and an object JSON value."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    line,
                    raw_trimmed,
                    name_arg,
                    "resource name",
                    line_number,
                    indent,
                    diagnostics,
                );
            }

            let Some(json_arg) = call.arguments.get(1) else {
                continue;
            };
            if !json_arg.text.trim_start().starts_with('{') {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line_number,
                        column_from_byte(line, indent + json_arg.offset),
                        format!("datapack.{resource_type}() JSON value must be an object"),
                    )
                    .with_help("Use an object literal such as `{ \"condition\": \"...\" }`."),
                );
            }
            continue;
        }

        if let Some(tag_type) = datapack_tag_type(helper) {
            if call.arg_count != 2 {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line_number,
                        column_from_byte(line, indent + call.offset),
                        format!("datapack.{tag_type}_tag() takes 2 arguments"),
                    )
                    .with_help("Pass a tag name and an array of resource IDs."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    line,
                    raw_trimmed,
                    name_arg,
                    "tag name",
                    line_number,
                    indent,
                    diagnostics,
                );
            }

            let Some(values_arg) = call.arguments.get(1) else {
                continue;
            };
            if !values_arg.text.trim_start().starts_with('[') {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line_number,
                        column_from_byte(line, indent + values_arg.offset),
                        "Tag values must be an array",
                    )
                    .with_help("Use an array literal such as `[\"minecraft:stone\"]`."),
                );
            } else {
                check_datapack_tag_value_resource_ids(
                    line,
                    raw_trimmed,
                    values_arg,
                    line_number,
                    indent,
                    diagnostics,
                );
            }
        }
    }
}

fn check_datapack_tag_value_resource_ids(
    line: &str,
    raw_trimmed: &str,
    argument: &FunctionCallArgumentSpan,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let raw_argument = raw_trimmed
        .get(argument.offset..argument.offset + argument.text.len())
        .unwrap_or(&argument.text);

    for item in array_literal_item_spans(raw_argument) {
        if !item.text.starts_with('"') && !item.text.starts_with('\'') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "datapack-resource-argument",
                    line_number,
                    column_from_byte(line, indent + argument.offset + item.offset),
                    "Tag values must be string resource IDs",
                )
                .with_help("Use string values such as `[\"minecraft:stone\"]`."),
            );
            continue;
        }

        let Some(value) = quoted_string_literal_at(raw_argument, item.offset) else {
            continue;
        };
        let Some(diagnostic) = validate_datapack_resource_id("tag value", value) else {
            continue;
        };
        diagnostics.push(
            SourceDiagnostic::error(
                "datapack-resource-id",
                line_number,
                column_from_byte(
                    line,
                    indent + argument.offset + item.offset + 1 + diagnostic.offset,
                ),
                diagnostic.message,
            )
            .with_help(diagnostic.help),
        );
    }
}

fn check_datapack_resource_id_argument(
    line: &str,
    raw_trimmed: &str,
    argument: &FunctionCallArgumentSpan,
    label: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let Some(value) = quoted_string_literal_at(raw_trimmed, argument.offset) else {
        return;
    };
    let Some(diagnostic) = validate_datapack_resource_id(label, value) else {
        return;
    };

    diagnostics.push(
        SourceDiagnostic::error(
            "datapack-resource-id",
            line_number,
            column_from_byte(line, indent + argument.offset + 1 + diagnostic.offset),
            diagnostic.message,
        )
        .with_help(diagnostic.help),
    );
}

struct ResourceIdDiagnostic {
    message: String,
    help: String,
    offset: usize,
}

fn validate_datapack_resource_id(label: &str, id: &str) -> Option<ResourceIdDiagnostic> {
    let (namespace, path, path_offset) = if let Some((namespace, path)) = id.split_once(':') {
        if let Some(extra_colon) = path.find(':') {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': resource IDs may contain at most one ':' separator"
                ),
                help: "Use one optional namespace separator, for example `minecraft:load`."
                    .to_string(),
                offset: namespace.len() + 1 + extra_colon,
            });
        }
        (Some(namespace), path, namespace.len() + 1)
    } else {
        (None, id, 0)
    };

    if let Some(namespace) = namespace {
        let invalid_namespace = namespace.is_empty()
            || namespace.chars().any(|c| {
                !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.')
            });
        if invalid_namespace {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': use lowercase namespace:path resource IDs"
                ),
                help: "Namespaces may contain lowercase letters, digits, `_`, `-`, or `.`."
                    .to_string(),
                offset: 0,
            });
        }
    } else if let Some((prefix, suffix)) = path.split_once('/') {
        if prefix == "minecraft" {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': '{prefix}' looks like a namespace. Use '{prefix}:{suffix}' instead of a slash-separated namespace prefix."
                ),
                help: "Use `namespace:path` for explicit namespaces, not `namespace/path`."
                    .to_string(),
                offset: prefix.len(),
            });
        }
    }

    if let Some((index, c)) = path.char_indices().find(|(_, c)| c.is_ascii_uppercase()) {
        return Some(ResourceIdDiagnostic {
            message: format!(
                "Invalid {label} '{id}': uppercase character '{c}' at position {}. Use lowercase resource paths or namespace:path IDs.",
                index + 1
            ),
            help: "Use lowercase resource IDs such as `minecraft:load` or `checks/ready`."
                .to_string(),
            offset: path_offset + index,
        });
    }

    if let Some(index) = path.find('\\') {
        return Some(ResourceIdDiagnostic {
            message: format!("Invalid {label} '{id}': use '/' path separators, not '\\'"),
            help: "Resource paths use forward slashes, for example `story/root`.".to_string(),
            offset: path_offset + index,
        });
    }

    let invalid_segment = path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    let invalid_char = path.chars().any(|c| {
        !(c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || c == '_'
            || c == '-'
            || c == '.'
            || c == '/')
    });
    if path.is_empty() || invalid_segment || invalid_char {
        return Some(ResourceIdDiagnostic {
            message: format!(
                "Invalid {label} '{id}': use lowercase resource paths or namespace:path IDs with letters, digits, '/', '_', '-', or '.', and no empty, '.', or '..' segments"
            ),
            help: "Use a relative path such as `checks/ready` or an explicit ID such as `minecraft:load`."
                .to_string(),
            offset: path_offset,
        });
    }

    None
}

fn quoted_string_literal_at(text: &str, offset: usize) -> Option<&str> {
    let quote = text.get(offset..)?.chars().next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }

    let value_start = offset + quote.len_utf8();
    let end = find_string_end(text, quote, value_start)?;
    Some(&text[value_start..end - quote.len_utf8()])
}

fn array_literal_item_spans(text: &str) -> Vec<ArgumentSpan> {
    let mut result = Vec::new();
    let mut start = None;
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            index = next_index;
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                if depth == 1 && start.is_none() {
                    start = Some(index);
                }
                index = next_index;
            }
            '[' => {
                depth += 1;
                if depth == 1 {
                    start = Some(next_index);
                }
                index = next_index;
            }
            ']' => {
                if depth == 1 {
                    if let Some(start_index) = start {
                        push_array_item_span(text, start_index, index, &mut result);
                    }
                    start = None;
                }
                depth = depth.saturating_sub(1);
                index = next_index;
            }
            ',' if depth == 1 => {
                if let Some(start_index) = start {
                    push_array_item_span(text, start_index, index, &mut result);
                }
                start = Some(next_index);
                index = next_index;
            }
            '(' | '{' if depth >= 1 => {
                if depth == 1 && start.is_none() {
                    start = Some(index);
                }
                depth += 1;
                index = next_index;
            }
            ')' | '}' if depth > 1 => {
                depth = depth.saturating_sub(1);
                index = next_index;
            }
            _ => {
                if depth == 1 && start.is_none() && !ch.is_whitespace() {
                    start = Some(index);
                }
                index = next_index;
            }
        }
    }

    result
}

fn push_array_item_span(text: &str, start: usize, end: usize, result: &mut Vec<ArgumentSpan>) {
    if start > end || end > text.len() {
        return;
    }
    let span = argument_span(text, start, end);
    if !span.text.is_empty() {
        result.push(span);
    }
}

fn check_noop_expression_statement(
    line: &str,
    trimmed: &str,
    raw_trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if should_skip_noop_expression_scan(trimmed, raw_trimmed) {
        return;
    }

    if trimmed.contains('(') {
        return;
    }

    if !function_calls_in_expression(trimmed).is_empty() {
        return;
    }

    if !looks_like_noop_expression(trimmed) {
        return;
    }

    diagnostics.push(
        SourceDiagnostic::error(
            "no-op-expression",
            line_number,
            column_from_byte(line, indent),
            "Standalone expression does not generate Minecraft commands",
        )
        .with_help(
            "Assign the value to a variable, call a function/helper, use a raw command, or write `pass` for an intentional no-op.",
        ),
    );
}

fn should_skip_noop_expression_scan(trimmed: &str, raw_trimmed: &str) -> bool {
    trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('@')
        || trimmed.ends_with(':')
        || raw_trimmed.starts_with('"')
        || raw_trimmed.starts_with('\'')
        || starts_with_keyword(trimmed, "def")
        || starts_with_keyword(trimmed, "if")
        || starts_with_keyword(trimmed, "elif")
        || starts_with_keyword(trimmed, "else")
        || starts_with_keyword(trimmed, "while")
        || starts_with_keyword(trimmed, "for")
        || starts_with_keyword(trimmed, "match")
        || starts_with_keyword(trimmed, "case")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "import")
        || starts_with_keyword(trimmed, "from")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "define")
        || starts_with_keyword(trimmed, "create")
        || starts_with_keyword(trimmed, "end")
        || starts_with_keyword(trimmed, "pass")
        || starts_with_keyword(trimmed, "const")
        || single_equals_index(trimmed).is_some()
}

fn looks_like_noop_expression(trimmed: &str) -> bool {
    let first = trimmed.chars().next();
    matches!(first, Some('0'..='9' | '[' | '{' | '+' | '-'))
        || starts_with_keyword(trimmed, "True")
        || starts_with_keyword(trimmed, "False")
        || starts_with_keyword(trimmed, "None")
        || starts_with_keyword(trimmed, "not")
        || first.is_some_and(|ch| ch == '_' || ch.is_ascii_alphabetic())
}

fn datapack_json_resource_type(helper: &str) -> Option<&str> {
    match helper {
        "predicate" | "advancement" | "loot_table" | "recipe" | "item_modifier" | "dialog" => {
            Some(helper)
        }
        _ => None,
    }
}

fn datapack_tag_type(helper: &str) -> Option<&str> {
    match helper {
        "function_tag" => Some("function"),
        "block_tag" => Some("block"),
        "item_tag" => Some("item"),
        "entity_type_tag" => Some("entity_type"),
        _ => None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SourceOffsetLocation {
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Default)]
struct MappedSourceExpression {
    raw: String,
    masked: String,
    locations: Vec<SourceOffsetLocation>,
}

impl MappedSourceExpression {
    fn push_line_fragment(
        &mut self,
        line_number: usize,
        raw_line: &str,
        masked_line: &str,
        start_byte: usize,
    ) {
        if !self.raw.is_empty() {
            let newline_location = SourceOffsetLocation {
                line: line_number.saturating_sub(1).max(1),
                column: 1,
            };
            self.raw.push('\n');
            self.masked.push('\n');
            self.locations.push(newline_location);
        }

        let raw_fragment = raw_line.get(start_byte..).unwrap_or("");
        let masked_fragment = masked_line.get(start_byte..).unwrap_or("");
        for ((raw_offset, raw_char), masked_char) in
            raw_fragment.char_indices().zip(masked_fragment.chars())
        {
            let location = SourceOffsetLocation {
                line: line_number,
                column: column_from_byte(raw_line, start_byte + raw_offset),
            };
            self.raw.push(raw_char);
            self.masked.push(masked_char);
            for _ in 0..raw_char.len_utf8() {
                self.locations.push(location);
            }
        }
    }

    fn location_at(&self, offset: usize) -> SourceOffsetLocation {
        self.locations
            .get(offset)
            .copied()
            .or_else(|| self.locations.last().copied())
            .unwrap_or(SourceOffsetLocation { line: 1, column: 1 })
    }
}

fn check_multiline_datapack_helper_argument_shapes(
    source: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut active_docstring_quote = None;
    let mut active_expression: Option<MappedSourceExpression> = None;
    let mut expression_depth = 0usize;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }
        let indent = masked.len() - trimmed.len();

        if let Some(expression) = active_expression.as_mut() {
            expression.push_line_fragment(line_number, line, &masked, indent);
            update_delimiter_depth_for_indentation(trimmed, &mut expression_depth);
            if expression_depth == 0 {
                if let Some(expression) = active_expression.take() {
                    check_mapped_datapack_helper_argument_shapes(&expression, diagnostics);
                }
            }
            continue;
        }

        if !trimmed.contains("datapack.") {
            continue;
        }

        let mut depth = 0usize;
        update_delimiter_depth_for_indentation(trimmed, &mut depth);
        if depth == 0 {
            continue;
        }

        let mut expression = MappedSourceExpression::default();
        expression.push_line_fragment(line_number, line, &masked, indent);
        expression_depth = depth;
        active_expression = Some(expression);
    }
}

fn check_mapped_datapack_helper_argument_shapes(
    expression: &MappedSourceExpression,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for call in function_calls_in_expression(&expression.masked) {
        let Some(helper) = call.name.strip_prefix("datapack.") else {
            continue;
        };

        if let Some(resource_type) = datapack_json_resource_type(helper) {
            if call.arg_count != 2 {
                let location = expression.location_at(call.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        location.line,
                        location.column,
                        format!("datapack.{resource_type}() takes 2 arguments"),
                    )
                    .with_help("Pass a resource name and an object JSON value."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_mapped_datapack_resource_id_argument(
                    expression,
                    name_arg,
                    "resource name",
                    diagnostics,
                );
            }

            let Some(json_arg) = call.arguments.get(1) else {
                continue;
            };
            if !json_arg.text.trim_start().starts_with('{') {
                let location = expression.location_at(json_arg.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        location.line,
                        location.column,
                        format!("datapack.{resource_type}() JSON value must be an object"),
                    )
                    .with_help("Use an object literal such as `{ \"condition\": \"...\" }`."),
                );
            }
            continue;
        }

        if let Some(tag_type) = datapack_tag_type(helper) {
            if call.arg_count != 2 {
                let location = expression.location_at(call.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        location.line,
                        location.column,
                        format!("datapack.{tag_type}_tag() takes 2 arguments"),
                    )
                    .with_help("Pass a tag name and an array of resource IDs."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_mapped_datapack_resource_id_argument(
                    expression,
                    name_arg,
                    "tag name",
                    diagnostics,
                );
            }

            let Some(values_arg) = call.arguments.get(1) else {
                continue;
            };
            if !values_arg.text.trim_start().starts_with('[') {
                let location = expression.location_at(values_arg.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        location.line,
                        location.column,
                        "Tag values must be an array",
                    )
                    .with_help("Use an array literal such as `[\"minecraft:stone\"]`."),
                );
            } else {
                check_mapped_datapack_tag_value_resource_ids(expression, values_arg, diagnostics);
            }
        }
    }
}

fn check_mapped_datapack_resource_id_argument(
    expression: &MappedSourceExpression,
    argument: &FunctionCallArgumentSpan,
    label: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let Some(value) = quoted_string_literal_at(&expression.raw, argument.offset) else {
        return;
    };
    let Some(diagnostic) = validate_datapack_resource_id(label, value) else {
        return;
    };
    let location = expression.location_at(argument.offset + 1 + diagnostic.offset);
    diagnostics.push(
        SourceDiagnostic::error(
            "datapack-resource-id",
            location.line,
            location.column,
            diagnostic.message,
        )
        .with_help(diagnostic.help),
    );
}

fn check_mapped_datapack_tag_value_resource_ids(
    expression: &MappedSourceExpression,
    argument: &FunctionCallArgumentSpan,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let raw_argument = expression
        .raw
        .get(argument.offset..argument.offset + argument.text.len())
        .unwrap_or(&argument.text);

    for item in array_literal_item_spans(raw_argument) {
        let item_offset = argument.offset + item.offset;
        if !item.text.starts_with('"') && !item.text.starts_with('\'') {
            let location = expression.location_at(item_offset);
            diagnostics.push(
                SourceDiagnostic::error(
                    "datapack-resource-argument",
                    location.line,
                    location.column,
                    "Tag values must be string resource IDs",
                )
                .with_help("Use string values such as `[\"minecraft:stone\"]`."),
            );
            continue;
        }

        let Some(value) = quoted_string_literal_at(raw_argument, item.offset) else {
            continue;
        };
        let Some(diagnostic) = validate_datapack_resource_id("tag value", value) else {
            continue;
        };
        let location = expression.location_at(item_offset + 1 + diagnostic.offset);
        diagnostics.push(
            SourceDiagnostic::error(
                "datapack-resource-id",
                location.line,
                location.column,
                diagnostic.message,
            )
            .with_help(diagnostic.help),
        );
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionCallSpan {
    name: String,
    offset: usize,
    arg_count: usize,
    arguments: Vec<FunctionCallArgumentSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionCallArgumentSpan {
    text: String,
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct FunctionSignature {
    name: String,
    params: Vec<String>,
    line: usize,
    column: usize,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DelimiterFrame {
    delimiter: char,
    line: usize,
    column: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct QuoteFrame {
    quote: char,
    line: usize,
    column: usize,
}

fn function_calls_in_expression(expression: &str) -> Vec<FunctionCallSpan> {
    let mut calls = Vec::new();
    let bytes = expression.as_bytes();
    let mut index = 0;

    while index < bytes.len() {
        if bytes[index] != b'(' {
            index += 1;
            continue;
        }

        let Some((name, offset)) = function_name_before_open_paren(expression, index) else {
            index += 1;
            continue;
        };
        let Some(close_index) = matching_close_paren(expression, index) else {
            index += 1;
            continue;
        };

        if !is_control_word(&name) {
            let arguments = split_top_level_arg_spans(&expression[index + 1..close_index])
                .into_iter()
                .filter(|argument| !argument.text.is_empty())
                .map(|argument| FunctionCallArgumentSpan {
                    text: argument.text,
                    offset: index + 1 + argument.offset,
                })
                .collect::<Vec<_>>();
            let arg_count = arguments.len();
            calls.push(FunctionCallSpan {
                name,
                offset,
                arg_count,
                arguments,
            });
        }
        index += 1;
    }

    calls
}

fn matching_close_paren(expression: &str, open_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }

    None
}

fn split_top_level_args(args: &str) -> Vec<&str> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;

    for (index, byte) in args.as_bytes().iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                result.push(&args[start..index]);
                start = index + 1;
            }
            _ => {}
        }
    }

    result.push(&args[start..]);
    result
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArgumentSpan {
    text: String,
    offset: usize,
}

fn split_top_level_arg_spans(args: &str) -> Vec<ArgumentSpan> {
    let mut result = Vec::new();
    let mut start = 0;
    let mut depth = 0usize;

    for (index, byte) in args.as_bytes().iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => depth = depth.saturating_sub(1),
            b',' if depth == 0 => {
                result.push(argument_span(args, start, index));
                start = index + 1;
            }
            _ => {}
        }
    }

    result.push(argument_span(args, start, args.len()));
    result
}

fn argument_span(args: &str, start: usize, end: usize) -> ArgumentSpan {
    let text = &args[start..end];
    let trim_start = text.len() - text.trim_start().len();
    let trimmed = text.trim();

    ArgumentSpan {
        text: trimmed.to_string(),
        offset: start + trim_start,
    }
}

fn function_name_before_open_paren(expression: &str, open_index: usize) -> Option<(String, usize)> {
    let bytes = expression.as_bytes();
    let mut end = open_index;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }

    let mut start = end;
    while start > 0 {
        let byte = bytes[start - 1];
        if byte.is_ascii_alphanumeric() || byte == b'_' || byte == b'.' {
            start -= 1;
        } else {
            break;
        }
    }

    if start == end {
        return None;
    }

    let name = expression[start..end].trim_matches('.').to_string();
    if name.is_empty() || name.chars().next().is_some_and(|ch| ch.is_ascii_digit()) {
        return None;
    }

    Some((name, start))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct MathValueFunctionDiagnostic {
    kind: &'static str,
    message: String,
    help: &'static str,
}

fn invalid_math_value_function_call(
    call: &FunctionCallSpan,
) -> Option<MathValueFunctionDiagnostic> {
    if !call.name.starts_with("math.") {
        return None;
    }

    let Some(arity) = math_value_function_arity(&call.name) else {
        return Some(MathValueFunctionDiagnostic {
            kind: "undefined-function",
            message: format!("Unknown math function `{}`", call.name),
            help: "Use one of the supported math intrinsics: math.sqrt, math.abs, math.min, or math.max.",
        });
    };

    if call.arg_count != arity {
        return Some(MathValueFunctionDiagnostic {
            kind: "function-argument-count",
            message: format!(
                "{}() takes {} {}, but {} provided",
                call.name,
                arity,
                argument_word(arity),
                call.arg_count
            ),
            help: "Use math.sqrt(value), math.abs(value), math.min(left, right), or math.max(left, right).",
        });
    }

    None
}

fn math_value_function_arity(name: &str) -> Option<usize> {
    match name {
        "math.sqrt" | "math.abs" => Some(1),
        "math.min" | "math.max" => Some(2),
        _ => None,
    }
}

fn argument_word(count: usize) -> &'static str {
    if count == 1 {
        "argument"
    } else {
        "arguments"
    }
}

fn is_allowed_value_function_call(name: &str) -> bool {
    math_value_function_arity(name).is_some()
}

fn is_control_word(name: &str) -> bool {
    matches!(name, "if" | "for" | "while" | "match" | "return")
}

fn looks_like_selector_definition(trimmed: &str) -> bool {
    let Some(index) = single_equals_index(trimmed) else {
        return false;
    };
    let left = trimmed[..index].trim();
    let right = trimmed[index + 1..].trim();
    left.starts_with('@')
        && left[1..]
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_')
        && right.starts_with('@')
}

fn single_equals_index(text: &str) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut delimiter_depth = 0usize;

    for (index, byte) in bytes.iter().enumerate() {
        match byte {
            b'(' | b'[' | b'{' => {
                delimiter_depth += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if delimiter_depth > 0 {
            continue;
        }

        if *byte != b'=' {
            continue;
        }
        let previous = index.checked_sub(1).and_then(|i| bytes.get(i)).copied();
        let next = bytes.get(index + 1).copied();
        if matches!(
            previous,
            Some(b'=' | b'!' | b'<' | b'>' | b'+' | b'-' | b'*' | b'/' | b'%' | b'^')
        ) || matches!(next, Some(b'='))
        {
            continue;
        }
        return Some(index);
    }
    None
}

fn starts_with_keyword(text: &str, keyword: &str) -> bool {
    let Some(rest) = text.strip_prefix(keyword) else {
        return false;
    };
    rest.is_empty() || rest.chars().next().is_some_and(|ch| !is_ident_char(ch))
}

fn find_word(text: &str, word: &str) -> Option<usize> {
    let mut search_from = 0;
    while let Some(offset) = text[search_from..].find(word) {
        let index = search_from + offset;
        let before = text[..index].chars().next_back();
        let after = text[index + word.len()..].chars().next();
        if before.is_none_or(|ch| !is_ident_char(ch)) && after.is_none_or(|ch| !is_ident_char(ch)) {
            return Some(index);
        }
        search_from = index + word.len();
    }
    None
}

fn is_ident_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || ch == '_'
}

fn column_from_byte(line: &str, byte_index: usize) -> usize {
    line[..byte_index.min(line.len())].chars().count() + 1
}

fn matching_close_delimiter(open: char) -> char {
    match open {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => open,
    }
}

fn triple_quote_pattern(quote: char) -> &'static str {
    match quote {
        '"' => "\"\"\"",
        '\'' => "'''",
        _ => "",
    }
}

fn leading_triple_quote(text: &str) -> Option<char> {
    if text.starts_with("\"\"\"") {
        Some('"')
    } else if text.starts_with("'''") {
        Some('\'')
    } else {
        None
    }
}

fn find_triple_quote(line: &str, quote: char, start: usize) -> Option<usize> {
    line.get(start..)?
        .find(triple_quote_pattern(quote))
        .map(|offset| start + offset)
}

fn find_string_end(line: &str, quote: char, start: usize) -> Option<usize> {
    let mut index = start;
    let mut escaped = false;

    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return Some(next_index);
        }
        index = next_index;
    }

    None
}

fn mask_non_code(line: &str) -> String {
    let mut output = String::with_capacity(line.len());
    let mut chars = line.chars().peekable();
    let mut quote = None;
    let mut escaped = false;

    while let Some(ch) = chars.next() {
        if let Some(active_quote) = quote {
            output.push(' ');
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                output.push('_');
            }
            '#' => {
                output.push(' ');
                for _ in chars {
                    output.push(' ');
                }
                break;
            }
            _ => output.push(ch),
        }
    }

    output
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
                    column_from_byte(line, indent + "import ".len()),
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
                    column_from_byte(line, indent + "from ".len()),
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
                    column_from_byte(line, items_offset + search_from + leading),
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
    use std::time::{SystemTime, UNIX_EPOCH};

    struct TempDir {
        path: PathBuf,
    }

    impl TempDir {
        fn new(name: &str) -> Self {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos();
            let path = std::env::temp_dir().join(format!(
                "cobble-diagnostics-{name}-{}-{nanos}",
                std::process::id()
            ));
            std::fs::create_dir_all(&path).unwrap();
            Self { path }
        }

        fn path(&self) -> &Path {
            &self.path
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.path);
        }
    }

    fn messages(source: &str) -> Vec<String> {
        analyze_source(source)
            .into_iter()
            .map(|diagnostic| diagnostic.message)
            .collect()
    }

    #[test]
    fn file_diagnostics_format_includes_source_snippet() {
        let diagnostics = FileSourceDiagnostics::new(
            "main.cbl",
            "def main():\n    value = (1 + 2\n",
            vec![SourceDiagnostic::error(
                "unclosed-delimiter",
                2,
                13,
                "Opening delimiter `(` is not closed",
            )
            .with_help("Add the matching `)` before the expression ends.")],
        );

        let formatted = format_file_diagnostics(&[diagnostics]);

        assert!(formatted.contains("main.cbl:2:13: error[unclosed-delimiter]"));
        assert!(formatted.contains("2 |     value = (1 + 2"));
        assert!(formatted.contains("  |             ^"));
        assert!(formatted.contains("help: Add the matching `)`"));
    }

    #[test]
    fn byte_offsets_account_for_crlf_line_endings() {
        let source = "first\r\nsecond\r\nthird";

        assert_eq!(byte_offset_for_line_column(source, 2, 1), "first\r\n".len());
        assert_eq!(
            byte_offset_for_line_column(source, 3, 3),
            "first\r\nsecond\r\nth".len()
        );
    }

    #[test]
    fn masks_strings_and_comments_before_detection() {
        let diagnostics = analyze_source(
            r#"
def main():
    message = "class try lambda"
    /tellraw @a {"text":"i for i in values"}
    score = 1 # score += 1
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_python_like_unsupported_syntax() {
        let source = r#"
@tick
def reward(player, amount=1, *extra):
    values = [i for i in range(3)]
    score += 1
    obj.field = 2
    break
    continue
    assert score
    raise score
    del score
import foo.bar
"#;

        let diagnostics = messages(source);
        assert!(diagnostics.iter().any(|m| m.contains("Decorators")));
        assert!(diagnostics
            .iter()
            .any(|m| m.contains("Default parameter values")));
        assert!(diagnostics.iter().any(|m| m.contains("`*args`")));
        assert!(diagnostics.iter().any(|m| m.contains("comprehensions")));
        assert!(diagnostics
            .iter()
            .any(|m| m.contains("Compound assignment")));
        assert!(diagnostics
            .iter()
            .any(|m| m.contains("simple identifier assignment")));
        assert!(diagnostics.iter().any(|m| m.contains("Dotted imports")));
        assert!(diagnostics.iter().any(|m| m.contains("`break`")));
        assert!(diagnostics.iter().any(|m| m.contains("`continue`")));
        assert!(diagnostics.iter().any(|m| m.contains("`assert`")));
        assert!(diagnostics.iter().any(|m| m.contains("`raise`")));
        assert!(diagnostics.iter().any(|m| m.contains("`del`")));
    }

    #[test]
    fn reports_each_named_unsupported_python_construct() {
        for (source, expected) in [
            ("class Reward:\n    pass\n", "`class` is not supported"),
            ("try:\n    pass\n", "`try` is not supported"),
            (
                "except ValueError:\n    pass\n",
                "`except` is not supported",
            ),
            ("finally:\n    pass\n", "`finally` is not supported"),
            ("with storage:\n    pass\n", "`with` is not supported"),
            ("nonlocal score\n", "`nonlocal` is not supported"),
            ("value = lambda x: x\n", "`lambda` is not supported"),
            ("yield score\n", "`yield` is not supported"),
            ("value = await score\n", "`await` is not supported"),
            ("async def main():\n    pass\n", "`async` is not supported"),
        ] {
            let diagnostics = analyze_source(source);
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.kind == "unsupported-python-syntax"
                        && diagnostic.message.contains(expected)
                }),
                "expected {expected:?} in diagnostics for source {source:?}: {diagnostics:?}",
            );
        }
    }

    #[test]
    fn reports_unsupported_import_forms() {
        let diagnostics = messages(
            r#"
import helpers as h
import foo, bar
from helpers import *
from helpers import setup as renamed
"#,
        );

        assert!(diagnostics
            .iter()
            .any(|message| message.contains("Import aliases are not supported")));
        assert!(diagnostics
            .iter()
            .any(|message| message.contains("Multiple modules in one import")));
        assert!(diagnostics
            .iter()
            .any(|message| message.contains("Wildcard imports are not supported")));
    }

    #[test]
    fn reports_duplicate_function_parameters_with_source_location() {
        let diagnostics = analyze_source(
            r#"
def greet(player, player):
    /say duplicate
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "duplicate-function-parameter");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 19);
        assert!(diagnostics[0]
            .message
            .contains("Duplicate function parameter `player`"));
    }

    #[test]
    fn reports_for_else_without_rejecting_if_else() {
        let diagnostics = analyze_source(
            r#"
def main():
    if True:
        pass
    else:
        pass
    for i in range(3):
        pass
    else:
        pass
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-control-flow");
        assert_eq!(diagnostics[0].line, 9);
    }

    #[test]
    fn reports_missing_block_colon_with_source_location() {
        let diagnostics = parse_source(
            r#"
def main()
    /say missing colon
"#,
        )
        .expect_err("missing colon should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "missing-block-colon");
        assert_eq!(diagnostics[0].line, 2);
        assert!(diagnostics[0].column > 1);
    }

    #[test]
    fn reports_unclosed_delimiter_with_source_location() {
        let diagnostics = parse_source(
            r#"
def main():
    value = (1 + 2
"#,
        )
        .expect_err("unclosed delimiter should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unclosed-delimiter");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 13);
        assert!(diagnostics[0]
            .message
            .contains("Opening delimiter `(` is not closed"));
    }

    #[test]
    fn reports_unmatched_closing_delimiter_with_source_location() {
        let diagnostics = parse_source(
            r#"
def main():
    value = 1]
"#,
        )
        .expect_err("unmatched closing delimiter should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unmatched-delimiter");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 14);
        assert!(diagnostics[0]
            .message
            .contains("Unexpected closing delimiter `]`"));
    }

    #[test]
    fn reports_unterminated_string_with_source_location() {
        let diagnostics = parse_source(
            r#"
def main():
    message = "oops
"#,
        )
        .expect_err("unterminated string should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unterminated-string");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 15);
    }

    #[test]
    fn reports_unexpected_indentation_with_source_location() {
        let diagnostics = parse_source(
            r#"
score = 1
    score = 2
"#,
        )
        .expect_err("unexpected indentation should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unexpected-indentation");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 5);
    }

    #[test]
    fn reports_inconsistent_indentation_with_source_location() {
        let diagnostics = parse_source(
            r#"
def main():
    if True:
        pass
      pass
"#,
        )
        .expect_err("inconsistent indentation should fail before parser fallback");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "inconsistent-indentation");
        assert_eq!(diagnostics[0].line, 5);
        assert_eq!(diagnostics[0].column, 7);
    }

    #[test]
    fn allows_multiline_expression_indentation() {
        let diagnostics = analyze_source(
            r#"
def main():
    value = (
        1 + 2
    )
    datapack.predicate(
        "always",
        {"condition": "minecraft:random_chance", "chance": 1}
    )
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn ignores_delimiters_inside_strings_comments_docstrings_and_raw_commands() {
        let diagnostics = analyze_source(
            r#"
def main():
    text = "literal ) ] }"
    value = 1 # (
    """Docstring can mention ( [ {
    across lines until it closes.
    """
    /say raw command can mention ]
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn ignores_selector_argument_equals_in_execute_modifiers() {
        let diagnostics = analyze_source(
            r#"
def main():
    as @Players at @s if entity @s[distance=..32]:
        /say nearby
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_return_duplicate_function_and_call_assignment() {
        let diagnostics = analyze_source(
            r#"
def helper():
    pass

def helper():
    return

def main():
    value = helper()
"#,
        );

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "duplicate-function"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "unsupported-return"));
        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "unsupported-function-call-expression"));
    }

    #[test]
    fn allows_math_intrinsics_in_assignment_values() {
        let diagnostics = analyze_source(
            r#"
def main():
    root = math.sqrt(64)
    lower = math.min(3, 4)
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn parse_source_returns_preflight_diagnostics() {
        let diagnostics = parse_source(
            r#"
def main():
    return
"#,
        )
        .expect_err("parse_source should reject unsupported return");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-return");
    }

    #[test]
    fn reports_type_mismatch_for_known_reassignments() {
        let diagnostics = analyze_source(
            r#"
def main():
    items = ["sword"]
    items = 3
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "type-mismatch");
        assert_eq!(diagnostics[0].line, 4);
        assert!(diagnostics[0]
            .message
            .contains("Type mismatch for variable 'items'"));
        assert!(diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("previously defined as type: list"));
    }

    #[test]
    fn reports_type_mismatch_for_module_variables_inside_functions() {
        let diagnostics = analyze_source(
            r#"
score = 1

def main():
    score = "done"
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "type-mismatch");
        assert_eq!(diagnostics[0].line, 5);
        assert!(diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("Cannot reassign to type: string"));
    }

    #[test]
    fn reports_undefined_before_type_mismatch_for_unknown_expression_inputs() {
        let diagnostics = analyze_source(
            r#"
flag = True

def main():
    flag = missing_score + 1
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-variable");
        assert!(diagnostics[0]
            .message
            .contains("Undefined variable `missing_score`"));
    }

    #[test]
    fn reports_datapack_json_resource_argument_shape_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.predicate("bad", ["not", "an", "object"])
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "datapack-resource-argument");
        assert!(diagnostics[0]
            .message
            .contains("datapack.predicate() JSON value must be an object"));
    }

    #[test]
    fn reports_datapack_tag_argument_shape_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.block_tag("bad", {"values": ["minecraft:stone"]})
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "datapack-resource-argument");
        assert!(diagnostics[0]
            .message
            .contains("Tag values must be an array"));
    }

    #[test]
    fn reports_datapack_tag_non_string_values() {
        let diagnostics = analyze_source(
            r#"
datapack.item_tag("bad", [1, True])
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind == "datapack-resource-argument"));
        assert!(diagnostics.iter().all(|diagnostic| diagnostic
            .message
            .contains("Tag values must be string resource IDs")));
    }

    #[test]
    fn reports_datapack_resource_id_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.function_tag("minecraft/load", ["resources:setup"])
datapack.predicate("Checks/Ready", {"condition": "minecraft:random_chance", "chance": 1})
datapack.item_tag("rewards", ["minecraft/diamond"])
"#,
        );

        assert_eq!(diagnostics.len(), 3);
        assert_eq!(diagnostics[0].kind, "datapack-resource-id");
        assert_eq!(diagnostics[0].line, 2);
        assert!(diagnostics[0]
            .message
            .contains("Use 'minecraft:load' instead"));
        assert_eq!(diagnostics[1].kind, "datapack-resource-id");
        assert_eq!(diagnostics[1].line, 3);
        assert!(diagnostics[1].message.contains("uppercase character 'C'"));
        assert_eq!(diagnostics[2].kind, "datapack-resource-id");
        assert_eq!(diagnostics[2].line, 4);
        assert!(diagnostics[2]
            .message
            .contains("Use 'minecraft:diamond' instead"));
    }

    #[test]
    fn reports_multiline_datapack_resource_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.item_tag(
    "rewards",
    ["minecraft/diamond", 1],
)
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].kind, "datapack-resource-id");
        assert_eq!(diagnostics[0].line, 4);
        assert!(diagnostics[0]
            .message
            .contains("Use 'minecraft:diamond' instead"));
        assert_eq!(diagnostics[1].kind, "datapack-resource-argument");
        assert_eq!(diagnostics[1].line, 4);
        assert!(diagnostics[1]
            .message
            .contains("Tag values must be string resource IDs"));
    }

    #[test]
    fn reports_undefined_variables_in_variable_dependent_expressions() {
        let diagnostics = analyze_source(
            r#"
def main():
    total = missing_score + 1
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-variable");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Undefined variable `missing_score`"));
    }

    #[test]
    fn reports_noop_standalone_expression_statements() {
        let diagnostics = analyze_source(
            r#"
def main():
    score + 1
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "no-op-expression");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 5);
        assert!(diagnostics[0]
            .message
            .contains("Standalone expression does not generate Minecraft commands"));
    }

    #[test]
    fn allows_pass_docstrings_and_call_statements() {
        let diagnostics = analyze_source(
            r#"
def main():
    pass
    """Single-line docstring with class and try words"""
    """
    Multi-line docstring with standalone words
    and arithmetic-looking text x + 1
    """
    helper()
    text.tellraw("@a", text.plain("ok"))

def helper():
    /say helper
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn ignores_multiline_docstring_bodies_in_later_semantic_passes() {
        let diagnostics = analyze_source(
            r#"
def main():
    """
    missing = unknown_value + 1
    helper("@a")
    from fake import missing
    """
    pass

def helper(player, message):
    pass
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_undefined_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
def main(player):
    /tellraw {player} {"text":"{message}"}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-placeholder");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Undefined command placeholder `message`"));
        assert!(diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("`{{message}}`"));
    }

    #[test]
    fn reports_forward_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
def main():
    /say {message}
    message = "hi"
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-placeholder");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Undefined command placeholder `message`"));
    }

    #[test]
    fn reports_unclosed_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
def main(player):
    /say {player
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unclosed-placeholder");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 10);
    }

    #[test]
    fn reports_invalid_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
def main():
    /say {bad-name}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "invalid-placeholder");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Invalid command placeholder `bad-name`"));
    }

    #[test]
    fn allows_defined_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
score = 1

def main(player):
    message = "ready"
    for i in range(3):
        /tellraw {player} {"text":"Score {score} {message} {i}"}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn allows_imported_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
from helper import imported_score

def main():
    /say {imported_score}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_treat_imported_modules_as_raw_command_placeholders() {
        let diagnostics = analyze_source(
            r#"
import helper

def main():
    /say {helper}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-placeholder");
        assert!(diagnostics[0]
            .message
            .contains("Undefined command placeholder `helper`"));
    }

    #[test]
    fn ignores_json_nbt_and_escaped_raw_command_braces() {
        let diagnostics = analyze_source(
            r#"
def main():
    /tellraw @a {"text":"literal {{name}}", "color":"gold"}
    /tellraw @a {"text":"literal {"}
    /data merge entity @s {}
    /data merge entity @s {Tags:["foo"]}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_report_defined_imported_builtin_or_parameter_symbols() {
        let diagnostics = analyze_source(
            r#"
import stdlib
from stdlib import event

counter = 1

def tick(player):
    global counter
    counter = counter + 1
    root = math.sqrt(counter)
    ready = counter > 0 and True
    if ready:
        /tellraw {player} {"text":"ready"}

stdlib.addEventListener(event.TICK, tick)
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_report_string_or_map_literal_tokens_as_undefined() {
        let diagnostics = analyze_source(
            r#"
status = "boot"
config = {enabled: True, label: "ready"}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn does_not_replace_attribute_or_subscript_errors_with_undefined_variables() {
        let diagnostics = analyze_source(
            r#"
def main():
    field = obj.value
    item = arr[0]
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert!(diagnostics
            .iter()
            .all(|diagnostic| diagnostic.kind == "unsupported-storage-access"));
        assert!(diagnostics[0]
            .message
            .contains("Cannot resolve storage-backed attribute access `obj.`"));
        assert!(diagnostics[1]
            .message
            .contains("Cannot resolve storage-backed subscript access `arr[...]`"));
    }

    #[test]
    fn reports_user_function_argument_count_for_forward_calls() {
        let diagnostics = analyze_source(
            r#"
def main():
    greet("@a")

def greet(player, message):
    /tellraw {player} {"text":"{message}"}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "function-argument-count");
        assert!(diagnostics[0]
            .message
            .contains("Function `greet` expects 2 argument(s), but 1 provided"));
        assert_eq!(diagnostics[0].line, 3);
    }

    #[test]
    fn reports_undefined_user_function_calls() {
        let diagnostics = analyze_source(
            r#"
def main():
    missing("x")
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-function");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Undefined function `missing`"));
    }

    #[test]
    fn reports_unknown_dotted_helper_calls() {
        let diagnostics = analyze_source(
            r#"
def main():
    helper.do()
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-function");
        assert!(diagnostics[0]
            .message
            .contains("Unknown helper function `helper.do`"));

        let diagnostics = analyze_source(
            r#"
def main():
    storage.nope()
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-function");
        assert!(diagnostics[0]
            .message
            .contains("Unknown helper function `storage.nope`"));
    }

    #[test]
    fn reports_value_only_text_helpers_as_standalone_statements() {
        let diagnostics = analyze_source(
            r#"
def main():
    text.plain("hello")
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-function-call-expression");
        assert!(diagnostics[0]
            .message
            .contains("returns a value and cannot be used as a standalone statement"));
    }

    #[test]
    fn reports_nested_function_call_arguments_for_user_functions() {
        let diagnostics = analyze_source(
            r#"
def main():
    greet(make_name())

def make_name():
    pass

def greet(name):
    /say {name}
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-function-call-argument");
        assert!(diagnostics[0]
            .message
            .contains("Function `greet` arguments cannot contain function call expressions"));
        assert_eq!(diagnostics[0].line, 3);
    }

    #[test]
    fn does_not_report_module_call_argument_counts() {
        let diagnostics = analyze_source(
            r#"
def main():
    text.tellraw("@a", text.plain("ok"))
    score.set("points", 1)
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_undefined_variables_in_standalone_call_arguments() {
        let diagnostics = analyze_source(
            r#"
def main():
    score.set("points", missing)
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-variable");
        assert_eq!(diagnostics[0].line, 3);
        assert!(diagnostics[0]
            .message
            .contains("Undefined variable `missing`"));
    }

    #[test]
    fn reports_unsupported_none_usage_but_allows_json_resource_null() {
        let diagnostics = analyze_source(
            r#"
def main():
    value = None
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-none");
        assert!(diagnostics[0]
            .message
            .contains("None/null is only supported in data pack JSON resource helper values"));

        let diagnostics = analyze_source(
            r#"
datapack.predicate("maybe", {
    "condition": "minecraft:random_chance",
    "chance": 1,
    "comment": None
})
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn rejects_lowercase_null_and_none_outside_json_resource_values() {
        let diagnostics = analyze_source(
            r#"
datapack.predicate("maybe", {"condition": "minecraft:random_chance", "chance": null})
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-none");
        assert!(diagnostics[0]
            .message
            .contains("None/null is only supported"));

        let diagnostics = analyze_source(
            r#"
datapack.predicate(None, {"condition": "minecraft:random_chance", "chance": 1})
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-none");
    }

    #[test]
    fn reports_unsupported_storage_access_shapes() {
        let diagnostics = analyze_source(
            r#"
def main():
    items = [1, 2, 3]
    first = items[i]
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-storage-access");
        assert!(diagnostics[0]
            .message
            .contains("Dynamic storage-backed subscript indexes are not supported"));

        let diagnostics = analyze_source(
            r#"
def main():
    x = 1
    y = x.foo
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-storage-access");
        assert!(diagnostics[0]
            .message
            .contains("does not support attribute access"));
    }

    #[test]
    fn allows_storage_backed_subscript_with_numeric_const_index() {
        let diagnostics = analyze_source(
            r#"
const INDEX = 0

def main():
    items = [1, 2, 3]
    first = items[INDEX]
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");

        let diagnostics = analyze_source(
            r#"
def main():
    const INDEX = 1
    items = [1, 2, 3]
    second = items[INDEX]
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_storage_access_and_later_type_mismatch_together() {
        let diagnostics = analyze_source(
            r#"
def main():
    items = [1, 2, 3]
    first = items[i]
    value = 1
    value = "one"
"#,
        );

        assert_eq!(diagnostics.len(), 2);
        assert_eq!(diagnostics[0].kind, "unsupported-storage-access");
        assert_eq!(diagnostics[1].kind, "type-mismatch");
    }

    #[test]
    fn reports_invalid_math_value_function_calls() {
        let diagnostics = analyze_source(
            r#"
def main():
    value = math.nope(1)
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "undefined-function");
        assert!(diagnostics[0]
            .message
            .contains("Unknown math function `math.nope`"));

        let diagnostics = analyze_source(
            r#"
def main():
    value = math.sqrt(1, 2)
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "function-argument-count");
        assert!(diagnostics[0]
            .message
            .contains("math.sqrt() takes 1 argument, but 2 provided"));
    }

    #[test]
    fn parse_source_file_reports_missing_import_at_import_site() {
        let temp_dir = TempDir::new("missing-import");
        let main = temp_dir.path().join("main.cbl");
        std::fs::write(&main, "import missing\n\ndef main():\n    pass\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("missing import should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "missing-import");
        assert_eq!(diagnostics[0].diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].diagnostics[0].column, 8);
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Cannot import 'missing'"));
    }

    #[test]
    fn parse_source_file_reports_import_cycle_with_chain() {
        let temp_dir = TempDir::new("import-cycle");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef main():\n    pass\n").unwrap();
        std::fs::write(&helper, "import main\n\ndef helper():\n    pass\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("import cycle should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, helper);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "circular-import");
        let help = diagnostics[0].diagnostics[0].help.as_deref().unwrap();
        assert!(help.contains("Import chain:"));
        assert!(help.contains(main.to_string_lossy().as_ref()));
        assert!(help.contains(helper.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_file_reports_imported_file_language_diagnostics() {
        let temp_dir = TempDir::new("imported-language-diagnostics");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef main():\n    pass\n").unwrap();
        std::fs::write(&helper, "def helper(value=1):\n    pass\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("imported diagnostic should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, helper);
        assert_eq!(
            diagnostics[0].diagnostics[0].kind,
            "unsupported-function-parameter"
        );
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("Import chain:"));
    }

    #[test]
    fn parse_source_file_reports_missing_from_import_item() {
        let temp_dir = TempDir::new("missing-from-import-item");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(
            &main,
            "from helper import greet, missing\n\ndef main():\n    pass\n",
        )
        .unwrap();
        std::fs::write(&helper, "def greet():\n    /say hi\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("missing import item should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "missing-import-item");
        assert_eq!(diagnostics[0].diagnostics[0].line, 1);
        assert_eq!(diagnostics[0].diagnostics[0].column, 27);
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Cannot import `missing` from `helper`"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("Available symbols: greet"));
    }

    #[test]
    fn parse_source_file_validates_items_for_already_visited_imports() {
        let temp_dir = TempDir::new("visited-missing-from-import-item");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(
            &main,
            "import helper\nfrom helper import missing\n\ndef main():\n    pass\n",
        )
        .unwrap();
        std::fs::write(&helper, "def greet():\n    /say hi\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("visited import item should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "missing-import-item");
        assert_eq!(diagnostics[0].diagnostics[0].line, 2);
    }

    #[test]
    fn parse_source_file_rejects_imported_function_raw_placeholder() {
        let temp_dir = TempDir::new("imported-function-placeholder");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(
            &main,
            "from helper import greet\n\ndef main():\n    /say {greet}\n",
        )
        .unwrap();
        std::fs::write(&helper, "def greet():\n    /say hi\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("function placeholder should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(
            diagnostics[0].diagnostics[0].kind,
            "unsupported-placeholder-symbol"
        );
        assert_eq!(diagnostics[0].diagnostics[0].line, 4);
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Imported function `greet` cannot be used as a command placeholder"));
    }

    #[test]
    fn parse_source_file_reports_cross_file_duplicate_functions() {
        let temp_dir = TempDir::new("cross-file-duplicate-functions");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef greet():\n    /say from main\n").unwrap();
        std::fs::write(&helper, "def greet():\n    /say from helper\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("duplicate function should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "duplicate-function");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Duplicate function definition `greet` across imported files"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains(helper.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_files_reports_directory_duplicate_functions() {
        let temp_dir = TempDir::new("directory-duplicate-functions");
        let first = temp_dir.path().join("first.cbl");
        let second = temp_dir.path().join("second.cbl");
        std::fs::write(&first, "def same():\n    /say first\n").unwrap();
        std::fs::write(&second, "def same():\n    /say second\n").unwrap();

        let diagnostics = parse_source_files(&[first.clone(), second.clone()])
            .expect_err("duplicate should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, second);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "duplicate-function");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Duplicate function definition `same` across imported files"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains(first.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_file_reports_cross_file_duplicate_selector_aliases() {
        let temp_dir = TempDir::new("cross-file-duplicate-selectors");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(
            &main,
            "import helper\n\n@Players = @a\n\ndef main():\n    pass\n",
        )
        .unwrap();
        std::fs::write(&helper, "@Players = @p\n\ndef helper():\n    pass\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("duplicate selector should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "duplicate-symbol");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Duplicate selector alias `@Players` across imported files"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains(helper.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_file_reports_cross_file_duplicate_entity_templates() {
        let temp_dir = TempDir::new("cross-file-duplicate-entities");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        let entity = "define @Marker = @e[type=marker]\ncreate {\"Tags\": [\"marker\"]}\nend\n";
        std::fs::write(
            &main,
            format!("import helper\n\n{entity}\ndef main():\n    pass\n"),
        )
        .unwrap();
        std::fs::write(&helper, format!("{entity}\ndef helper():\n    pass\n")).unwrap();

        let diagnostics = parse_source_file(&main).expect_err("duplicate entity should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "duplicate-symbol");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Duplicate entity template `@Marker` across imported files"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains(helper.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_file_reports_imported_function_argument_count() {
        let temp_dir = TempDir::new("imported-function-argument-count");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef main():\n    greet(\"@a\")\n").unwrap();
        std::fs::write(
            &helper,
            "def greet(player, message):\n    /tellraw {player} {\"text\":\"{message}\"}\n",
        )
        .unwrap();

        let diagnostics = parse_source_file(&main).expect_err("argument count should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(
            diagnostics[0].diagnostics[0].kind,
            "function-argument-count"
        );
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Function `greet` expects 2 argument(s), but 1 provided"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains(helper.to_string_lossy().as_ref()));
    }

    #[test]
    fn parse_source_file_reports_unknown_cross_file_function_calls() {
        let temp_dir = TempDir::new("unknown-cross-file-function");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef main():\n    missing(\"@a\")\n").unwrap();
        std::fs::write(&helper, "def greet(player):\n    /say {player}\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("unknown function should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "undefined-function");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Undefined function `missing`"));
    }

    #[test]
    fn parse_source_file_reports_unknown_cross_file_dotted_helper_calls() {
        let temp_dir = TempDir::new("unknown-cross-file-dotted-function");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(&main, "import helper\n\ndef main():\n    helper.do()\n").unwrap();
        std::fs::write(&helper, "def greet(player):\n    /say {player}\n").unwrap();

        let diagnostics = parse_source_file(&main).expect_err("unknown helper should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].path, main);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "undefined-function");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Unknown helper function `helper.do`"));
    }
}
