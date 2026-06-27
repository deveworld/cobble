use crate::ast::Program;
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
