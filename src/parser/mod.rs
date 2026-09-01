// Parser module structure

// Tokenizer module - handles lexical analysis
pub mod tokenizer;

// Combinators module - handles parsing token streams into AST
mod combinators;

// Re-export public API
pub use combinators::{
    parse, parse_located_with_diagnostics, parse_with_diagnostics, token_parser,
    verify_span_tree_alignment, LocatedProgram, ParseDiagnostic, ProgramSpanTable, StatementSpan,
    StatementSpanBranch, StatementSpanKind, StatementSpanNode, StatementSpanTree,
};
pub use tokenizer::{tokenize, tokenize_spanned, SpannedToken, Token};
