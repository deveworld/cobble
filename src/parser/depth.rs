//! A nesting limit checked on the tokens before parsing.
//!
//! The parser and every pass after it walk expressions recursively, so very
//! deep input, such as 20,000 nested brackets or a chain of 20,000 `not`s,
//! would overflow the stack and abort the compiler. This check runs without
//! recursion and reports such input as a diagnostic first.

use super::tokenizer::{SpannedToken, Token};
use crate::ast::SourceSpan;

/// The deepest nesting a statement may have: blocks, brackets, operators,
/// attribute and subscript chains counted together.
pub const MAX_NESTING_DEPTH: usize = 1000;

/// One comma- or colon-separated part of a bracket group.
#[derive(Default)]
struct Segment {
    /// Operators, attribute dots, and subscripts or calls in this part.
    operators: usize,
    /// The deepest group nested in this part.
    deepest_child: usize,
}

impl Segment {
    fn depth(&self) -> usize {
        self.operators + self.deepest_child
    }
}

#[derive(Default)]
struct Group {
    current: Segment,
    deepest_segment: usize,
}

impl Group {
    fn end_segment(&mut self) {
        self.deepest_segment = self.deepest_segment.max(self.current.depth());
        self.current = Segment::default();
    }

    fn depth(mut self) -> usize {
        self.end_segment();
        self.deepest_segment
    }
}

/// An upper bound of how deep the parsed tree of each statement nests, as
/// the first statement found past the limit: its first token's span and
/// its estimated depth.
pub fn find_too_deep_statement(tokens: &[SpannedToken]) -> Option<(SourceSpan, usize)> {
    let mut block_depth = 0usize;
    let mut groups: Vec<Group> = vec![Group::default()];
    let mut statement_start: Option<SourceSpan> = None;
    let mut previous: Option<&Token> = None;

    for spanned in tokens {
        let token = &spanned.token;
        let at_top = groups.len() == 1;
        if at_top
            && matches!(
                token,
                Token::Newline | Token::Indent | Token::Dedent | Token::Eof
            )
        {
            let depth = block_depth + std::mem::take(&mut groups[0]).depth();
            if depth > MAX_NESTING_DEPTH {
                return Some((statement_start.unwrap_or(spanned.span), depth));
            }
            match token {
                Token::Indent => block_depth += 1,
                Token::Dedent => block_depth = block_depth.saturating_sub(1),
                _ => {}
            }
            statement_start = None;
            previous = None;
            continue;
        }
        statement_start.get_or_insert(spanned.span);

        // `a[0][1]` and `f(x)(y)` nest each call or subscript in the next.
        let after_postfix = matches!(
            previous,
            Some(Token::RParen | Token::RBracket | Token::RBrace)
        );
        match token {
            Token::FString(text) => {
                let fields = super::strings::format_literal_body(text)
                    .and_then(|(_, body)| super::strings::format_parts(body).ok())
                    .map(|parts| parts.len())
                    .unwrap_or(1000);
                groups.last_mut().expect("never empty").current.operators += fields * 2;
            }
            Token::LParen | Token::LBracket | Token::LBrace => {
                if after_postfix {
                    groups.last_mut().expect("never empty").current.operators += 1;
                }
                groups.push(Group::default());
            }
            Token::RParen | Token::RBracket | Token::RBrace => {
                if groups.len() > 1 {
                    let depth = 1 + groups.pop().expect("checked above").depth();
                    let parent = &mut groups.last_mut().expect("never empty").current;
                    parent.deepest_child = parent.deepest_child.max(depth);
                }
            }
            Token::Comma | Token::Colon | Token::SemiColon | Token::Equals => {
                groups.last_mut().expect("never empty").end_segment();
            }
            Token::Plus
            | Token::Minus
            | Token::Star
            | Token::Slash
            | Token::Percent
            | Token::Caret
            | Token::EqEq
            | Token::NotEq
            | Token::Lt
            | Token::LtEq
            | Token::Gt
            | Token::GtEq
            | Token::And
            | Token::Or
            | Token::Not
            | Token::In
            | Token::If
            | Token::Else
            | Token::Dot => {
                groups.last_mut().expect("never empty").current.operators += 1;
            }
            _ => {}
        }
        previous = Some(token);
    }

    // Input that ends inside brackets.
    let mut depth = 0;
    while let Some(group) = groups.pop() {
        depth = 1 + (group.depth().max(depth));
    }
    let depth = block_depth + depth.saturating_sub(1);
    match (depth > MAX_NESTING_DEPTH, statement_start) {
        (true, Some(span)) => Some((span, depth)),
        _ => None,
    }
}
