use crate::ast::{CobbleType, Expression, Program, Statement};
use crate::parser::{parse, parse_located_with_diagnostics, LocatedProgram};
use std::collections::{HashMap, HashSet};

#[path = "diagnostics/imports.rs"]
mod imports;
#[path = "diagnostics/python_compat.rs"]
mod python_compat;
#[path = "diagnostics/reporting.rs"]
mod reporting;
#[path = "diagnostics/text.rs"]
mod text;

#[cfg(test)]
use imports::MAX_IMPORT_DEPTH;
pub use imports::{analyze_in_memory_imports, parse_source_file, parse_source_files};
pub use python_compat::{
    python_compat_observed_constructs, python_compat_suggestion_for_keyword,
    python_compat_suggestion_for_kind, python_compat_supported_constructs,
};
pub use reporting::{
    format_diagnostics, format_diagnostics_with_source, format_file_diagnostics,
    DiagnosticSeverity, FileSourceDiagnostics, ParsedSourceFile, SourceDiagnostic,
};
pub use text::byte_offset_for_line_column;

use text::{
    argument_span, column_from_byte, find_string_end, find_triple_quote, find_word,
    function_name_before_open_paren, is_ident_char, leading_triple_quote, mask_non_code,
    matching_close_delimiter, matching_close_paren, raw_offset_from_masked, single_equals_index,
    split_top_level_arg_spans, split_top_level_args, starts_with_keyword, triple_quote_pattern,
    ArgumentSpan,
};

/// Base keyword sets shared by the line-scan skip lists below. Each scan site
/// combines these slices and documents its own additions/omissions so every
/// check's effective skip set stays explicit in one place.
const DECLARATION_KEYWORDS: &[&str] = &["def", "import", "from", "define"];
const CONTROL_FLOW_KEYWORDS: &[&str] = &["if", "elif", "while", "for"];
const RESOURCE_BLOCK_KEYWORDS: &[&str] = &["create", "end"];

fn starts_with_any_keyword(trimmed: &str, keywords: &[&str]) -> bool {
    keywords
        .iter()
        .any(|keyword| starts_with_keyword(trimmed, keyword))
}

pub fn parse_source(source: &str) -> Result<Program, Vec<SourceDiagnostic>> {
    parse_source_located(source).map(|located| located.program)
}

pub fn parse_source_located(source: &str) -> Result<LocatedProgram, Vec<SourceDiagnostic>> {
    let diagnostics = analyze_source(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    parse_located_with_diagnostics(source).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| {
                SourceDiagnostic::error_at_span(
                    "parse",
                    error.span,
                    format!("Parse error: {}", error.message),
                )
            })
            .collect()
    })
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
        check_imports(trimmed, line_number, indent, &mut diagnostics);
        check_function_definition(
            trimmed,
            line_number,
            indent,
            &mut function_defs,
            &mut diagnostics,
        );
        check_function_parameters(trimmed, line_number, indent, &mut diagnostics);
        check_assignment_target(line, trimmed, line_number, indent, &mut diagnostics);
        check_assignment_function_calls(trimmed, line_number, indent, &mut diagnostics);
        check_comprehension(&masked, line_number, &mut diagnostics);
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

pub(super) fn update_delimiter_depth_for_indentation(masked_line: &str, depth: &mut usize) {
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

pub(super) fn should_skip_docstring_scan_line(
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
                    indent + column_from_byte(trimmed, index),
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
                    indent + column_from_byte(trimmed, "import ".len() + index),
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
                    indent + column_from_byte(trimmed, "import ".len() + index),
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
                    indent + column_from_byte(trimmed, "import ".len()),
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
                    indent + column_from_byte(trimmed, "import ".len() + index),
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

        let items_offset = "from ".len() + module.len() + " import ".len();

        if module.starts_with('.') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-import",
                    line_number,
                    indent + column_from_byte(trimmed, "from ".len()),
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
                    indent + column_from_byte(trimmed, "from ".len() + index),
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
                    indent + column_from_byte(trimmed, items_offset + leading),
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
                    indent + column_from_byte(trimmed, items_offset + index),
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
                indent + column_from_byte(trimmed, open + 1 + index),
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
                indent + column_from_byte(trimmed, open + 1 + index),
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

        let column = indent + column_from_byte(trimmed, open + 1 + param.offset);
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
    trimmed: &str,
    line_number: usize,
    indent: usize,
    function_defs: &mut HashMap<String, FunctionSignature>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if !starts_with_keyword(trimmed, "def") {
        return;
    }

    let Some(signature) = parse_function_signature(trimmed, line_number, indent) else {
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

pub(super) fn parse_function_signature(
    trimmed: &str,
    line_number: usize,
    indent: usize,
) -> Option<FunctionSignature> {
    let rest = trimmed.strip_prefix("def")?.trim_start();
    // Column of the function name, computed from the `def ` prefix width so a
    // name that also occurs inside `def ` (e.g. `def de()`) is not mislocated.
    let name_offset = trimmed.len() - rest.len();
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
        column: indent + column_from_byte(trimmed, name_offset),
    })
}

fn check_assignment_target(
    line: &str,
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // Declarations, control-flow headers, `return`/`global` statements, and
    // selector (`@`) lines are never assignment statements.
    if starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, CONTROL_FLOW_KEYWORDS)
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "global")
        || trimmed.starts_with('@')
    {
        return;
    }

    if let Some(compound) = compound_assignment_span(trimmed) {
        if compound.target.contains(',') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-assignment-target",
                    line_number,
                    column_from_byte(line, indent),
                    "Unsupported assignment target",
                )
                .with_help("Assign to a variable or storage-backed attribute/subscript target."),
            );
        }
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

    if target.contains(',') {
        diagnostics.push(
            SourceDiagnostic::error(
                "unsupported-assignment-target",
                line_number,
                column_from_byte(line, indent),
                "Unsupported assignment target",
            )
            .with_help("Assign to a variable or storage-backed attribute/subscript target."),
        );
    }
}

fn check_assignment_function_calls(
    trimmed: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // Same skip set as `check_assignment_target` minus `return`: a returned
    // expression is scanned like an assignment RHS so misused value helpers in
    // `return ...` lines are reported at the `return` site.
    if starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, CONTROL_FLOW_KEYWORDS)
        || starts_with_keyword(trimmed, "global")
        || trimmed.starts_with('@')
    {
        return;
    }

    let (rhs_span, is_return_expression) = match return_expression_span(trimmed) {
        Some(span) => (span, true),
        None => match assignment_rhs_span(trimmed) {
            Some(span) => (span, false),
            None => return,
        },
    };
    let rhs = &rhs_span.text;

    for call in function_calls_in_expression(rhs) {
        if let Some(diagnostic) = invalid_value_function_call(&call) {
            diagnostics.push(
                SourceDiagnostic::error(
                    diagnostic.kind,
                    line_number,
                    indent + column_from_byte(trimmed, rhs_span.offset + call.offset),
                    diagnostic.message,
                )
                .with_help(diagnostic.help),
            );
            return;
        }

        if is_allowed_value_function_call(&call.name) {
            continue;
        }

        if is_return_expression {
            // Module calls in `return <expression>` are validated by the
            // undefined-function scan and the transpiler; only value-helper
            // misuse (len/math arity, unknown math) is reported here.
            continue;
        }

        if let Some((module, method)) = split_module_call_name(&call.name) {
            if is_known_module_call(module, method) {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "unsupported-function-call-expression",
                        line_number,
                        indent + column_from_byte(trimmed, rhs_span.offset + call.offset),
                        format!(
                            "`{}` is a command helper and cannot be used as an expression value",
                            call.name
                        ),
                    )
                    .with_help(
                        "Use command helpers as standalone statements, or call a user function that returns an integer score value.",
                    ),
                );
                return;
            }
        }
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
                continue;
            }

            diagnostics.push(
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
    let mut multiline_expression_depth = 0usize;
    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        let raw_trimmed = line.trim_start();
        if should_skip_docstring_scan_line(line, raw_trimmed, &mut active_docstring_quote) {
            continue;
        }

        let masked = mask_non_code(line);
        let trimmed = masked.trim_start();
        if should_skip_call_argument_scan(trimmed) {
            update_delimiter_depth_for_indentation(trimmed, &mut multiline_expression_depth);
            continue;
        }

        let inside_multiline_expression = multiline_expression_depth > 0;
        let indent = masked.len() - trimmed.len();
        if !inside_multiline_expression {
            for call in function_calls_in_expression(trimmed) {
                if let Some((module, method)) = split_module_call_name(&call.name) {
                    if is_user_function_call_statement(trimmed, &call) {
                        if let Some(message) =
                            value_only_module_call_message(module, method, &call.name)
                        {
                            diagnostics.push(
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
                    }

                    if is_known_module_call(module, method) {
                        continue;
                    }

                    diagnostics.push(
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

                if function_defs.contains_key(&call.name)
                    || is_allowed_value_function_call(&call.name)
                    || call.name == "range"
                {
                    continue;
                }

                diagnostics.push(
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
        }
        update_delimiter_depth_for_indentation(trimmed, &mut multiline_expression_depth);
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

pub(super) fn is_user_function_call_statement(trimmed: &str, call: &FunctionCallSpan) -> bool {
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

pub(super) fn is_known_non_user_function_call(name: &str) -> bool {
    split_module_call_name(name)
        .is_some_and(|(module, method)| is_known_module_call(module, method))
        || is_allowed_value_function_call(name)
}

pub(super) fn split_module_call_name(name: &str) -> Option<(&str, &str)> {
    name.rsplit_once('.')
}

/// Callable stdlib method names per module, layered on the shared registry
/// in `crate::stdlib::STDLIB_MODULES`. Deliberate per-site deltas from that
/// base:
/// - adds the "stdlib" pseudo-module (`stdlib.addEventListener`);
/// - omits "event": its members (`event.LOAD`, `event.TICK`) are attributes,
///   not calls;
/// - omits "datapack": its helpers are validated through the datapack
///   resource/tag tables inside `is_known_module_call`.
pub(crate) const STDLIB_MODULE_METHODS: &[(&str, &[&str])] = &[
    ("stdlib", &["addEventListener"]),
    ("math", &["sqrt", "abs", "min", "max"]),
    (
        "text",
        &[
            "plain",
            "colored",
            "score",
            "selector",
            "tellraw",
            "title",
            "subtitle",
            "actionbar",
        ],
    ),
    (
        "score",
        &["set", "add", "remove", "reset", "copy", "operation"],
    ),
    ("score.objective", &["add", "remove", "display"]),
    ("random", &["int", "bool"]),
    ("timer", &["set", "tick", "done", "reset"]),
    (
        "storage",
        &[
            "set",
            "merge",
            "remove",
            "copy",
            "append",
            "prepend",
            "insert",
            "get",
            "read_score",
            "copy_from",
            "path",
            "child",
            "index",
        ],
    ),
    (
        "item_component",
        &["components", "custom_name", "lore", "unbreakable"],
    ),
    ("schedule", &["once", "clear"]),
    (
        "selector",
        &[
            "current",
            "nearest_player",
            "all_players",
            "all_entities",
            "tagged",
            "players_tagged",
            "entity_type",
        ],
    ),
    ("position", &["here", "absolute", "relative", "local"]),
    (
        "bossbar",
        &[
            "add",
            "remove",
            "set_value",
            "set_max",
            "set_name",
            "set_color",
            "set_style",
            "set_visible",
            "set_players",
        ],
    ),
    ("team", &["add", "remove", "join", "leave", "modify"]),
    (
        "entity",
        &[
            "tag_add",
            "tag_remove",
            "effect_give",
            "effect_clear",
            "attribute_get",
            "attribute_base_set",
            "teleport",
        ],
    ),
    ("resource_pack", &["item_model", "block_model", "lang"]),
];

pub(super) fn is_known_module_call(module: &str, method: &str) -> bool {
    if module == "datapack" {
        return datapack_json_resource_type(method).is_some()
            || datapack_tag_type(method).is_some();
    }
    STDLIB_MODULE_METHODS
        .iter()
        .any(|(name, methods)| *name == module && methods.contains(&method))
}

pub(super) fn value_only_module_call_message(
    module: &str,
    method: &str,
    name: &str,
) -> Option<String> {
    (module == "text" && matches!(method, "plain" | "colored" | "score" | "selector"))
        .then(|| format!("`{name}` returns a value and cannot be used as a standalone statement"))
        .or_else(|| {
            (module == "storage" && matches!(method, "path" | "child" | "index")).then(|| {
                format!(
                    "{name}() returns a storage path string and cannot be used as a standalone statement"
                )
            })
        })
        .or_else(|| {
            (module == "item_component"
                && matches!(method, "components" | "custom_name" | "lore" | "unbreakable"))
            .then(|| {
                format!(
                    "{name}() returns an item component JSON object and cannot be used as a standalone statement"
                )
            })
        })
        .or_else(|| {
            (module == "selector"
                && matches!(
                    method,
                    "current"
                        | "nearest_player"
                        | "all_players"
                        | "all_entities"
                        | "tagged"
                        | "players_tagged"
                        | "entity_type"
                ))
            .then(|| {
                format!(
                    "{name}() returns a selector string and cannot be used as a standalone statement"
                )
            })
        })
        .or_else(|| {
            (module == "position"
                && matches!(method, "here" | "absolute" | "relative" | "local"))
            .then(|| {
                format!(
                    "{name}() returns a coordinate string and cannot be used as a standalone statement"
                )
            })
        })
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

        check_none_usage_in_line(trimmed, line_number, indent, diagnostics);
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
                indent + column_from_byte(trimmed, token.offset),
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
                    column_from_byte(
                        &masked,
                        indent + assignment.value_offset + diagnostic.offset,
                    ),
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
        let (base_type, nested_storage_chain) = match type_env.get(base) {
            Some(base_type) => (base_type.clone(), false),
            None => {
                let Some((root, _root_start)) =
                    storage_chain_root_before_subscript(expression, base_start)
                else {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Cannot resolve storage-backed subscript access `{base}[...]`"
                        ),
                        help: "Assign a list, map, or string literal to the base variable before using storage-backed access.".to_string(),
                    });
                };
                let Some(root_type) = type_env.get(root) else {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Cannot resolve storage-backed subscript access `{base}[...]`"
                        ),
                        help: "Assign a list, map, or string literal to the base variable before using storage-backed access.".to_string(),
                    });
                };
                if !matches!(
                    root_type,
                    CobbleType::List | CobbleType::Map | CobbleType::String
                ) {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Variable `{root}` is type {}, which does not support subscript access",
                            root_type.name()
                        ),
                        help: "Use subscript access only on storage-backed list, map, or string variables."
                            .to_string(),
                    });
                }
                (CobbleType::Unknown, true)
            }
        };

        if !nested_storage_chain
            && !matches!(
                base_type,
                CobbleType::List | CobbleType::Map | CobbleType::String
            )
        {
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

        if base_type == CobbleType::Map {
            if is_quoted_string_literal(index_expression) {
                index = close_index + 1;
                continue;
            }

            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression, type_env)
            {
                if index_type != CobbleType::String && index_type != CobbleType::Unknown {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed map keys must be string values, not {}",
                            index_type.name()
                        ),
                        help: "Use a string variable, function parameter, string literal key, or attribute access for maps, such as `config[key]`, `config[\"chance\"]`, or `config.chance`.".to_string(),
                    });
                }
            }

            if is_constant_storage_index(index_expression, constant_env) {
                return Some(StorageAccessDiagnostic {
                    offset: index + 1,
                    message: "Storage-backed map keys must be strings, not numeric consts"
                        .to_string(),
                    help: "Use a string variable, string literal key, or attribute access for maps, such as `config[key]`, `config[\"chance\"]`, or `config.chance`.".to_string(),
                });
            }

            index = close_index + 1;
            continue;
        }

        if nested_storage_chain {
            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression, type_env)
            {
                if !matches!(
                    index_type,
                    CobbleType::Integer | CobbleType::String | CobbleType::Unknown
                ) {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed indexes must be integer or string values, not {}",
                            index_type.name()
                        ),
                        help: "Use integer scores for list/string indexes or string values for map keys in storage-backed chains.".to_string(),
                    });
                }
            }
            index = close_index + 1;
            continue;
        }

        let is_static_index = is_literal_storage_index(index_expression)
            || is_constant_storage_index(index_expression, constant_env);
        if !is_static_index {
            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression, type_env)
            {
                if index_type != CobbleType::Integer {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed list indexes must be integer scores, not {}",
                            index_type.name()
                        ),
                        help: "Use an integer score expression for runtime list/string indexes, such as `items[i]` after `i = 0`.".to_string(),
                    });
                }
            }
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
        let Some((_field, field_end)) = identifier_after(expression, index + 1) else {
            index += 1;
            continue;
        };
        if matches!(next_non_whitespace_byte(expression, field_end), Some(b'(')) {
            index = field_end;
            continue;
        }
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

fn storage_chain_root_before_subscript(text: &str, base_start: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = base_start;
    let mut chain_start = base_start;

    loop {
        let mut dot = cursor;
        while dot > 0 && bytes[dot - 1].is_ascii_whitespace() {
            dot -= 1;
        }
        if dot == 0 || bytes[dot - 1] != b'.' {
            break;
        }

        let segment_start = storage_chain_segment_start_before(text, dot - 1)?;
        chain_start = segment_start;
        cursor = segment_start;
    }

    if chain_start == base_start {
        return None;
    }
    identifier_after(text, chain_start)
}

fn storage_chain_segment_start_before(text: &str, before_dot: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = before_dot;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }

    if bytes[end - 1] == b']' {
        let open = matching_open_bracket(text, end - 1)?;
        let (_name, start) = identifier_before(text, open)?;
        return Some(start);
    }

    let (_name, start) = identifier_before(text, end)?;
    Some(start)
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

fn matching_open_bracket(expression: &str, close_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().take(close_index + 1).rev() {
        match byte {
            b']' => depth += 1,
            b'[' => {
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
    is_numeric_literal(index_expression) || is_quoted_string_literal(index_expression)
}

fn is_quoted_string_literal(index_expression: &str) -> bool {
    let index_expression = index_expression.trim();
    index_expression.len() >= 2
        && matches!(index_expression.as_bytes()[0], b'"' | b'\'')
        && index_expression
            .as_bytes()
            .last()
            .is_some_and(|last| *last == index_expression.as_bytes()[0])
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
                "%" if (right as i32) != 0 => Some(((left as i32) % (right as i32)) as f64),
                // Negative constant exponents cannot be folded to an integer
                // result; leave them unfolded instead of casting through u32.
                "^" if right >= 0.0 => Some((left as i32).checked_pow(right as u32)? as f64),
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
    let mut delimiter_depth = 0usize;

    for (index, ch) in expression.char_indices().rev() {
        match ch {
            ')' | ']' | '}' => {
                delimiter_depth += 1;
                continue;
            }
            '(' | '[' | '{' => {
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

    if let Some(compound) = compound_assignment_span(masked_trimmed) {
        if !is_simple_module_name(&compound.target) {
            return None;
        }

        let raw_trimmed = line.trim_start();
        let raw_rhs = raw_trimmed.get(compound.value_offset..)?;
        let stripped_rhs = strip_comment_preserving_strings(raw_rhs);
        let value = stripped_rhs.trim();
        if value.is_empty() {
            return None;
        }

        return Some(TypeAssignmentSpan {
            target: compound.target.clone(),
            value: format!("{} {} ({})", compound.target, compound.operator, value),
            value_offset: compound.target_offset,
            column: indent + column_from_byte(masked_trimmed, compound.target_offset),
            is_const: false,
        });
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
        column: indent + column_from_byte(masked_trimmed, raw_target.find(target).unwrap_or(0)),
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
    if expression.starts_with("len(") && function_calls.len() == 1 {
        let call = &function_calls[0];
        if call.name == "len"
            && call.arg_count == 1
            && expression_references_are_known(expression, type_env)
        {
            return Some(CobbleType::Integer);
        }
    }
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
            if let Some(signature) = parse_function_signature(trimmed, line_number, indent) {
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

pub(super) fn raw_command_placeholder_locations(source: &str, name: &str) -> Vec<(usize, usize)> {
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
        && !content.contains('=')
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

    if let Some(compound) = compound_assignment_span(trimmed) {
        if is_simple_module_name(&compound.target) {
            return Some(compound.target);
        }
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssignmentRhsSpan {
    text: String,
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CompoundAssignmentSpan {
    target: String,
    target_offset: usize,
    operator: &'static str,
    value: String,
    value_offset: usize,
}

/// Treats `return <expression>` like an assignment RHS so expression-position
/// checks can report at the `return` site. Offsets stay relative to `trimmed`.
fn return_expression_span(trimmed: &str) -> Option<AssignmentRhsSpan> {
    if !starts_with_keyword(trimmed, "return") {
        return None;
    }

    let rest = &trimmed["return".len()..];
    let offset = "return".len() + (rest.len() - rest.trim_start().len());
    let text = rest.trim();
    if text.is_empty() {
        return None;
    }

    Some(AssignmentRhsSpan {
        text: text.to_string(),
        offset,
    })
}

fn assignment_rhs_span(trimmed: &str) -> Option<AssignmentRhsSpan> {
    if let Some(compound) = compound_assignment_span(trimmed) {
        return Some(AssignmentRhsSpan {
            text: compound.value,
            offset: compound.value_offset,
        });
    }

    let equals_index = single_equals_index(trimmed)?;
    let rhs = &trimmed[equals_index + 1..];
    let trim_start = rhs.len() - rhs.trim_start().len();
    Some(AssignmentRhsSpan {
        text: rhs.trim().to_string(),
        offset: equals_index + 1 + trim_start,
    })
}

fn compound_assignment_span(trimmed: &str) -> Option<CompoundAssignmentSpan> {
    let (operator_index, operator_token, operator) = top_level_compound_assignment(trimmed)?;
    let raw_target = trimmed[..operator_index].trim();
    let target_offset =
        trimmed[..operator_index].len() - trimmed[..operator_index].trim_start().len();
    let value_start = operator_index + operator_token.len();
    let raw_value = trimmed.get(value_start..)?;
    let value_trim_start = raw_value.len() - raw_value.trim_start().len();
    let value = raw_value.trim().to_string();
    if raw_target.is_empty() || value.is_empty() {
        return None;
    }

    Some(CompoundAssignmentSpan {
        target: raw_target.to_string(),
        target_offset,
        operator,
        value,
        value_offset: value_start + value_trim_start,
    })
}

fn top_level_compound_assignment(trimmed: &str) -> Option<(usize, &'static str, &'static str)> {
    let bytes = trimmed.as_bytes();
    let mut delimiter_depth = 0usize;
    let operators = [
        ("+=", "+"),
        ("-=", "-"),
        ("*=", "*"),
        ("/=", "/"),
        ("%=", "%"),
        ("^=", "^"),
    ];

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
            for (token, operator) in operators {
                if trimmed[index..].starts_with(token) {
                    return Some((index, token, operator));
                }
            }
        }

        index += 1;
    }

    None
}

pub(super) fn should_skip_assignment_symbol_scan(trimmed: &str) -> bool {
    // Declarations, control flow, and resource blocks, plus `return`/`global`
    // statements and selector (`@`) lines: none of them define symbols via
    // assignment syntax.
    starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, CONTROL_FLOW_KEYWORDS)
        || starts_with_any_keyword(trimmed, RESOURCE_BLOCK_KEYWORDS)
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "global")
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
            if let Some(signature) = parse_function_signature(trimmed, line_number, indent) {
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
                        column_from_byte(&masked, indent + expression.offset + reference.offset),
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
    if let Some(compound) = compound_assignment_span(trimmed) {
        expressions.push(ExpressionSpan {
            text: compound.target,
            offset: compound.target_offset,
        });
        if !compound.value.is_empty() {
            expressions.push(ExpressionSpan {
                text: compound.value,
                offset: compound.value_offset,
            });
        }
        return expressions;
    }

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

    if starts_with_keyword(trimmed, "return") {
        let rest_offset = "return".len();
        let rest = trimmed[rest_offset..].trim_start();
        let trim_start = trimmed[rest_offset..].len() - rest.len();
        if !rest.is_empty() {
            expressions.push(ExpressionSpan {
                text: rest.to_string(),
                offset: rest_offset + trim_start,
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
    // Declarations and resource blocks plus `global` statements and selector
    // (`@`) lines. Control-flow headers and `return` lines are NOT skipped so
    // the expressions they carry still get reference checks.
    starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, RESOURCE_BLOCK_KEYWORDS)
        || starts_with_keyword(trimmed, "global")
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

/// Builtin names that are not stdlib modules, layered on the shared registry
/// in `crate::stdlib::STDLIB_MODULES`: every non-dotted entry of that table
/// is also a builtin symbol (dotted entries like "score.objective" can never
/// appear as a bare identifier).
pub(crate) const NON_MODULE_BUILTIN_SYMBOLS: &[&str] =
    &["True", "False", "None", "len", "range", "stdlib"];

pub(crate) fn is_builtin_symbol(name: &str) -> bool {
    NON_MODULE_BUILTIN_SYMBOLS.contains(&name)
        || crate::stdlib::STDLIB_MODULES
            .iter()
            .any(|module| !module.contains('.') && *module == name)
}

pub(super) fn should_skip_call_argument_scan(trimmed: &str) -> bool {
    // Declarations and resource blocks plus raw-command (`/`) and selector
    // (`@`) lines. Unlike the assignment scans this keeps `global` lines
    // scannable, and `return` lines are scanned so calls inside a returned
    // expression get arity/existence checks at the `return` site.
    trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('@')
        || starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, RESOURCE_BLOCK_KEYWORDS)
}

fn check_comprehension(masked: &str, line_number: usize, diagnostics: &mut Vec<SourceDiagnostic>) {
    for (open, close) in [('[', ']'), ('{', '}')] {
        if let Some(index) = comprehension_for_index(masked, open, close) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unsupported-comprehension",
                    line_number,
                    column_from_byte(masked, index),
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
                        indent + column_from_byte(trimmed, call.offset),
                        format!("datapack.{resource_type}() takes 2 arguments"),
                    )
                    .with_help("Pass a resource name and an object JSON value."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    line,
                    trimmed,
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
                        indent + column_from_byte(trimmed, json_arg.offset),
                        format!("datapack.{resource_type}() JSON value must be an object"),
                    )
                    .with_help("Use an object literal such as `{ \"condition\": \"...\" }`."),
                );
            }
            continue;
        }

        if let Some(tag_type) = datapack_tag_type(helper) {
            if !(2..=3).contains(&call.arg_count) {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line_number,
                        indent + column_from_byte(trimmed, call.offset),
                        format!("datapack.{tag_type}_tag() takes 2 or 3 arguments"),
                    )
                    .with_help(
                        "Pass a tag name, an array of resource IDs, and optional replace boolean.",
                    ),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    line,
                    trimmed,
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
                        indent + column_from_byte(trimmed, values_arg.offset),
                        "Tag values must be an array",
                    )
                    .with_help("Use an array literal such as `[\"minecraft:stone\"]`."),
                );
            } else {
                check_datapack_tag_value_resource_ids(
                    line,
                    trimmed,
                    raw_trimmed,
                    values_arg,
                    line_number,
                    indent,
                    diagnostics,
                );
            }

            if let Some(replace_arg) = call.arguments.get(2) {
                let replace = replace_arg.text.trim();
                if replace != "True" && replace != "False" {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "datapack-resource-argument",
                            line_number,
                            indent + column_from_byte(trimmed, replace_arg.offset),
                            "Tag replace must be a literal boolean",
                        )
                        .with_help("Use `True` to replace vanilla tag values or `False` to preserve the explicit setting."),
                    );
                }
            }
        }
    }
}

fn check_datapack_tag_value_resource_ids(
    line: &str,
    trimmed: &str,
    raw_trimmed: &str,
    argument: &FunctionCallArgumentSpan,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // `argument.offset` and `argument.text` describe the masked `trimmed`;
    // map both span ends to raw byte offsets so a non-ASCII string earlier on
    // the line (for example a Korean tag name) cannot drift the sub-slice or
    // the reported columns.
    let raw_offset = raw_offset_from_masked(raw_trimmed, trimmed, argument.offset);
    let raw_end =
        raw_offset_from_masked(raw_trimmed, trimmed, argument.offset + argument.text.len());
    let raw_argument = raw_trimmed
        .get(raw_offset..raw_end)
        .unwrap_or(&argument.text);

    for item in array_literal_item_spans(raw_argument) {
        if item.text.starts_with('{') {
            for diagnostic in datapack_tag_object_diagnostics(&item.text) {
                diagnostics.push(
                    SourceDiagnostic::error(
                        diagnostic.kind,
                        line_number,
                        column_from_byte(
                            line,
                            indent + raw_offset + item.offset + diagnostic.offset,
                        ),
                        diagnostic.message,
                    )
                    .with_help(diagnostic.help),
                );
            }
            continue;
        }

        if !item.text.starts_with('"') && !item.text.starts_with('\'') {
            diagnostics.push(
                SourceDiagnostic::error(
                    "datapack-resource-argument",
                    line_number,
                    column_from_byte(line, indent + raw_offset + item.offset),
                    "Tag values must be string resource IDs or object entries",
                )
                .with_help(
                    "Use string values such as `[\"minecraft:stone\"]` or object entries such as `[{\"id\": \"minecraft:stone\", \"required\": False}]`.",
                ),
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
                    indent + raw_offset + item.offset + 1 + diagnostic.offset,
                ),
                diagnostic.message,
            )
            .with_help(diagnostic.help),
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn check_datapack_resource_id_argument(
    line: &str,
    trimmed: &str,
    raw_trimmed: &str,
    argument: &FunctionCallArgumentSpan,
    label: &str,
    line_number: usize,
    indent: usize,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // `argument.offset` is a byte offset into the masked `trimmed`; map it to
    // the raw text before slicing or reporting so a non-ASCII string earlier
    // on the line cannot drift the lookup or the column.
    let raw_offset = raw_offset_from_masked(raw_trimmed, trimmed, argument.offset);
    let Some(value) = quoted_string_literal_at(raw_trimmed, raw_offset) else {
        return;
    };
    let Some(diagnostic) = validate_datapack_resource_id(label, value) else {
        return;
    };

    diagnostics.push(
        SourceDiagnostic::error(
            "datapack-resource-id",
            line_number,
            column_from_byte(line, indent + raw_offset + 1 + diagnostic.offset),
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

struct TagObjectDiagnostic {
    kind: &'static str,
    message: String,
    help: String,
    offset: usize,
}

fn datapack_tag_object_diagnostics(text: &str) -> Vec<TagObjectDiagnostic> {
    let Some(Expression::Map(entries)) = parse_diagnostic_expression(text) else {
        return vec![TagObjectDiagnostic {
            kind: "datapack-resource-argument",
            message: "Tag object values must be valid object entries".to_string(),
            help:
                "Use object entries such as `{\"id\": \"minecraft:stone\", \"required\": False}`."
                    .to_string(),
            offset: 0,
        }];
    };

    let mut diagnostics = Vec::new();
    let mut id = None;
    let mut required = None;

    for (key, value) in &entries {
        match key.as_str() {
            "id" => id = Some(value),
            "required" => required = Some(value),
            _ => diagnostics.push(TagObjectDiagnostic {
                kind: "datapack-resource-argument",
                message: "Tag object values may only contain \"id\" and \"required\" fields"
                    .to_string(),
                help:
                    "Remove extra fields or use a string tag entry such as `\"minecraft:stone\"`."
                        .to_string(),
                offset: object_field_key_offset(text, key).unwrap_or(0),
            }),
        }
    }

    match id {
        Some(Expression::String(value)) => {
            if let Some(diagnostic) = validate_datapack_resource_id("tag value", value) {
                let value_offset = object_field_value_offset(text, "id").unwrap_or(0);
                let quote_offset = text
                    .get(value_offset..)
                    .and_then(|rest| rest.chars().next())
                    .filter(|quote| matches!(quote, '"' | '\''))
                    .map_or(0, char::len_utf8);
                diagnostics.push(TagObjectDiagnostic {
                    kind: "datapack-resource-id",
                    message: diagnostic.message,
                    help: diagnostic.help,
                    offset: value_offset + quote_offset + diagnostic.offset,
                });
            }
        }
        Some(_) | None => diagnostics.push(TagObjectDiagnostic {
            kind: "datapack-resource-argument",
            message: "Tag object values must include a string \"id\" resource ID".to_string(),
            help:
                "Use object entries such as `{\"id\": \"minecraft:stone\", \"required\": False}`."
                    .to_string(),
            offset: object_field_value_offset(text, "id").unwrap_or(0),
        }),
    }

    if let Some(value) = required {
        if !matches!(value, Expression::Boolean(_)) {
            diagnostics.push(TagObjectDiagnostic {
                kind: "datapack-resource-argument",
                message: "Tag object value \"required\" must be a boolean".to_string(),
                help: "Use `required: True` or `required: False`.".to_string(),
                offset: object_field_value_offset(text, "required").unwrap_or(0),
            });
        }
    }

    diagnostics
}

fn parse_diagnostic_expression(text: &str) -> Option<Expression> {
    let source = format!("__cobble_diagnostic_value = {}\n", text);
    let program = parse(&source).ok()?;
    match program.statements.into_iter().next()? {
        Statement::Assignment(assignment) => Some(assignment.value),
        Statement::Expression(expression) => Some(expression),
        _ => None,
    }
}

fn object_field_key_offset(text: &str, key: &str) -> Option<usize> {
    for pattern in [
        format!("\"{}\"", key),
        format!("'{}'", key),
        key.to_string(),
    ] {
        if let Some(offset) = text.find(&pattern) {
            return Some(offset);
        }
    }
    None
}

fn object_field_value_offset(text: &str, key: &str) -> Option<usize> {
    let key_offset = object_field_key_offset(text, key)?;
    let colon_offset = text.get(key_offset..)?.find(':')? + key_offset;
    let value_start = colon_offset + ':'.len_utf8();
    let whitespace = text
        .get(value_start..)?
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    Some(value_start + whitespace)
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
    // Every statement-leading keyword: declarations, control flow (plus
    // `else`/`match`/`case`), resource blocks, `return`, `global`, `pass`,
    // and `const`, alongside raw-command/selector/docstring/assignment lines.
    trimmed.is_empty()
        || trimmed.starts_with('/')
        || trimmed.starts_with('@')
        || trimmed.ends_with(':')
        || raw_trimmed.starts_with('"')
        || raw_trimmed.starts_with('\'')
        || starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, CONTROL_FLOW_KEYWORDS)
        || starts_with_any_keyword(trimmed, RESOURCE_BLOCK_KEYWORDS)
        || starts_with_keyword(trimmed, "else")
        || starts_with_keyword(trimmed, "match")
        || starts_with_keyword(trimmed, "case")
        || starts_with_keyword(trimmed, "return")
        || starts_with_keyword(trimmed, "global")
        || starts_with_keyword(trimmed, "pass")
        || starts_with_keyword(trimmed, "const")
        || compound_assignment_span(trimmed).is_some()
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
            if !(2..=3).contains(&call.arg_count) {
                let location = expression.location_at(call.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        location.line,
                        location.column,
                        format!("datapack.{tag_type}_tag() takes 2 or 3 arguments"),
                    )
                    .with_help(
                        "Pass a tag name, an array of resource IDs, and optional replace boolean.",
                    ),
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

            if let Some(replace_arg) = call.arguments.get(2) {
                let replace = replace_arg.text.trim();
                if replace != "True" && replace != "False" {
                    let location = expression.location_at(replace_arg.offset);
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "datapack-resource-argument",
                            location.line,
                            location.column,
                            "Tag replace must be a literal boolean",
                        )
                        .with_help("Use `True` to replace vanilla tag values or `False` to preserve the explicit setting."),
                    );
                }
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
        if item.text.starts_with('{') {
            for diagnostic in datapack_tag_object_diagnostics(&item.text) {
                let location = expression.location_at(item_offset + diagnostic.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        diagnostic.kind,
                        location.line,
                        location.column,
                        diagnostic.message,
                    )
                    .with_help(diagnostic.help),
                );
            }
            continue;
        }

        if !item.text.starts_with('"') && !item.text.starts_with('\'') {
            let location = expression.location_at(item_offset);
            diagnostics.push(
                SourceDiagnostic::error(
                    "datapack-resource-argument",
                    location.line,
                    location.column,
                    "Tag values must be string resource IDs or object entries",
                )
                .with_help(
                    "Use string values such as `[\"minecraft:stone\"]` or object entries such as `[{\"id\": \"minecraft:stone\", \"required\": False}]`.",
                ),
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
pub(super) struct FunctionCallSpan {
    pub(super) name: String,
    pub(super) offset: usize,
    pub(super) arg_count: usize,
    pub(super) arguments: Vec<FunctionCallArgumentSpan>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FunctionCallArgumentSpan {
    pub(super) text: String,
    pub(super) offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct FunctionSignature {
    pub(super) name: String,
    pub(super) params: Vec<String>,
    pub(super) line: usize,
    pub(super) column: usize,
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

pub(super) fn function_calls_in_expression(expression: &str) -> Vec<FunctionCallSpan> {
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

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueFunctionDiagnostic {
    kind: &'static str,
    message: String,
    help: &'static str,
}

fn invalid_value_function_call(call: &FunctionCallSpan) -> Option<ValueFunctionDiagnostic> {
    if call.name == "len" {
        if call.arg_count != 1 {
            return Some(ValueFunctionDiagnostic {
                kind: "function-argument-count",
                message: format!("len() takes 1 argument, but {} provided", call.arg_count),
                help: "Use len(value) with a string, list, map, or storage-backed value.",
            });
        }
        return None;
    }

    invalid_math_value_function_call(call)
}

fn invalid_math_value_function_call(call: &FunctionCallSpan) -> Option<ValueFunctionDiagnostic> {
    if !call.name.starts_with("math.") {
        return None;
    }

    let Some(arity) = math_value_function_arity(&call.name) else {
        return Some(ValueFunctionDiagnostic {
            kind: "undefined-function",
            message: format!("Unknown math function `{}`", call.name),
            help: "Use one of the supported math intrinsics: math.sqrt, math.abs, math.min, or math.max.",
        });
    };

    if call.arg_count != arity {
        return Some(ValueFunctionDiagnostic {
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
    name == "len" || math_value_function_arity(name).is_some()
}

fn is_control_word(name: &str) -> bool {
    matches!(name, "if" | "for" | "while" | "match" | "return")
}

pub(super) fn looks_like_selector_definition(trimmed: &str) -> bool {
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

fn is_simple_module_name(module: &str) -> bool {
    let mut chars = module.chars();
    let Some(first) = chars.next() else {
        return false;
    };

    (first.is_ascii_alphabetic() || first == '_') && chars.all(is_ident_char)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::{Path, PathBuf};
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

    #[test]
    fn negative_constant_exponents_do_not_fold() {
        let constant_env = HashMap::new();
        // A negative exponent used to be cast through `as u32` (yielding 0 and
        // folding the expression to 1); it must not fold at all.
        assert_eq!(
            evaluate_numeric_const_for_diagnostics("2 ^ (0 - 1)", &constant_env),
            None
        );
        assert_eq!(
            evaluate_numeric_const_for_diagnostics("2 ^ 3", &constant_env),
            Some(8.0)
        );
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
    fn allows_compound_assignment_statements() {
        let diagnostics = analyze_source(
            r#"
def main():
    score = 1
    other = 2
    score += other
    score -= 1
    score *= 2
    score /= 2
    score %= 2
    score ^= other
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
    a, b = 2
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
            .any(|m| m.contains("Unsupported assignment target")));
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
    fn numeric_const_diagnostics_ignore_unicode_string_constants() {
        let source = r#"
const LABEL = "é"

def main():
    config = {"chance": 1}
    first = config[0]
"#;

        let diagnostics =
            parse_source(source).expect_err("numeric map key should still report a diagnostic");

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "unsupported-storage-access"));
    }

    #[test]
    fn numeric_const_diagnostics_ignore_fractional_modulo_divisor() {
        let source = r#"
const INDEX = 1 % 0.5

def main():
    config = {"chance": 1}
    first = config[INDEX]
"#;

        let diagnostics = parse_source(source)
            .expect_err("non-evaluable map key should still report a diagnostic");

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "unsupported-storage-access"));
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
    fn reports_undefined_function_call_expression() {
        let diagnostics = analyze_source(
            r#"
def helper():
    return 1

def main():
    value = missing()
"#,
        );

        assert!(diagnostics
            .iter()
            .any(|diagnostic| diagnostic.kind == "undefined-function"));
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
        let program = parse_source(
            r#"
def main():
    return 1
"#,
        )
        .expect("parse_source should allow return statements");

        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn parse_source_preserves_parser_diagnostic_location() {
        let diagnostics = parse_source("def main():\n    x = \n")
            .expect_err("parse_source should reject incomplete assignments");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "parse");
        assert_eq!(diagnostics[0].line, 2);
        assert_eq!(diagnostics[0].column, 9);
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
    fn reports_datapack_tag_replace_argument_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.block_tag("bad", ["minecraft:stone"], "yes")
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "datapack-resource-argument");
        assert!(diagnostics[0]
            .message
            .contains("Tag replace must be a literal boolean"));
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
    fn allows_datapack_tag_object_values() {
        let diagnostics = analyze_source(
            r#"
datapack.item_tag("ok", [
    "minecraft:diamond",
    {"id": "minecraft:emerald", "required": False},
])
"#,
        );

        assert!(diagnostics.is_empty());
    }

    #[test]
    fn reports_datapack_tag_object_value_diagnostics() {
        let diagnostics = analyze_source(
            r#"
datapack.item_tag("bad", [
    {"id": "minecraft/diamond", "required": False},
    {"required": False},
    {"id": "minecraft:emerald", "required": "no"},
    {"id": "minecraft:gold_ingot", "optional": False},
])
"#,
        );

        assert_eq!(diagnostics.len(), 4);
        assert_eq!(diagnostics[0].kind, "datapack-resource-id");
        assert!(diagnostics[0]
            .message
            .contains("Use 'minecraft:diamond' instead"));
        assert_eq!(diagnostics[1].kind, "datapack-resource-argument");
        assert!(diagnostics[1]
            .message
            .contains("must include a string \"id\""));
        assert_eq!(diagnostics[2].kind, "datapack-resource-argument");
        assert!(diagnostics[2]
            .message
            .contains("\"required\" must be a boolean"));
        assert_eq!(diagnostics[3].kind, "datapack-resource-argument");
        assert!(diagnostics[3]
            .message
            .contains("may only contain \"id\" and \"required\""));
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
    fn allows_selector_score_maps_in_raw_commands() {
        let diagnostics = analyze_source(
            r#"
def tick():
    /execute as @a[scores={demo=1..}] run title @s actionbar {"text":"Running","color":"yellow"}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
    fn allows_nested_function_call_arguments_for_user_functions() {
        let diagnostics = analyze_source(
            r#"
def main():
    greet(make_name())

def make_name():
    return 7

def greet(name):
    /say {name}
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
    config = {"chance": 1}
    i = 0
    first = config[i]
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "unsupported-storage-access");
        assert!(diagnostics[0]
            .message
            .contains("Dynamic storage-backed map keys must be string values"));

        let diagnostics = analyze_source(
            r#"
def main():
    config = {"chance": 1}
    key = "chance"
    first = config[key]
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");

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
    fn allows_storage_backed_subscript_with_runtime_list_index() {
        let diagnostics = analyze_source(
            r#"
def main():
    i = 1
    items = [1, 2, 3]
    first = items[i]
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
    }

    #[test]
    fn reports_storage_access_and_later_type_mismatch_together() {
        let diagnostics = analyze_source(
            r#"
def main():
    config = {"chance": 1}
    i = 0
    first = config[i]
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
    fn parse_source_file_reports_maximum_import_depth() {
        let temp_dir = TempDir::new("maximum-import-depth");
        let main = temp_dir.path().join("main.cbl");
        std::fs::write(&main, "import m1\n\ndef main():\n    pass\n").unwrap();

        for index in 1..=MAX_IMPORT_DEPTH {
            let path = temp_dir.path().join(format!("m{index}.cbl"));
            let source = if index == MAX_IMPORT_DEPTH {
                format!("def m{index}():\n    pass\n")
            } else {
                format!("import m{}\n\ndef m{index}():\n    pass\n", index + 1)
            };
            std::fs::write(path, source).unwrap();
        }

        let diagnostics = parse_source_file(&main).expect_err("deep imports should fail");

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].diagnostics[0].kind, "maximum-import-depth");
        assert!(diagnostics[0].diagnostics[0]
            .message
            .contains("Maximum import depth exceeded (50)"));
        assert!(diagnostics[0].diagnostics[0]
            .help
            .as_deref()
            .unwrap()
            .contains("Import chain:"));
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
    fn python_compat_range_construct_does_not_claim_literal_only_bounds() {
        let observed = python_compat_observed_constructs(
            "def main(limit):\n    for i in range(limit):\n        pass\n",
        );

        assert!(observed.contains(&"for range(...) loops".to_string()));
        assert!(!observed
            .iter()
            .any(|construct| construct.contains("literal bounds")));
    }

    #[test]
    fn python_compat_assignment_target_suggestion_mentions_storage_targets() {
        let suggestion = python_compat_suggestion_for_kind(
            "unsupported-assignment-target",
            "Unsupported assignment target",
            None,
        )
        .unwrap();

        assert!(suggestion.contains("already storage-backed"));
        assert!(suggestion.contains("Destructuring assignment is not supported"));
        assert!(!suggestion.contains("attributes, and subscripts are not assignment targets"));
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

    #[test]
    fn parse_source_file_allows_qualified_imported_function_calls() {
        let temp_dir = TempDir::new("qualified-imported-function");
        let main = temp_dir.path().join("main.cbl");
        let helper = temp_dir.path().join("helper.cbl");
        std::fs::write(
            &main,
            "import helper\n\ndef main():\n    helper.do()\n    value = helper.double(6)\n",
        )
        .unwrap();
        std::fs::write(
            &helper,
            "def do():\n    /say helper\n\ndef double(value):\n    return value * 2\n",
        )
        .unwrap();

        parse_source_file(&main).expect("qualified imported calls should pass diagnostics");
    }
}
