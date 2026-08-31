use crate::diagnostics::{
    byte_offset_for_line_column, DiagnosticSeverity, FileSourceDiagnostics, SourceDiagnostic,
};
use ariadne::{Color, Label, Report, ReportKind, Source};

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
