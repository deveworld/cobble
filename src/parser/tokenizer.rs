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
    let mut tokens = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut paren_depth = 0;

    for (line_idx, line) in source.lines().enumerate() {
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
                tokens.push(Token::Indent);
            } else if indent_level < current_indent {
                while indent_stack.len() > 1 && *indent_stack.last().unwrap() > indent_level {
                    indent_stack.pop();
                    tokens.push(Token::Dedent);
                }
                if *indent_stack.last().unwrap() != indent_level {
                    return Err(format!("Indentation error at line {}", line_idx + 1));
                }
            }
        }

        // Tokenize the line content
        let line_content = line.trim();
        tokenize_line(line_content, &mut tokens, &mut paren_depth)?;

        // Only emit Newline if not inside parentheses/brackets/braces
        if paren_depth == 0 {
            tokens.push(Token::Newline);
        }
    }

    // Add remaining dedents
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token::Dedent);
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

/// Check if the minus sign should be treated as a binary operator
/// based on the previous token context
fn should_be_binary_minus(tokens: &[Token]) -> bool {
    // If previous token is one of these, minus is a binary operator:
    // Number, Ident, RParen, RBracket, True_, False_, None_
    if let Some(last_token) = tokens.last() {
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
fn should_be_power_operator(tokens: &[Token]) -> bool {
    // Similar to should_be_binary_minus - if previous token can be an operand,
    // then ^ is the power operator, not a coordinate marker
    if let Some(last_token) = tokens.last() {
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
fn tokenize_line(line: &str, tokens: &mut Vec<Token>, paren_depth: &mut i32) -> Result<(), String> {
    let mut chars = line.chars().peekable();

    while let Some(&ch) = chars.peek() {
        match ch {
            ' ' | '\t' => {
                chars.next();
            }
            '/' => {
                // Check if this is a Minecraft command (starts with / followed by letter)
                // or a division operator
                chars.next();
                if let Some(&next_ch) = chars.peek() {
                    // Minecraft command only if followed immediately by a letter (no space)
                    if next_ch.is_alphabetic() {
                        // Minecraft command - consume rest of line
                        let mut cmd: String = chars.collect();
                        cmd = strip_minecraft_inline_comment(&cmd).to_string();
                        cmd = cmd.trim_end().to_string();

                        tokens.push(Token::MinecraftCommand(cmd));
                        break;
                    } else {
                        // Division operator or other use
                        tokens.push(Token::Slash);
                    }
                } else {
                    // End of line after /, treat as Slash
                    tokens.push(Token::Slash);
                }
            }
            '"' | '\'' => {
                // String literal
                let quote = chars.next().unwrap();
                let mut s = String::new();
                let mut escaped = false;
                for ch in chars.by_ref() {
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
                tokens.push(Token::String(s));
            }
            '0'..='9' => {
                // Number
                let mut num = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() {
                        num.push(chars.next().unwrap());
                    } else if ch == '.' {
                        // Check if this is a range operator (..)
                        let mut temp_chars = chars.clone();
                        temp_chars.next(); // skip first dot
                        if let Some(&next_ch) = temp_chars.peek() {
                            if next_ch == '.' {
                                // This is "..", stop parsing number
                                break;
                            }
                        }
                        // Single dot, part of decimal number
                        num.push(chars.next().unwrap());
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
                tokens.push(Token::Number(num));
            }
            'a'..='z' | 'A'..='Z' | '_' => {
                // Identifier or keyword (may include namespace like minecraft:stone)
                let mut ident = String::new();
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        ident.push(chars.next().unwrap());
                    } else if ch == ':' {
                        // Check if this is a namespace separator (followed by identifier)
                        let mut temp_chars = chars.clone();
                        temp_chars.next(); // skip the colon
                        if let Some(&next_ch) = temp_chars.peek() {
                            if next_ch.is_alphabetic() || next_ch == '_' {
                                // This is a namespace separator
                                ident.push(chars.next().unwrap()); // add the colon
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
                tokens.push(token);
            }
            '@' => {
                // Selector (e.g., @a, @p, @s, @e[...], @Player)
                let mut selector = String::new();
                selector.push(chars.next().unwrap()); // @
                                                      // Collect all alphanumeric characters (for @Player, @Boss, etc.)
                while let Some(&ch) = chars.peek() {
                    if ch.is_alphanumeric() || ch == '_' {
                        selector.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                // Handle selector arguments
                if chars.peek() == Some(&'[') {
                    let mut bracket_depth = 0;
                    while let Some(ch) = chars.peek() {
                        selector.push(*ch);
                        if *ch == '[' {
                            bracket_depth += 1;
                        } else if *ch == ']' {
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
                tokens.push(Token::Ident(selector));
            }
            '~' => {
                // Coordinate marker
                let mut coord = String::new();
                coord.push(chars.next().unwrap());
                while let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        coord.push(chars.next().unwrap());
                    } else {
                        break;
                    }
                }
                tokens.push(Token::Ident(coord));
            }
            '^' => {
                chars.next();
                // Context-aware: check if it's a coordinate (^number) or power operator (^)
                // If previous token suggests binary operator context, it's power operator
                if should_be_power_operator(tokens) {
                    // It's a power operator
                    tokens.push(Token::Caret);
                } else if let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        // It's a coordinate marker (in execute commands)
                        let mut coord = String::from("^");
                        while let Some(&ch) = chars.peek() {
                            if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                                coord.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        tokens.push(Token::Ident(coord));
                    } else {
                        // It's a power operator
                        tokens.push(Token::Caret);
                    }
                } else {
                    // End of input, it's a power operator
                    tokens.push(Token::Caret);
                }
            }
            '=' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::EqEq);
                } else {
                    tokens.push(Token::Equals);
                }
            }
            '!' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::NotEq);
                } else {
                    return Err("Unexpected '!' character".to_string());
                }
            }
            '<' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::LtEq);
                } else {
                    tokens.push(Token::Lt);
                }
            }
            '>' => {
                chars.next();
                if chars.peek() == Some(&'=') {
                    chars.next();
                    tokens.push(Token::GtEq);
                } else {
                    tokens.push(Token::Gt);
                }
            }
            '(' => {
                chars.next();
                tokens.push(Token::LParen);
                *paren_depth += 1;
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
                *paren_depth -= 1;
            }
            '[' => {
                chars.next();
                tokens.push(Token::LBracket);
                *paren_depth += 1;
            }
            ']' => {
                chars.next();
                tokens.push(Token::RBracket);
                *paren_depth -= 1;
            }
            ':' => {
                chars.next();
                tokens.push(Token::Colon);
            }
            ';' => {
                chars.next();
                tokens.push(Token::SemiColon);
            }
            ',' => {
                chars.next();
                tokens.push(Token::Comma);
            }
            '.' => {
                chars.next();
                tokens.push(Token::Dot);
            }
            '+' => {
                chars.next();
                tokens.push(Token::Plus);
            }
            '-' => {
                chars.next();
                // Context-aware parsing: check if this should be binary minus or unary negative
                if let Some(&next_ch) = chars.peek() {
                    // Only treat as negative number if:
                    // 1. Next char is a digit
                    // 2. Previous token suggests unary context (not a binary operator context)
                    if next_ch.is_ascii_digit() && !should_be_binary_minus(tokens) {
                        let mut num = String::from("-");
                        while let Some(&ch) = chars.peek() {
                            if ch.is_ascii_digit() {
                                num.push(chars.next().unwrap());
                            } else if ch == '.' {
                                // Check if this is a range operator (..)
                                let mut temp_chars = chars.clone();
                                temp_chars.next(); // skip first dot
                                if let Some(&next_ch) = temp_chars.peek() {
                                    if next_ch == '.' {
                                        // This is "..", stop parsing number
                                        break;
                                    }
                                }
                                // Single dot, part of decimal number
                                num.push(chars.next().unwrap());
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
                        tokens.push(Token::Number(num));
                    } else {
                        // Binary minus operator
                        tokens.push(Token::Minus);
                    }
                } else {
                    tokens.push(Token::Minus);
                }
            }
            '*' => {
                chars.next();
                tokens.push(Token::Star);
            }
            '%' => {
                chars.next();
                tokens.push(Token::Percent);
            }
            '{' => {
                chars.next();
                tokens.push(Token::LBrace);
                *paren_depth += 1;
            }
            '}' => {
                chars.next();
                tokens.push(Token::RBrace);
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
