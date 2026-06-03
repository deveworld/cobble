use crate::diagnostics::{
    byte_offset_for_line_column, DiagnosticSeverity, FileSourceDiagnostics, SourceDiagnostic,
};
use ariadne::{Color, Label, Report, ReportKind, Source};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum CobbleError {
    #[error("Parse error")]
    ParseError(String),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Transpilation error: {0}")]
    TranspileError(String),
}

pub fn report_error(filename: &str, src: &str, error: &CobbleError) {
    match error {
        CobbleError::ParseError(msg) => {
            Report::build(ReportKind::Error, (filename, 0..1))
                .with_message("Parse Error")
                .with_label(
                    Label::new((filename, 0..1))
                        .with_message(msg.clone())
                        .with_color(Color::Red),
                )
                .finish()
                .print((filename, Source::from(src)))
                .unwrap();
        }
        CobbleError::TranspileError(msg) => {
            Report::build(ReportKind::Error, (filename, 0..1))
                .with_message("Transpilation Error")
                .with_label(
                    Label::new((filename, 0..1))
                        .with_message(msg.clone())
                        .with_color(Color::Red),
                )
                .finish()
                .print((filename, Source::from(src)))
                .unwrap();
        }
        CobbleError::IoError(e) => {
            eprintln!("IO Error: {}", e);
        }
    }
}

/// Report parse errors with ariadne (for token-based parsing)
pub fn report_parse_errors(filename: &str, src: &str, errors: &[String]) {
    for error in errors {
        eprintln!("Parse error in {}: {}", filename, error);
    }

    // If there's only one error, try to provide a better report
    if errors.len() == 1 {
        Report::build(ReportKind::Error, (filename, 0..src.len().min(1)))
            .with_message("Parse Error")
            .with_note(&errors[0])
            .finish()
            .eprint((filename, Source::from(src)))
            .ok();
    }
}

pub fn report_source_diagnostics(filename: &str, src: &str, diagnostics: &[SourceDiagnostic]) {
    for diagnostic in diagnostics {
        let start = byte_offset_for_line_column(src, diagnostic.line, diagnostic.column);
        let end = (start + 1).min(src.len());
        let color = match diagnostic.severity {
            DiagnosticSeverity::Error => Color::Red,
            DiagnosticSeverity::Warning => Color::Yellow,
        };
        let report_kind = match diagnostic.severity {
            DiagnosticSeverity::Error => ReportKind::Error,
            DiagnosticSeverity::Warning => ReportKind::Warning,
        };

        let mut report = Report::build(report_kind, (filename, start..end))
            .with_message(format!("{}: {}", diagnostic.kind, diagnostic.message))
            .with_label(
                Label::new((filename, start..end))
                    .with_message(diagnostic.message.clone())
                    .with_color(color),
            );

        if let Some(help) = &diagnostic.help {
            report = report.with_note(help);
        }

        report.finish().eprint((filename, Source::from(src))).ok();
    }
}

pub fn report_file_source_diagnostics(diagnostics: &[FileSourceDiagnostics]) {
    for file_diagnostics in diagnostics {
        let filename = file_diagnostics.path.to_string_lossy();
        report_source_diagnostics(
            &filename,
            &file_diagnostics.source,
            &file_diagnostics.diagnostics,
        );
    }
}
