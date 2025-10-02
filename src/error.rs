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
