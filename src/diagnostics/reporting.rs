use crate::ast::{Program, SourceSpan};
use crate::parser::ProgramSpanTable;
use crate::transpiler::{TranspileError, TranspileErrorKind};
use std::path::PathBuf;

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
    pub spans: ProgramSpanTable,
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

    pub fn error_at_span(
        kind: impl Into<String>,
        span: SourceSpan,
        message: impl Into<String>,
    ) -> Self {
        Self::error(kind, span.line, span.column, message)
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

impl SourceDiagnostic {
    /// Returns true when a transpiler error carries a classification the
    /// reporters render as a source diagnostic (the unroll/runtime-for and
    /// stdlib-module kinds). Callers that fall back to a generic diagnostic
    /// for plain semantic errors gate on this predicate.
    pub fn transpile_error_is_recognized(error: &TranspileError) -> bool {
        error.kind != TranspileErrorKind::Semantic
    }

    /// Recovers a best-effort 1-based source location for a transpiler error:
    /// structured `location:`/`source-key:`/`loop:` trailers for unroll and
    /// runtime-for errors, and a `module '<name>'` scan for stdlib errors.
    pub fn recover_transpile_error_location(
        error: &TranspileError,
        source: &str,
    ) -> Option<(usize, usize)> {
        if error.kind.is_unroll() || error.kind.is_runtime_for() {
            let message = error.message.as_str();
            return unroll_error_location(message)
                .or_else(|| {
                    unroll_error_source_key(message).and_then(|source_key| {
                        find_for_loop_location_for_source_key(source, source_key)
                    })
                })
                .or_else(|| {
                    unroll_error_target(message)
                        .and_then(|target| find_for_loop_location_for_target(source, target))
                })
                .or_else(|| find_for_loop_location(source));
        }

        let module = error
            .message
            .split_once("module '")
            .and_then(|(_, rest)| rest.split_once('\''))
            .map(|(module, _)| module)?;
        let needle = format!("{module}.");
        find_source_location(source, &needle).or_else(|| find_source_location(source, module))
    }

    /// Builds the error diagnostic `cobble check` and the web demo report for
    /// a transpiler error, mapping [`TranspileErrorKind`] onto its diagnostic
    /// slug, recovering a location with
    /// [`Self::recover_transpile_error_location`], and stripping the
    /// machine-oriented `source-key:`/`location:` trailer lines.
    pub fn from_transpile_error(error: &TranspileError, source: &str) -> Self {
        let (line, column) =
            Self::recover_transpile_error_location(error, source).unwrap_or((1, 1));
        Self::error(
            error.kind.slug(),
            line,
            column,
            clean_transpiler_error_message(&error.message),
        )
    }
}

fn clean_transpiler_error_message(error: &str) -> String {
    error
        .lines()
        .filter(|line| {
            let line = line.trim_start();
            !line.starts_with("source-key:") && !line.starts_with("location:")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn unroll_error_target(error: &str) -> Option<&str> {
    error
        .split_once("\n  loop: for ")
        .and_then(|(_, rest)| rest.split_whitespace().next())
}

fn unroll_error_location(error: &str) -> Option<(usize, usize)> {
    let location = error
        .split_once("\n  location: ")
        .and_then(|(_, rest)| rest.lines().next())?
        .trim();
    let (line, column) = location.split_once(':')?;
    Some((line.parse().ok()?, column.parse().ok()?))
}

fn unroll_error_source_key(error: &str) -> Option<&str> {
    error
        .split_once("\n  source-key: ")
        .and_then(|(_, rest)| rest.lines().next())
        .map(str::trim)
        .filter(|source_key| !source_key.is_empty())
}

fn find_source_location(source: &str, needle: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        line.find(needle)
            .map(|column_index| (line_index + 1, column_index + 1))
    })
}

fn find_for_loop_location_for_target(source: &str, target: &str) -> Option<(usize, usize)> {
    let needle = format!("for {target} ");
    source.lines().enumerate().find_map(|(line_index, line)| {
        let column_index = line.find(&needle)?;
        line[column_index..]
            .trim_start()
            .starts_with(&needle)
            .then_some((line_index + 1, column_index + 1))
    })
}

fn find_for_loop_location_for_source_key(source: &str, source_key: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        let statement = line.trim_start();
        if !statement.starts_with("for ") {
            return None;
        }
        if crate::transpiler::Transpiler::source_line_key(statement) != source_key {
            return None;
        }
        let column = line.len() - statement.len() + 1;
        Some((line_index + 1, column))
    })
}

fn find_for_loop_location(source: &str) -> Option<(usize, usize)> {
    source.lines().enumerate().find_map(|(line_index, line)| {
        let column_index = line.find("for ")?;
        line[column_index..]
            .trim_start()
            .starts_with("for ")
            .then_some((line_index + 1, column_index + 1))
    })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transpile_error_kinds_map_onto_diagnostic_slugs() {
        assert_eq!(
            TranspileErrorKind::UnrollLimit.slug(),
            "unroll-limit-exceeded"
        );
        assert_eq!(
            TranspileErrorKind::UnrollNested.slug(),
            "unroll-limit-exceeded"
        );
        assert_eq!(
            TranspileErrorKind::UnrollCommandLimit.slug(),
            "unroll-limit-exceeded"
        );
        assert_eq!(
            TranspileErrorKind::RuntimeForUnsupported.slug(),
            "runtime-for-unsupported"
        );
        assert_eq!(
            TranspileErrorKind::MissingStdlibModule.slug(),
            "missing-stdlib-module"
        );
        assert_eq!(
            TranspileErrorKind::UnknownStdlibModule.slug(),
            "unknown-stdlib-module"
        );
        assert_eq!(TranspileErrorKind::Semantic.slug(), "semantic");
    }

    #[test]
    fn from_transpile_error_recovers_location_and_strips_trailers() {
        let error = TranspileError::new(
            TranspileErrorKind::UnrollLimit,
            "unroll-limit-exceeded: range expands past 1024 iterations.\n  loop: for i\n  location: 2:5",
        );
        let diagnostic = SourceDiagnostic::from_transpile_error(
            &error,
            "def main():\n    for i in range(1025):\n        /say bad {i}\n",
        );

        assert_eq!(diagnostic.kind, "unroll-limit-exceeded");
        assert_eq!((diagnostic.line, diagnostic.column), (2, 5));
        assert_eq!(
            diagnostic.message,
            "unroll-limit-exceeded: range expands past 1024 iterations.\n  loop: for i"
        );
    }

    #[test]
    fn transpile_error_recognition_gates_on_error_kind() {
        assert!(SourceDiagnostic::transpile_error_is_recognized(
            &TranspileError::new(
                TranspileErrorKind::UnrollNonLiteral,
                "unroll-non-literal: dynamic bound"
            )
        ));
        assert!(SourceDiagnostic::transpile_error_is_recognized(
            &TranspileError::new(
                TranspileErrorKind::MissingStdlibModule,
                "module 'text' not imported."
            )
        ));
        assert!(!SourceDiagnostic::transpile_error_is_recognized(
            &TranspileError::from("Transpile exploded for other reasons")
        ));
    }

    #[test]
    fn error_at_span_preserves_existing_line_column_shape() {
        let diagnostic = SourceDiagnostic::error_at_span(
            "parse",
            SourceSpan::new(10, 15, 3, 7),
            "unexpected token",
        );

        assert_eq!(diagnostic.kind, "parse");
        assert_eq!(diagnostic.line, 3);
        assert_eq!(diagnostic.column, 7);
        assert_eq!(diagnostic.message, "unexpected token");
        assert_eq!(diagnostic.severity, DiagnosticSeverity::Error);
    }
}
