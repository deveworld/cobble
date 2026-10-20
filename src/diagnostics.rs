use crate::ast::Program;
use crate::parser::{parse_located_with_diagnostics, LocatedProgram};
use std::collections::HashSet;

#[path = "diagnostics/imports.rs"]
mod imports;
#[path = "diagnostics/python_compat.rs"]
mod python_compat;
#[path = "diagnostics/reporting.rs"]
mod reporting;
#[path = "diagnostics/semantic.rs"]
mod semantic;
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
    column_from_byte, find_string_end, find_triple_quote, find_word,
    function_name_before_open_paren, is_ident_char, leading_triple_quote, mask_non_code,
    matching_close_delimiter, matching_close_paren, single_equals_index, split_top_level_arg_spans,
    starts_with_keyword, triple_quote_pattern,
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
    let SourceAnalysis {
        diagnostics,
        parse_outcome,
    } = analyze_source_stages(source);
    if !diagnostics.is_empty() {
        return Err(diagnostics);
    }

    match parse_outcome {
        Some(outcome) => outcome,
        // Unreachable in practice: the parse is attempted whenever the
        // structural/indentation bail-outs pass, and those bail-outs always
        // leave `diagnostics` non-empty. Parse again instead of panicking.
        None => parse_located_with_diagnostics(source).map_err(parse_error_diagnostics),
    }
}

pub fn analyze_source(source: &str) -> Vec<SourceDiagnostic> {
    analyze_source_stages(source).diagnostics
}

/// Result of the staged analysis: the accumulated diagnostics plus the
/// single parse attempt the stages performed (`None` when a structural or
/// indentation bail-out returned before the parser could run), so
/// `parse_source_located` never parses the same source twice.
struct SourceAnalysis {
    diagnostics: Vec<SourceDiagnostic>,
    parse_outcome: Option<Result<LocatedProgram, Vec<SourceDiagnostic>>>,
}

fn parse_error_diagnostics(errors: Vec<crate::parser::ParseDiagnostic>) -> Vec<SourceDiagnostic> {
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
}

/// Staged analysis pipeline: pre-parse text checks (structural and
/// indentation bail-outs, then the per-line co-reporting loop), one parse
/// attempt, then the AST-anchored and remaining text-based semantic passes.
/// Semantic passes only run when the source parses; a file that fails to
/// parse surfaces its pre-parse diagnostics, or kind `parse` when there are
/// none (via `parse_source_located`).
fn analyze_source_stages(source: &str) -> SourceAnalysis {
    let mut diagnostics = Vec::new();
    check_structural_syntax(source, &mut diagnostics);
    if !diagnostics.is_empty() {
        return SourceAnalysis {
            diagnostics,
            parse_outcome: None,
        };
    }
    check_indentation_syntax(source, &mut diagnostics);
    if !diagnostics.is_empty() {
        return SourceAnalysis {
            diagnostics,
            parse_outcome: None,
        };
    }

    let mut active_for_blocks: Vec<usize> = Vec::new();
    let mut active_docstring_quote: Option<char> = None;
    let mut multiline_expression_depth = 0usize;

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
        // Comments are masked to spaces, so trim both ends: a header like
        // `def main():  # entry` still ends with `:`.
        let start_trimmed = masked.trim_start();
        let trimmed = start_trimmed.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        if multiline_expression_depth > 0 {
            if !trimmed.starts_with('/') {
                update_delimiter_depth_for_indentation(&masked, &mut multiline_expression_depth);
            }
            continue;
        }

        let indent = masked.len() - start_trimmed.len();
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
        check_function_parameters(trimmed, line_number, indent, &mut diagnostics);
        check_assignment_target(line, trimmed, line_number, indent, &mut diagnostics);
        check_comprehension(&masked, line_number, &mut diagnostics);

        if !trimmed.starts_with('/') {
            update_delimiter_depth_for_indentation(&masked, &mut multiline_expression_depth);
        }
    }

    // The parse is attempted regardless of pre-parse findings so the
    // AST-anchored passes below can still co-report (function-argument-count
    // runs whenever the file parses, even alongside pre-parse diagnostics).
    let parse_outcome = parse_located_with_diagnostics(source).map_err(parse_error_diagnostics);
    let Ok(located) = &parse_outcome else {
        return SourceAnalysis {
            diagnostics,
            parse_outcome: Some(parse_outcome),
        };
    };

    let context = semantic::SemanticContext::new(source, &located.program, &located.spans);
    // These two AST-anchored passes keep their old stage-B loop positions
    // relative to each other and to the datapack/no-op passes below
    // (check_function_parameters ran before check_assignment_function_calls,
    // both before the datapack and no-op scans on each line).
    semantic::check_duplicate_function_parameters(&context, &mut diagnostics);
    semantic::check_assignment_value_function_calls(&context, &mut diagnostics);
    // Single-line datapack call diagnostics co-report with the per-line loop
    // above (the old stage-B position); multi-line-call diagnostics stay
    // behind the old multiline pass's `is_empty` gate below.
    let datapack_arguments = semantic::check_datapack_resource_arguments(&context);
    diagnostics.extend(datapack_arguments.inline);
    semantic::check_noop_expression_statements(&context, &mut diagnostics);
    let function_defs = semantic::collect_function_signatures(&context, &mut diagnostics);

    if diagnostics.is_empty() {
        diagnostics.extend(datapack_arguments.deferred);
    }

    let call_sites = semantic::collect_call_sites(&context);
    semantic::check_user_function_call_arguments(&function_defs, &call_sites, &mut diagnostics);
    if diagnostics.is_empty() && !semantic::has_non_stdlib_imports(&located.program) {
        semantic::check_undefined_function_calls(&function_defs, &call_sites, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        semantic::check_raw_command_placeholders(&context, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        semantic::check_unsupported_none_usage(&context, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        semantic::check_storage_backed_access(&context, &mut diagnostics);
        semantic::check_type_mismatches(&context, &mut diagnostics);
    }
    if diagnostics.is_empty() {
        semantic::check_undefined_variable_references(&context, &function_defs, &mut diagnostics);
    }

    SourceAnalysis {
        diagnostics,
        parse_outcome: Some(parse_outcome),
    }
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
        // Comments are masked to spaces, so trim both ends: a header like
        // `def main():  # entry` still ends with `:`.
        let start_trimmed = masked.trim_start();
        let trimmed = start_trimmed.trim_end();
        if trimmed.is_empty() {
            continue;
        }

        if delimiter_depth == 0 {
            let indent = masked.len() - start_trimmed.len();
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
    // `duplicate-function-parameter` moved to the AST-anchored
    // `semantic::check_duplicate_function_parameters` (duplicate parameters
    // parse; only the grammar-rejected `*`/default forms stay pre-parse).
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
struct CompoundAssignmentSpan {
    target: String,
    target_offset: usize,
    operator: &'static str,
    value: String,
    value_offset: usize,
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
                // Compare bytes: `index` walks raw byte positions, so slicing
                // `trimmed` here would panic inside a multi-byte character.
                // Every token is ASCII, so a match also proves `index` is a
                // char boundary for the callers that slice on it.
                if bytes[index..].starts_with(token.as_bytes()) {
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

fn math_value_function_arity(name: &str) -> Option<usize> {
    match name {
        "math.sqrt" | "math.abs" => Some(1),
        "math.min" | "math.max" => Some(2),
        _ => None,
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
    left.starts_with('@') && left[1..].chars().all(is_ident_char) && right.starts_with('@')
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
    use std::collections::HashMap;
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
            semantic::evaluate_numeric_const_for_diagnostics("2 ^ (0 - 1)", &constant_env),
            None
        );
        assert_eq!(
            semantic::evaluate_numeric_const_for_diagnostics("2 ^ 3", &constant_env),
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
    fn reports_noop_for_parenthesized_and_call_containing_expressions() {
        // The old line scan bailed on any line containing `(`; the AST walk
        // flags every expression statement whose root is not a call.
        let diagnostics = analyze_source(
            r#"
def main():
    (score)
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "no-op-expression");
        assert_eq!(diagnostics[0].line, 3);
        assert_eq!(diagnostics[0].column, 5);

        let diagnostics = analyze_source(
            r#"
def helper(value):
    pass

def main():
    score + helper(1)
"#,
        );

        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].kind, "no-op-expression");
        assert_eq!(diagnostics[0].line, 6);
        assert_eq!(diagnostics[0].column, 5);
    }

    #[test]
    fn does_not_report_noop_for_docstring_prose_statements() {
        // Docstring prose parses into bare identifier statements that the
        // transpiler deliberately drops; the no-op check must stay silent
        // on them, like the old per-line scan's docstring skip.
        let diagnostics = analyze_source(
            r#"
def main():
    """
    helper stuff
    """
    /say hi
"#,
        );

        assert!(diagnostics.is_empty(), "{diagnostics:?}");
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
    fn reports_call_arity_inside_multiline_docstring_bodies() {
        // The parser treats code-shaped lines inside a `"""` block as real
        // statements (the transpiler compiles them), so the AST-anchored
        // arity check reports the call at its exact source position. The
        // old per-line scan skipped docstring bodies and left the same error
        // to the transpiler, which reported it as kind `semantic` at 1:1.
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

        assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
        assert_eq!(diagnostics[0].kind, "function-argument-count");
        assert_eq!((diagnostics[0].line, diagnostics[0].column), (5, 5));
        assert!(diagnostics[0]
            .message
            .contains("Function `helper` expects 2 argument(s), but 1 provided"));
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
