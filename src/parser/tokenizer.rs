use crate::ast::SourceSpan;

/// Token type for indentation-based parsing
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    // Keywords
    Import,
    From,
    Def,
    If,
    Elif,
    Else,
    For,
    While,
    Return,
    Pass,
    In,
    Global,
    As,
    At,
    Asat,
    And,
    Or,
    Not,
    Unless,
    Match,
    Case,
    Const,
    Define,
    Create,
    End,
    To,
    By,
    Underscore,

    // Literals
    Number(String), // Store as string to avoid f64 Eq/Hash issues
    String(String),
    True_,
    False_,
    None_,

    // Identifiers
    Ident(String),

    // Symbols
    LParen,
    RParen,
    LBracket,
    RBracket,
    LBrace,
    RBrace,
    Colon,
    SemiColon,
    Comma,
    Dot,
    Equals,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Caret,

    // Comparison
    EqEq,
    NotEq,
    Lt,
    LtEq,
    Gt,
    GtEq,

    // Special
    MinecraftCommand(String),
    Newline,
    Indent,
    Dedent,
    Eof,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct SpannedToken {
    pub token: Token,
    pub span: SourceSpan,
}

impl SpannedToken {
    fn new(token: Token, start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            token,
            span: SourceSpan::new(start, end, line, column),
        }
    }
}

impl std::fmt::Display for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Token::Ident(s) => write!(f, "{}", s),
            Token::String(s) => write!(
                f,
                "{}",
                serde_json::to_string(s).unwrap_or_else(|_| format!("\"{}\"", s))
            ),
            Token::Number(n) => write!(f, "{}", n),
            Token::MinecraftCommand(s) => write!(f, "/{}", s),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::SemiColon => write!(f, ";"),
            Token::Comma => write!(f, ","),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
            Token::LBrace => write!(f, "{{"),
            Token::RBrace => write!(f, "}}"),
            Token::Plus => write!(f, "+"),
            Token::Minus => write!(f, "-"),
            Token::Star => write!(f, "*"),
            Token::Slash => write!(f, "/"),
            Token::Percent => write!(f, "%"),
            Token::Caret => write!(f, "^"),
            Token::Equals => write!(f, "="),
            Token::EqEq => write!(f, "=="),
            Token::NotEq => write!(f, "!="),
            Token::Lt => write!(f, "<"),
            Token::LtEq => write!(f, "<="),
            Token::Gt => write!(f, ">"),
            Token::GtEq => write!(f, ">="),
            // Keywords - must be lowercase for Minecraft compatibility
            Token::If => write!(f, "if"),
            Token::Unless => write!(f, "unless"),
            Token::As => write!(f, "as"),
            Token::At => write!(f, "at"),
            Token::And => write!(f, "and"),
            Token::Or => write!(f, "or"),
            Token::Not => write!(f, "not"),
            Token::In => write!(f, "in"),
            Token::For => write!(f, "for"),
            Token::While => write!(f, "while"),
            Token::Elif => write!(f, "elif"),
            Token::Else => write!(f, "else"),
            Token::Def => write!(f, "def"),
            Token::Return => write!(f, "return"),
            Token::Pass => write!(f, "pass"),
            Token::Global => write!(f, "global"),
            Token::Import => write!(f, "import"),
            Token::From => write!(f, "from"),
            Token::Asat => write!(f, "asat"),
            Token::Match => write!(f, "match"),
            Token::Case => write!(f, "case"),
            Token::Const => write!(f, "const"),
            Token::Define => write!(f, "define"),
            Token::Create => write!(f, "create"),
            Token::End => write!(f, "end"),
            Token::To => write!(f, "to"),
            Token::By => write!(f, "by"),
            Token::Underscore => write!(f, "_"),
            Token::True_ => write!(f, "True"),
            Token::False_ => write!(f, "False"),
            Token::None_ => write!(f, "None"),
            _ => write!(f, "{:?}", self),
        }
    }
}

/// Manual tokenizer that handles indentation
pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    Ok(tokenize_spanned(source)?
        .into_iter()
        .map(|token| token.token)
        .collect())
}

/// Manual tokenizer that preserves source byte spans for each token.
pub fn tokenize_spanned(source: &str) -> Result<Vec<SpannedToken>, String> {
    let mut tokens = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut paren_depth = 0;

    for (line_idx, (line_start, line)) in source_lines_with_offsets(source).into_iter().enumerate()
    {
        // Skip empty lines and comments
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        // Only handle indentation if we are not inside parentheses/brackets/braces
        if paren_depth == 0 {
            // Calculate indentation
            let indent_level = line.len() - line.trim_start().len();
            let current_indent = *indent_stack.last().unwrap();

            // Handle indentation changes
            if indent_level > current_indent {
                indent_stack.push(indent_level);
                push_token(
                    &mut tokens,
                    Token::Indent,
                    line_start + indent_level,
                    line_start + indent_level,
                    line_idx + 1,
                    indent_level + 1,
                );
            } else if indent_level < current_indent {
                while indent_stack.len() > 1 && *indent_stack.last().unwrap() > indent_level {
                    indent_stack.pop();
                    push_token(
                        &mut tokens,
                        Token::Dedent,
                        line_start + indent_level,
                        line_start + indent_level,
                        line_idx + 1,
                        indent_level + 1,
                    );
                }
                if *indent_stack.last().unwrap() != indent_level {
                    return Err(format!("Indentation error at line {}", line_idx + 1));
                }
            }
        }

        // Tokenize the line content
        let line_content = line.trim();
        let content_offset = line.find(line_content).unwrap_or(0);
        tokenize_line(
            line_content,
            line_start + content_offset,
            content_offset,
            line_idx + 1,
            &mut tokens,
            &mut paren_depth,
        )?;

        // Only emit Newline if not inside parentheses/brackets/braces
        if paren_depth == 0 {
            push_token(
                &mut tokens,
                Token::Newline,
                line_start + line.len(),
                line_start + line.len(),
                line_idx + 1,
                line.chars().count() + 1,
            );
        }
    }

    // Add remaining dedents
    while indent_stack.len() > 1 {
        indent_stack.pop();
        push_token(&mut tokens, Token::Dedent, source.len(), source.len(), 1, 1);
    }

    push_token(&mut tokens, Token::Eof, source.len(), source.len(), 1, 1);
    Ok(tokens)
}

fn push_token(
    tokens: &mut Vec<SpannedToken>,
    token: Token,
    start: usize,
    end: usize,
    line: usize,
    column: usize,
) {
    tokens.push(SpannedToken::new(token, start, end, line, column));
}

fn source_lines_with_offsets(source: &str) -> Vec<(usize, &str)> {
    let mut lines = Vec::new();
    let mut offset = 0;

    for segment in source.split_inclusive('\n') {
        let mut line = segment.strip_suffix('\n').unwrap_or(segment);
        if let Some(without_cr) = line.strip_suffix('\r') {
            line = without_cr;
        }
        lines.push((offset, line));
        offset += segment.len();
    }

    lines
}

/// Check if the minus sign should be treated as a binary operator
/// based on the previous token context
fn should_be_binary_minus(tokens: &[SpannedToken]) -> bool {
    // If previous token is one of these, minus is a binary operator:
    // Number, Ident, RParen, RBracket, True_, False_, None_
    if let Some(last_token) = tokens.last().map(|token| &token.token) {
        matches!(
            last_token,
            Token::Number(_)
                | Token::Ident(_)
                | Token::RParen
                | Token::RBracket
                | Token::True_
                | Token::False_
                | Token::None_
        )
    } else {
        // At start of line or after operators/keywords, it's unary
        false
    }
}

/// Check if the caret should be treated as a power operator
/// based on the previous token context
fn should_be_power_operator(tokens: &[SpannedToken]) -> bool {
    // Similar to should_be_binary_minus - if previous token can be an operand,
    // then ^ is the power operator, not a coordinate marker
    if let Some(last_token) = tokens.last().map(|token| &token.token) {
        matches!(
            last_token,
            Token::Number(_)
                | Token::Ident(_)
                | Token::RParen
                | Token::RBracket
                | Token::True_
                | Token::False_
                | Token::None_
        )
    } else {
        false
    }
}

/// Tokenize a single line
fn tokenize_line(
    line: &str,
    line_offset: usize,
    column_offset: usize,
    line_number: usize,
    tokens: &mut Vec<SpannedToken>,
    paren_depth: &mut i32,
) -> Result<(), String> {
    let mut chars = line.char_indices().peekable();

    while let Some(&(start, ch)) = chars.peek() {
        match ch {
            ' ' | '\t' => {
                chars.next();
            }
            '/' => {
                // Check if this is a Minecraft command (starts with / followed by letter)
                // or a division operator
                chars.next();
                if let Some(&(next_index, next_ch)) = chars.peek() {
                    // Minecraft command only if followed immediately by a letter (no space)
                    if next_ch.is_alphabetic() {
                        // Minecraft command - consume rest of line
                        let rest = &line[next_index..];
                        let stripped = strip_minecraft_inline_comment(rest).trim_end();
                        push_token(
                            tokens,
                            Token::MinecraftCommand(stripped.to_string()),
                            line_offset + start,
                            line_offset + next_index + stripped.len(),
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                        break;
                    } else {
                        // Division operator or other use
                        push_token(
                            tokens,
                            Token::Slash,
                            line_offset + start,
                            line_offset + start + ch.len_utf8(),
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                    }
                } else {
                    // End of line after /, treat as Slash
                    push_token(
                        tokens,
                        Token::Slash,
                        line_offset + start,
                        line_offset + start + ch.len_utf8(),
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '"' | '\'' => {
                // String literal
                let (quote_index, quote) = chars.next().unwrap();
                let mut s = String::new();
                let mut escaped = false;
                let mut end = quote_index + quote.len_utf8();
                for (index, ch) in chars.by_ref() {
                    end = index + ch.len_utf8();
                    if escaped {
                        s.push(ch);
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == quote {
                        break;
                    } else {
                        s.push(ch);
                    }
                }
                push_token(
                    tokens,
                    Token::String(s),
                    line_offset + start,
                    line_offset + end,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '0'..='9' => {
                // Number
                let mut num = String::new();
                let mut end = start;
                while let Some(&(index, ch)) = chars.peek() {
                    if ch.is_ascii_digit() {
                        let (_, consumed) = chars.next().unwrap();
                        num.push(consumed);
                        end = index + consumed.len_utf8();
                    } else if ch == '.' {
                        // Check if this is a range operator (..)
                        let mut temp_chars = chars.clone();
                        temp_chars.next(); // skip first dot
                        if let Some(&(_, next_ch)) = temp_chars.peek() {
                            if next_ch == '.' {
                                // This is "..", stop parsing number
                                break;
                            }
                        }
                        // Single dot, part of decimal number
                        let (_, consumed) = chars.next().unwrap();
                        num.push(consumed);
                        end = index + consumed.len_utf8();
                    } else {
                        break;
                    }
                }
                // Validate that the number can be parsed
                if num.parse::<f64>().is_err() {
                    return Err(format!(
                        "Invalid number literal: '{}' at line {}",
                        num, line
                    ));
                }
                push_token(
                    tokens,
                    Token::Number(num),
                    line_offset + start,
                    line_offset + end,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                // Identifier or keyword (may include namespace like minecraft:stone)
                let mut ident = String::new();
                let mut end = start;
                while let Some(&(index, ch)) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        let (_, consumed) = chars.next().unwrap();
                        ident.push(consumed);
                        end = index + consumed.len_utf8();
                    } else if ch == ':' {
                        // Check if this is a namespace separator (followed by identifier)
                        let mut temp_chars = chars.clone();
                        temp_chars.next(); // skip the colon
                        if let Some(&(_, next_ch)) = temp_chars.peek() {
                            if next_ch.is_alphabetic() || next_ch == '_' {
                                // This is a namespace separator
                                let (_, consumed) = chars.next().unwrap();
                                ident.push(consumed); // add the colon
                                end = index + consumed.len_utf8();
                                continue;
                            }
                        }
                        // Not a namespace separator, stop here
                        break;
                    } else {
                        break;
                    }
                }
                let token = match ident.as_str() {
                    "import" => Token::Import,
                    "from" => Token::From,
                    "def" => Token::Def,
                    "if" => Token::If,
                    "elif" => Token::Elif,
                    "else" => Token::Else,
                    "for" => Token::For,
                    "while" => Token::While,
                    "return" => Token::Return,
                    "pass" => Token::Pass,
                    "in" => Token::In,
                    "global" => Token::Global,
                    "as" => Token::As,
                    "at" => Token::At,
                    "asat" => Token::Asat,
                    "and" => Token::And,
                    "or" => Token::Or,
                    "not" => Token::Not,
                    "unless" => Token::Unless,
                    "match" => Token::Match,
                    "case" => Token::Case,
                    "const" => Token::Const,
                    "define" => Token::Define,
                    "create" => Token::Create,
                    "end" => Token::End,
                    "to" => Token::To,
                    "by" => Token::By,
                    "_" => Token::Underscore,
                    "True" => Token::True_,
                    "False" => Token::False_,
                    "None" => Token::None_,
                    _ => Token::Ident(ident),
                };
                push_token(
                    tokens,
                    token,
                    line_offset + start,
                    line_offset + end,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '@' => {
                // Selector (e.g., @a, @p, @s, @e[...], @Player)
                let mut selector = String::new();
                let (_, at) = chars.next().unwrap();
                selector.push(at); // @
                                   // Collect all alphanumeric characters (for @Player, @Boss, etc.)
                let mut end = start + at.len_utf8();
                while let Some(&(index, ch)) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        let (_, consumed) = chars.next().unwrap();
                        selector.push(consumed);
                        end = index + consumed.len_utf8();
                    } else {
                        break;
                    }
                }
                // Handle selector arguments
                if chars.peek().map(|(_, ch)| ch) == Some(&'[') {
                    let mut bracket_depth = 0;
                    while let Some(&(index, ch)) = chars.peek() {
                        selector.push(ch);
                        end = index + ch.len_utf8();
                        if ch == '[' {
                            bracket_depth += 1;
                        } else if ch == ']' {
                            bracket_depth -= 1;
                            chars.next();
                            if bracket_depth == 0 {
                                break;
                            }
                            continue;
                        }
                        chars.next();
                    }
                }
                push_token(
                    tokens,
                    Token::Ident(selector),
                    line_offset + start,
                    line_offset + end,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '~' => {
                // Coordinate marker
                let mut coord = String::new();
                let (_, tilde) = chars.next().unwrap();
                coord.push(tilde);
                let mut end = start + tilde.len_utf8();
                while let Some(&(index, ch)) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        let (_, consumed) = chars.next().unwrap();
                        coord.push(consumed);
                        end = index + consumed.len_utf8();
                    } else {
                        break;
                    }
                }
                push_token(
                    tokens,
                    Token::Ident(coord),
                    line_offset + start,
                    line_offset + end,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '^' => {
                chars.next();
                // Context-aware: check if it's a coordinate (^number) or power operator (^)
                // If previous token suggests binary operator context, it's power operator
                if should_be_power_operator(tokens) {
                    // It's a power operator
                    push_token(
                        tokens,
                        Token::Caret,
                        line_offset + start,
                        line_offset + start + ch.len_utf8(),
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                } else if let Some(&(_, ch)) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        // It's a coordinate marker (in execute commands)
                        let mut coord = String::from("^");
                        let mut end = start + '^'.len_utf8();
                        while let Some(&(index, ch)) = chars.peek() {
                            if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                                let (_, consumed) = chars.next().unwrap();
                                coord.push(consumed);
                                end = index + consumed.len_utf8();
                            } else {
                                break;
                            }
                        }
                        push_token(
                            tokens,
                            Token::Ident(coord),
                            line_offset + start,
                            line_offset + end,
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                    } else {
                        // It's a power operator
                        push_token(
                            tokens,
                            Token::Caret,
                            line_offset + start,
                            line_offset + start + '^'.len_utf8(),
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                    }
                } else {
                    // End of input, it's a power operator
                    push_token(
                        tokens,
                        Token::Caret,
                        line_offset + start,
                        line_offset + start + '^'.len_utf8(),
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '=' => {
                chars.next();
                if chars.peek().map(|(_, ch)| ch) == Some(&'=') {
                    chars.next();
                    push_token(
                        tokens,
                        Token::EqEq,
                        line_offset + start,
                        line_offset + start + 2,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                } else {
                    push_token(
                        tokens,
                        Token::Equals,
                        line_offset + start,
                        line_offset + start + 1,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '!' => {
                chars.next();
                if chars.peek().map(|(_, ch)| ch) == Some(&'=') {
                    chars.next();
                    push_token(
                        tokens,
                        Token::NotEq,
                        line_offset + start,
                        line_offset + start + 2,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                } else {
                    return Err("Unexpected '!' character".to_string());
                }
            }
            '<' => {
                chars.next();
                if chars.peek().map(|(_, ch)| ch) == Some(&'=') {
                    chars.next();
                    push_token(
                        tokens,
                        Token::LtEq,
                        line_offset + start,
                        line_offset + start + 2,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                } else {
                    push_token(
                        tokens,
                        Token::Lt,
                        line_offset + start,
                        line_offset + start + 1,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '>' => {
                chars.next();
                if chars.peek().map(|(_, ch)| ch) == Some(&'=') {
                    chars.next();
                    push_token(
                        tokens,
                        Token::GtEq,
                        line_offset + start,
                        line_offset + start + 2,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                } else {
                    push_token(
                        tokens,
                        Token::Gt,
                        line_offset + start,
                        line_offset + start + 1,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '(' => {
                chars.next();
                push_token(
                    tokens,
                    Token::LParen,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth += 1;
            }
            ')' => {
                chars.next();
                push_token(
                    tokens,
                    Token::RParen,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth -= 1;
            }
            '[' => {
                chars.next();
                push_token(
                    tokens,
                    Token::LBracket,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth += 1;
            }
            ']' => {
                chars.next();
                push_token(
                    tokens,
                    Token::RBracket,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth -= 1;
            }
            ':' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Colon,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            ';' => {
                chars.next();
                push_token(
                    tokens,
                    Token::SemiColon,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            ',' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Comma,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '.' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Dot,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '+' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Plus,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '-' => {
                chars.next();
                // Context-aware parsing: check if this should be binary minus or unary negative
                if let Some(&(_, next_ch)) = chars.peek() {
                    // Only treat as negative number if:
                    // 1. Next char is a digit
                    // 2. Previous token suggests unary context (not a binary operator context)
                    if next_ch.is_ascii_digit() && !should_be_binary_minus(tokens) {
                        let mut num = String::from("-");
                        let mut end = start + '-'.len_utf8();
                        while let Some(&(index, ch)) = chars.peek() {
                            if ch.is_ascii_digit() {
                                let (_, consumed) = chars.next().unwrap();
                                num.push(consumed);
                                end = index + consumed.len_utf8();
                            } else if ch == '.' {
                                // Check if this is a range operator (..)
                                let mut temp_chars = chars.clone();
                                temp_chars.next(); // skip first dot
                                if let Some(&(_, next_ch)) = temp_chars.peek() {
                                    if next_ch == '.' {
                                        // This is "..", stop parsing number
                                        break;
                                    }
                                }
                                // Single dot, part of decimal number
                                let (_, consumed) = chars.next().unwrap();
                                num.push(consumed);
                                end = index + consumed.len_utf8();
                            } else {
                                break;
                            }
                        }
                        // Validate that the number can be parsed
                        if num.parse::<f64>().is_err() {
                            return Err(format!(
                                "Invalid number literal: '{}' at line {}",
                                num, line
                            ));
                        }
                        push_token(
                            tokens,
                            Token::Number(num),
                            line_offset + start,
                            line_offset + end,
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                    } else {
                        // Binary minus operator
                        push_token(
                            tokens,
                            Token::Minus,
                            line_offset + start,
                            line_offset + start + 1,
                            line_number,
                            column_from_byte(line, start, column_offset),
                        );
                    }
                } else {
                    push_token(
                        tokens,
                        Token::Minus,
                        line_offset + start,
                        line_offset + start + 1,
                        line_number,
                        column_from_byte(line, start, column_offset),
                    );
                }
            }
            '*' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Star,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '%' => {
                chars.next();
                push_token(
                    tokens,
                    Token::Percent,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
            }
            '{' => {
                chars.next();
                push_token(
                    tokens,
                    Token::LBrace,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth += 1;
            }
            '}' => {
                chars.next();
                push_token(
                    tokens,
                    Token::RBrace,
                    line_offset + start,
                    line_offset + start + 1,
                    line_number,
                    column_from_byte(line, start, column_offset),
                );
                *paren_depth -= 1;
            }
            '#' => {
                // Comment - ignore rest of line
                break;
            }
            _ => {
                return Err(format!("Unexpected character: {}", ch));
            }
        }
    }

    Ok(())
}

fn column_from_byte(line: &str, byte_index: usize, column_offset: usize) -> usize {
    column_offset + line[..byte_index].chars().count() + 1
}

fn strip_minecraft_inline_comment(command: &str) -> &str {
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let chars: Vec<(usize, char)> = command.char_indices().collect();

    for (position, (index, ch)) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        if *ch == '\\' {
            escaped = true;
            continue;
        }

        if let Some(active_quote) = quote {
            if *ch == active_quote {
                quote = None;
            }
            continue;
        }

        if *ch == '"' || *ch == '\'' {
            quote = Some(*ch);
            continue;
        }

        if *ch == '#' {
            let prev_is_space = position == 0
                || chars
                    .get(position.wrapping_sub(1))
                    .map(|(_, c)| c.is_whitespace())
                    .unwrap_or(false);
            let next_is_space_or_end = chars
                .get(position + 1)
                .map(|(_, c)| c.is_whitespace())
                .unwrap_or(true);
            if prev_is_space && next_is_space_or_end {
                return command[..*index].trim_end();
            }
        }
    }

    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spanned_tokens_preserve_existing_token_sequence() {
        let source = "def main():\n    x = -1\n    /say hi # comment\n";
        let plain = tokenize(source).unwrap();
        let spanned = tokenize_spanned(source)
            .unwrap()
            .into_iter()
            .map(|token| token.token)
            .collect::<Vec<_>>();

        assert_eq!(plain, spanned);
    }

    #[test]
    fn spanned_tokens_report_byte_range_line_and_column() {
        let source = "def main():\n    /say hi # comment\n";
        let tokens = tokenize_spanned(source).unwrap();

        let def = &tokens[0];
        assert_eq!(def.token, Token::Def);
        assert_eq!(def.span, SourceSpan::new(0, 3, 1, 1));

        let command = tokens
            .iter()
            .find(|token| matches!(token.token, Token::MinecraftCommand(_)))
            .unwrap();
        assert_eq!(command.token, Token::MinecraftCommand("say hi".to_string()));
        assert_eq!(command.span, SourceSpan::new(16, 23, 2, 5));
    }

    #[test]
    fn spanned_tokens_account_for_crlf_byte_offsets() {
        let source = "x = 1\r\nabc = 2\n";
        let tokens = tokenize_spanned(source).unwrap();

        let abc = tokens
            .iter()
            .find(|token| token.token == Token::Ident("abc".to_string()))
            .unwrap();
        assert_eq!(abc.span, SourceSpan::new(7, 10, 2, 1));

        let two = tokens
            .iter()
            .find(|token| token.token == Token::Number("2".to_string()))
            .unwrap();
        assert_eq!(two.span, SourceSpan::new(13, 14, 2, 7));
    }
}
