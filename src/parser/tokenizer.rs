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
    Colon,
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
            Token::String(s) => write!(f, "\"{}\"", s),
            Token::Number(n) => write!(f, "{}", n),
            Token::MinecraftCommand(s) => write!(f, "/{}", s),
            Token::Dot => write!(f, "."),
            Token::Colon => write!(f, ":"),
            Token::Comma => write!(f, ","),
            Token::LParen => write!(f, "("),
            Token::RParen => write!(f, ")"),
            Token::LBracket => write!(f, "["),
            Token::RBracket => write!(f, "]"),
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

/// Find the position of a comment (#) that's not inside a string
/// Returns None if no comment found, Some(position) otherwise
fn find_comment_position(text: &str) -> Option<usize> {
    let mut in_string = false;
    let mut string_char = ' ';
    let mut escaped = false;

    for (i, ch) in text.chars().enumerate() {
        if escaped {
            escaped = false;
            continue;
        }

        if ch == '\\' && in_string {
            escaped = true;
            continue;
        }

        if (ch == '"' || ch == '\'') {
            if !in_string {
                in_string = true;
                string_char = ch;
            } else if ch == string_char {
                in_string = false;
            }
            continue;
        }

        if ch == '#' && !in_string {
            return Some(i);
        }
    }

    None
}

/// Manual tokenizer that handles indentation
pub fn tokenize(source: &str) -> Result<Vec<Token>, String> {
    let mut tokens = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];

    for (line_idx, line) in source.lines().enumerate() {
        // Skip empty lines and comments
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

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

        // Tokenize the line content
        let line_content = line.trim();
        tokenize_line(line_content, &mut tokens)?;
        tokens.push(Token::Newline);
    }

    // Add remaining dedents
    while indent_stack.len() > 1 {
        indent_stack.pop();
        tokens.push(Token::Dedent);
    }

    tokens.push(Token::Eof);
    Ok(tokens)
}

/// Tokenize a single line
fn tokenize_line(line: &str, tokens: &mut Vec<Token>) -> Result<(), String> {
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

                        // Strip inline comments (# character and everything after)
                        // Minecraft only supports comments at the beginning of lines
                        // We need to respect strings in the command to avoid removing # inside strings
                        let comment_pos = find_comment_position(&cmd);
                        if let Some(pos) = comment_pos {
                            cmd.truncate(pos);
                        }

                        // Trim trailing whitespace after comment removal
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
                    if ch.is_ascii_digit() || ch == '.' {
                        num.push(chars.next().unwrap());
                    } else {
                        break;
                    }
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
                // Check if it's a coordinate (^number) or power operator (^)
                if let Some(&ch) = chars.peek() {
                    if ch.is_ascii_digit() || ch == '.' || ch == '-' {
                        // It's a coordinate marker
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
            }
            ')' => {
                chars.next();
                tokens.push(Token::RParen);
            }
            '[' => {
                chars.next();
                tokens.push(Token::LBracket);
            }
            ']' => {
                chars.next();
                tokens.push(Token::RBracket);
            }
            ':' => {
                chars.next();
                tokens.push(Token::Colon);
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
                // Could be a negative number or minus operator
                if let Some(&next_ch) = chars.peek() {
                    if next_ch.is_ascii_digit() {
                        let mut num = String::from("-");
                        while let Some(&ch) = chars.peek() {
                            if ch.is_ascii_digit() || ch == '.' {
                                num.push(chars.next().unwrap());
                            } else {
                                break;
                            }
                        }
                        tokens.push(Token::Number(num));
                    } else {
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
            '{' | '}' => {
                // Part of JSON or NBT data - consume as identifier for now
                let mut data = String::new();
                let mut brace_depth = 0;
                loop {
                    match chars.peek() {
                        Some(&'{') => {
                            brace_depth += 1;
                            data.push(chars.next().unwrap());
                        }
                        Some(&'}') => {
                            data.push(chars.next().unwrap());
                            brace_depth -= 1;
                            if brace_depth == 0 {
                                break;
                            }
                        }
                        Some(_ch) => {
                            data.push(chars.next().unwrap());
                        }
                        None => break,
                    }
                }
                tokens.push(Token::Ident(data));
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
