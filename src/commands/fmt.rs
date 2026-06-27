use super::{find_cobble_files, output_safety::ensure_no_symlink_components};
use crate::config::CobbleConfig;
use crate::diagnostics::{parse_source, FileSourceDiagnostics, SourceDiagnostic};
use crate::error::report_file_source_diagnostics;
use crate::fs_safety::write_file_atomic;
use std::fs;
use std::path::{Path, PathBuf};

pub struct FmtOptions {
    pub input: Option<PathBuf>,
    pub check: bool,
    pub diff: bool,
}

struct FormatCandidate {
    path: PathBuf,
    original: String,
    formatted: String,
}

pub fn format(options: FmtOptions) -> Result<(), String> {
    let (config, config_dir) = if let Some(config_path) = find_config(&options.input) {
        let config = CobbleConfig::load(&config_path)?;
        let config_dir = config_path.parent().unwrap().to_path_buf();
        (Some(config), config_dir)
    } else {
        (
            None,
            std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
        )
    };

    let source_path = if let Some(ref input_path) = options.input {
        input_path.clone()
    } else if let Some(ref cfg) = config {
        config_dir.join(&cfg.build.source)
    } else {
        return Err("No input specified and no cobble.toml found".to_string());
    };

    ensure_no_symlink_components(&source_path, "format source")?;

    let files_to_format = if source_path.is_file() {
        vec![source_path.clone()]
    } else if source_path.is_dir() {
        find_cobble_files(&source_path)?
    } else {
        return Err(format!("Source path does not exist: {:?}", source_path));
    };

    if files_to_format.is_empty() {
        println!("No Cobble files found to format");
        return Ok(());
    }

    let mut candidates = Vec::new();
    let mut diagnostics = Vec::new();

    for path in &files_to_format {
        if let Err(error) = ensure_no_symlink_components(path, "format source") {
            diagnostics.push(FileSourceDiagnostics::new(
                path,
                "",
                vec![SourceDiagnostic::error("source-symlink", 1, 1, error)],
            ));
            continue;
        }

        let source = match fs::read_to_string(path) {
            Ok(source) => source,
            Err(error) => {
                diagnostics.push(FileSourceDiagnostics::new(
                    path,
                    "",
                    vec![SourceDiagnostic::error(
                        "source-read",
                        1,
                        1,
                        format!("Failed to read source file: {error}"),
                    )],
                ));
                continue;
            }
        };

        let formatted = format_source(&source);
        if let Err(source_diagnostics) = parse_source(&formatted) {
            diagnostics.push(FileSourceDiagnostics::new(
                path,
                formatted,
                source_diagnostics,
            ));
            continue;
        }

        candidates.push(FormatCandidate {
            path: path.clone(),
            original: source,
            formatted,
        });
    }

    if !diagnostics.is_empty() {
        report_file_source_diagnostics(&diagnostics);
        let total_errors = diagnostics
            .iter()
            .map(|file| file.diagnostics.len())
            .sum::<usize>();
        return Err(format!(
            "Formatting aborted with {} error(s); no files were written",
            total_errors
        ));
    }

    let changed: Vec<&FormatCandidate> = candidates
        .iter()
        .filter(|candidate| candidate.original != candidate.formatted)
        .collect();

    if options.diff {
        if changed.is_empty() {
            println!("All {} file(s) are formatted", files_to_format.len());
            return Ok(());
        }

        for candidate in &changed {
            print!(
                "{}",
                format_diff(
                    &display_path(&candidate.path, &config_dir),
                    &candidate.original,
                    &candidate.formatted
                )
            );
        }
        return Err(format!(
            "{} file(s) differ from formatter output",
            changed.len()
        ));
    }

    if options.check {
        if changed.is_empty() {
            println!("All {} file(s) are formatted", files_to_format.len());
            return Ok(());
        }

        for candidate in &changed {
            println!(
                "Would reformat {}",
                display_path(&candidate.path, &config_dir)
            );
        }
        return Err(format!("{} file(s) need formatting", changed.len()));
    }

    for candidate in &changed {
        ensure_no_symlink_components(&candidate.path, "format source")?;
    }

    for candidate in &changed {
        write_file_atomic(&candidate.path, &candidate.formatted)
            .map_err(|error| format!("Failed to write {}: {error}", candidate.path.display()))?;
        println!("Formatted {}", display_path(&candidate.path, &config_dir));
    }

    if changed.is_empty() {
        println!(
            "All {} file(s) are already formatted",
            files_to_format.len()
        );
    } else {
        println!("Formatted {} file(s)", changed.len());
    }

    Ok(())
}

fn format_source(source: &str) -> String {
    let source = source.strip_prefix('\u{feff}').unwrap_or(source);
    let mut output = Vec::new();
    let mut indent_stack = vec![0usize];
    let mut previous_code_allows_indent = false;
    let mut active_docstring: Option<ActiveDocstring> = None;

    for raw_line in source.lines() {
        let line = raw_line.strip_suffix('\r').unwrap_or(raw_line);
        let trimmed_end = line.trim_end();

        if let Some(docstring) = active_docstring {
            let content = strip_indent_width(trimmed_end, docstring.raw_indent);
            if content.is_empty() && trimmed_end.trim().is_empty() {
                output.push(String::new());
                previous_code_allows_indent = false;
                continue;
            }

            let mut formatted = "    ".repeat(docstring.logical_level);
            formatted.push_str(content);
            if content.contains(triple_quote_pattern(docstring.quote)) {
                active_docstring = None;
            }
            output.push(formatted);
            previous_code_allows_indent = false;
            continue;
        }

        let trimmed_start = trimmed_end.trim_start_matches([' ', '\t']);

        if trimmed_start.is_empty() {
            output.push(String::new());
            continue;
        }

        let raw_indent = indent_width(trimmed_end);
        let is_comment = trimmed_start.starts_with('#');
        let logical_level = if is_comment {
            comment_indent_level(raw_indent, &indent_stack, previous_code_allows_indent)
        } else {
            update_indent_stack(raw_indent, &mut indent_stack);
            indent_stack.len().saturating_sub(1)
        };

        let mut formatted = "    ".repeat(logical_level);
        formatted.push_str(trimmed_start);
        output.push(formatted);

        if !is_comment {
            if let Some(quote) = leading_triple_quote(trimmed_start) {
                if !has_triple_quote_after_open(trimmed_start, quote) {
                    active_docstring = Some(ActiveDocstring {
                        quote,
                        raw_indent,
                        logical_level,
                    });
                }
            }
            previous_code_allows_indent = trimmed_start.ends_with(':');
        }
    }

    while output.last().is_some_and(|line| line.is_empty()) {
        output.pop();
    }

    let mut formatted = output.join("\n");
    formatted.push('\n');
    formatted
}

fn format_diff(path: &str, original: &str, formatted: &str) -> String {
    let mut output = String::new();
    output.push_str("--- ");
    output.push_str(path);
    output.push('\n');
    output.push_str("+++ ");
    output.push_str(path);
    output.push_str("\n@@\n");

    for line in diff_lines(original, formatted) {
        output.push_str(&line);
        output.push('\n');
    }

    output
}

fn diff_lines(original: &str, formatted: &str) -> Vec<String> {
    let original_lines = original.lines().collect::<Vec<_>>();
    let formatted_lines = formatted.lines().collect::<Vec<_>>();
    let mut lcs = vec![vec![0usize; formatted_lines.len() + 1]; original_lines.len() + 1];

    for original_index in (0..original_lines.len()).rev() {
        for formatted_index in (0..formatted_lines.len()).rev() {
            lcs[original_index][formatted_index] =
                if original_lines[original_index] == formatted_lines[formatted_index] {
                    lcs[original_index + 1][formatted_index + 1] + 1
                } else {
                    lcs[original_index + 1][formatted_index]
                        .max(lcs[original_index][formatted_index + 1])
                };
        }
    }

    let mut output = Vec::new();
    let mut original_index = 0usize;
    let mut formatted_index = 0usize;
    while original_index < original_lines.len() && formatted_index < formatted_lines.len() {
        if original_lines[original_index] == formatted_lines[formatted_index] {
            output.push(format!(" {}", original_lines[original_index]));
            original_index += 1;
            formatted_index += 1;
        } else if lcs[original_index + 1][formatted_index]
            >= lcs[original_index][formatted_index + 1]
        {
            output.push(format!("-{}", original_lines[original_index]));
            original_index += 1;
        } else {
            output.push(format!("+{}", formatted_lines[formatted_index]));
            formatted_index += 1;
        }
    }

    while original_index < original_lines.len() {
        output.push(format!("-{}", original_lines[original_index]));
        original_index += 1;
    }
    while formatted_index < formatted_lines.len() {
        output.push(format!("+{}", formatted_lines[formatted_index]));
        formatted_index += 1;
    }

    output
}

#[derive(Clone, Copy)]
struct ActiveDocstring {
    quote: char,
    raw_indent: usize,
    logical_level: usize,
}

fn update_indent_stack(raw_indent: usize, indent_stack: &mut Vec<usize>) {
    let current_indent = *indent_stack.last().unwrap_or(&0);
    if raw_indent > current_indent {
        indent_stack.push(raw_indent);
        return;
    }

    while indent_stack.len() > 1 && *indent_stack.last().unwrap_or(&0) > raw_indent {
        indent_stack.pop();
    }

    if *indent_stack.last().unwrap_or(&0) != raw_indent {
        indent_stack.push(raw_indent);
        indent_stack.sort_unstable();
        indent_stack.dedup();
    }
}

fn comment_indent_level(
    raw_indent: usize,
    indent_stack: &[usize],
    previous_code_allows_indent: bool,
) -> usize {
    if let Some(level) = indent_stack.iter().position(|indent| *indent == raw_indent) {
        return level;
    }

    let current_level = indent_stack.len().saturating_sub(1);
    let current_indent = *indent_stack.last().unwrap_or(&0);
    if raw_indent > current_indent && previous_code_allows_indent {
        return current_level + 1;
    }

    indent_stack
        .iter()
        .rposition(|indent| *indent < raw_indent)
        .unwrap_or(0)
}

fn strip_indent_width(line: &str, width: usize) -> &str {
    let mut consumed_width = 0usize;
    for (byte_index, ch) in line.char_indices() {
        let char_width = match ch {
            ' ' => 1,
            '\t' => 4,
            _ => return &line[byte_index..],
        };
        if consumed_width + char_width > width {
            return &line[byte_index..];
        }
        consumed_width += char_width;
        if consumed_width == width {
            return &line[byte_index + ch.len_utf8()..];
        }
    }
    ""
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

fn has_triple_quote_after_open(text: &str, quote: char) -> bool {
    text.get(3..)
        .map(|rest| rest.contains(triple_quote_pattern(quote)))
        .unwrap_or(false)
}

fn triple_quote_pattern(quote: char) -> &'static str {
    match quote {
        '"' => "\"\"\"",
        '\'' => "'''",
        _ => unreachable!("unsupported triple quote delimiter"),
    }
}

fn indent_width(line: &str) -> usize {
    line.chars()
        .take_while(|ch| *ch == ' ' || *ch == '\t')
        .map(|ch| if ch == '\t' { 4 } else { 1 })
        .sum()
}

fn display_path(path: &Path, base: &Path) -> String {
    path.strip_prefix(base)
        .unwrap_or(path)
        .to_string_lossy()
        .into_owned()
}

fn find_config(input: &Option<PathBuf>) -> Option<PathBuf> {
    if let Some(path) = input {
        if path.is_file() {
            if let Some(parent) = path.parent() {
                return CobbleConfig::find_in_path(parent);
            }
        } else {
            return CobbleConfig::find_in_path(path);
        }
    }
    CobbleConfig::find_in_path(".")
}

#[cfg(test)]
mod tests {
    use super::{format_diff, format_source};

    #[test]
    fn formats_indentation_trailing_space_and_eof_newline() {
        let source = "def main():  \r\n  # setup  \r\n  /say hello  \r\n\r\n";

        assert_eq!(
            format_source(source),
            "def main():\n    # setup\n    /say hello\n"
        );
    }

    #[test]
    fn preserves_raw_command_payloads_and_comments() {
        let source =
            "def main():\n    /tellraw @a {\"text\":\"Hi\",\"color\":\"green\"}\n    # keep me\n";

        assert_eq!(format_source(source), source);
    }

    #[test]
    fn normalizes_tabs_as_indentation_levels() {
        let source = "def main():\n\t/say tabbed\n";

        assert_eq!(format_source(source), "def main():\n    /say tabbed\n");
    }

    #[test]
    fn strips_bom_and_normalizes_crlf() {
        let source = "\u{feff}def main():\r\n  /say windows\r\n\r\n";

        assert_eq!(format_source(source), "def main():\n    /say windows\n");
    }

    #[test]
    fn preserves_trailing_comments() {
        let source = "const VALUE = 1  # keep trailing comment  \n";

        assert_eq!(
            format_source(source),
            "const VALUE = 1  # keep trailing comment\n"
        );
    }

    #[test]
    fn preserves_multiline_docstring_relative_indentation() {
        let source = "def main():\n  \"\"\"\n  Keep this line.\n    Keep this relative indent.\n  \"\"\"\n  /say done\n";

        assert_eq!(
            format_source(source),
            "def main():\n    \"\"\"\n    Keep this line.\n      Keep this relative indent.\n    \"\"\"\n    /say done\n"
        );
    }

    #[test]
    fn preserves_docstring_blank_lines_without_trailing_spaces() {
        let source = "def main():\n  \"\"\"\n  Before.\n\n  After.\n  \"\"\"\n  /say done\n";

        assert_eq!(
            format_source(source),
            "def main():\n    \"\"\"\n    Before.\n\n    After.\n    \"\"\"\n    /say done\n"
        );
    }

    #[test]
    fn emits_line_diff_for_formatter_changes() {
        let diff = format_diff(
            "src/main.cbl",
            "def main():  \n  /say diff  \n",
            "def main():\n    /say diff\n",
        );

        assert!(diff.contains("--- src/main.cbl\n+++ src/main.cbl\n@@\n"));
        assert!(diff.contains("-def main():  \n"));
        assert!(diff.contains("+def main():\n"));
        assert!(diff.contains("-  /say diff  \n"));
        assert!(diff.contains("+    /say diff\n"));
    }
}
