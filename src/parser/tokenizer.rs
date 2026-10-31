use crate::ast::SourceSpan;
use std::borrow::Cow;
use std::iter::Peekable;
use std::str::CharIndices;

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
    /// Line the token ends on: `span.line` except for a triple-quoted
    /// string that spans several lines.
    pub end_line: usize,
}

impl SpannedToken {
    fn new(token: Token, start: usize, end: usize, line: usize, column: usize) -> Self {
        Self {
            token,
            span: SourceSpan::new(start, end, line, column),
            end_line: line,
        }
    }
}

/// A tokenizer failure anchored at the source position that caused it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TokenizeError {
    pub message: String,
    pub span: SourceSpan,
}

impl TokenizeError {
    fn at(message: impl Into<String>, start: usize, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            span: SourceSpan::new(start, start, line, column),
        }
    }
}

impl std::fmt::Display for TokenizeError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.write_str(&self.message)
    }
}

/// A triple-quoted string still open at the end of a source line. Its text
/// continues on the following raw lines up to the closing delimiter.
struct OpenTripleQuote {
    quote: char,
    start: usize,
    line: usize,
    column: usize,
    text: String,
}

fn triple_quote_pattern(quote: char) -> &'static str {
    if quote == '\'' {
        "'''"
    } else {
        "\"\"\""
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
    tokenize_spanned_located(source).map_err(|error| error.message)
}

/// Like [`tokenize_spanned`], but a failure carries its source position.
pub fn tokenize_spanned_located(source: &str) -> Result<Vec<SpannedToken>, TokenizeError> {
    let source = &*join_continued_commands(source);
    let mut tokens = Vec::new();
    let mut indent_stack: Vec<usize> = vec![0];
    let mut paren_depth = 0;
    let lines = source_lines_with_offsets(source);
    let mut line_idx = 0;

    while line_idx < lines.len() {
        let (line_start, line) = lines[line_idx];
        // Skip empty lines and comments
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            line_idx += 1;
            continue;
        }

        // Only handle indentation if we are not inside parentheses/brackets/braces
        if paren_depth == 0 {
            // Calculate indentation. Levels compare in bytes; columns count
            // characters, so wide indentation such as U+3000 still reports
            // the column a reader sees.
            let indent_level = line.len() - line.trim_start().len();
            let indent_column = line[..indent_level].chars().count() + 1;
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
                    indent_column,
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
                        indent_column,
                    );
                }
                if *indent_stack.last().unwrap() != indent_level {
                    return Err(TokenizeError::at(
                        format!("Indentation error at line {}", line_idx + 1),
                        line_start + indent_level,
                        line_idx + 1,
                        indent_column,
                    ));
                }
            }
        }

        // Tokenize the line content
        let line_content = line.trim();
        let content_offset = line.find(line_content).unwrap_or(0);
        let mut open_quote = tokenize_line(
            line_content,
            line_start + content_offset,
            line[..content_offset].chars().count(),
            line_idx + 1,
            &mut tokens,
            &mut paren_depth,
        )?;

        // A triple-quoted string (usually a docstring) swallows whole raw
        // lines, including blank, comment-like, and code-like ones, until its
        // closing delimiter. Tokenizing resumes after the delimiter, and the
        // logical line ends on the line that closed the string.
        let mut end_line = (line_idx, line_start, line);
        while let Some(mut open) = open_quote.take() {
            let pattern = triple_quote_pattern(open.quote);
            loop {
                line_idx += 1;
                let Some(&(next_start, next_line)) = lines.get(line_idx) else {
                    return Err(TokenizeError::at(
                        format!(
                            "Unterminated triple-quoted string: add the closing {}",
                            pattern
                        ),
                        open.start,
                        open.line,
                        open.column,
                    ));
                };
                end_line = (line_idx, next_start, next_line);
                open.text.push('\n');
                let Some(close) = next_line.find(pattern) else {
                    open.text.push_str(next_line);
                    continue;
                };
                open.text.push_str(&next_line[..close]);
                let after_close = close + pattern.len();
                push_token(
                    &mut tokens,
                    Token::String(open.text),
                    open.start,
                    next_start + after_close,
                    open.line,
                    open.column,
                );
                if let Some(string_token) = tokens.last_mut() {
                    string_token.end_line = line_idx + 1;
                }
                let rest = &next_line[after_close..];
                let rest_content = rest.trim();
                if !rest_content.is_empty() {
                    let rest_offset = after_close + (rest.len() - rest.trim_start().len());
                    open_quote = tokenize_line(
                        rest_content,
                        next_start + rest_offset,
                        next_line[..rest_offset].chars().count(),
                        line_idx + 1,
                        &mut tokens,
                        &mut paren_depth,
                    )?;
                }
                break;
            }
        }

        // Only emit Newline if not inside parentheses/brackets/braces
        if paren_depth == 0 {
            let (end_idx, end_start, end_text) = end_line;
            push_token(
                &mut tokens,
                Token::Newline,
                end_start + end_text.len(),
                end_start + end_text.len(),
                end_idx + 1,
                end_text.chars().count() + 1,
            );
        }
        line_idx += 1;
    }

    let (end_line, end_column) = source_end_line_column(source);

    // Add remaining dedents
    while indent_stack.len() > 1 {
        indent_stack.pop();
        push_token(
            &mut tokens,
            Token::Dedent,
            source.len(),
            source.len(),
            end_line,
            end_column,
        );
    }

    push_token(
        &mut tokens,
        Token::Eof,
        source.len(),
        source.len(),
        end_line,
        end_column,
    );
    relax_soft_keywords(&mut tokens);
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

/// Joins raw commands continued with a trailing `\`, the way Minecraft reads
/// a function file: the next line's indent doesn't count. Blank lines and
/// trailing spaces take the freed room, so every line and offset after a
/// command stays where it was in the source. With nothing left to join, a
/// final `\` is dropped.
pub fn join_continued_commands(source: &str) -> Cow<'_, str> {
    let is_command = |line: &str| line.trim_start().starts_with('/');
    if !source
        .lines()
        .any(|line| is_command(line) && command_continues(line))
    {
        return Cow::Borrowed(source);
    }
    let lines: Vec<&str> = source.split_inclusive('\n').collect();
    let mut output = String::with_capacity(source.len());
    let mut open_quote = None;
    let mut index = 0;
    while index < lines.len() {
        let first = index;
        index += 1;
        if open_quote.is_some() || !is_command(lines[first]) {
            open_quote = triple_quote_after(lines[first], open_quote);
            output.push_str(lines[first]);
            continue;
        }
        if !command_continues(lines[first]) {
            output.push_str(lines[first]);
            continue;
        }
        let mut joined = lines[first].trim_end().to_string();
        while command_continues(&joined) {
            joined.truncate(strip_minecraft_inline_comment(&joined).trim_end().len() - 1);
            let Some(next) = lines.get(index) else {
                break;
            };
            joined.push_str(next.trim());
            index += 1;
        }
        let consumed = &lines[first..index];
        let size: usize = consumed.iter().map(|line| line.len()).sum();
        let newlines = consumed.iter().filter(|line| line.ends_with('\n')).count();
        output.push_str(&joined);
        output.push_str(&" ".repeat(size - joined.len() - newlines));
        output.push_str(&"\n".repeat(newlines));
    }
    Cow::Owned(output)
}

/// Whether a raw command goes on in the next line. A `\` in the command's
/// comment doesn't count.
fn command_continues(command: &str) -> bool {
    strip_minecraft_inline_comment(command)
        .trim_end()
        .ends_with('\\')
}

/// The triple quote still open after `line`, given the one open before it.
/// The text of a triple-quoted string stays as written.
fn triple_quote_after(line: &str, open: Option<char>) -> Option<char> {
    if let Some(quote) = open {
        return (!line.contains(triple_quote_pattern(quote))).then_some(quote);
    }
    if line.trim_start().starts_with('#') {
        return None;
    }
    ['"', '\'']
        .into_iter()
        .find(|quote| line.matches(triple_quote_pattern(*quote)).count() % 2 == 1)
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

fn source_end_line_column(source: &str) -> (usize, usize) {
    let mut line = 1;
    let mut column = 1;

    for ch in source.chars() {
        if ch == '\n' {
            line += 1;
            column = 1;
        } else {
            column += 1;
        }
    }

    (line, column)
}

/// Check if the minus sign should be treated as a binary operator
/// based on the previous token context. The same "previous token is an
/// operand" rule decides whether `/` divides instead of starting a command.
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
) -> Result<Option<OpenTripleQuote>, TokenizeError> {
    let mut tokenizer = LineTokenizer {
        line,
        line_offset,
        column_offset,
        line_number,
        chars: line.char_indices().peekable(),
        tokens,
        paren_depth,
        last_column: (0, 0),
    };

    while let Some(&(start, ch)) = tokenizer.chars.peek() {
        match ch {
            ' ' | '\t' => {
                tokenizer.chars.next();
            }
            '#' => tokenizer.tag_or_comment(start),
            '/' => tokenizer.command_or_slash(start),
            '"' | '\'' if line[start..].starts_with(triple_quote_pattern(ch)) => {
                if let Some(open) = tokenizer.triple_quoted_string(start, ch) {
                    return Ok(Some(open));
                }
            }
            '"' | '\'' => tokenizer.string_literal(start),
            '0'..='9' => tokenizer.number(start, String::new())?,
            '-' => tokenizer.minus_or_negative_number(start)?,
            'a'..='z' | 'A'..='Z' | '_' => tokenizer.identifier_or_keyword(start),
            '@' => tokenizer.selector(start),
            '~' => tokenizer.coordinate(start),
            '^' => tokenizer.caret_or_coordinate(start),
            '=' => tokenizer.operator_with_optional_eq(start, Token::Equals, Token::EqEq),
            '<' => tokenizer.operator_with_optional_eq(start, Token::Lt, Token::LtEq),
            '>' => tokenizer.operator_with_optional_eq(start, Token::Gt, Token::GtEq),
            '!' => tokenizer.not_equals(start)?,
            '(' => tokenizer.open_group(start, Token::LParen),
            '[' => tokenizer.open_group(start, Token::LBracket),
            '{' => tokenizer.open_group(start, Token::LBrace),
            ')' => tokenizer.close_group(start, Token::RParen),
            ']' => tokenizer.close_group(start, Token::RBracket),
            '}' => tokenizer.close_group(start, Token::RBrace),
            ':' => tokenizer.single(start, Token::Colon),
            ';' => tokenizer.single(start, Token::SemiColon),
            ',' => tokenizer.single(start, Token::Comma),
            '.' => tokenizer.single(start, Token::Dot),
            '+' => tokenizer.single(start, Token::Plus),
            '*' => tokenizer.single(start, Token::Star),
            '%' => tokenizer.single(start, Token::Percent),
            _ => {
                let hint = if ch.is_alphabetic() {
                    ". Names use ASCII letters, digits, and underscores; other text goes in a string"
                } else {
                    ""
                };
                return Err(tokenizer.error_at(start, format!("Unexpected character: {ch}{hint}")));
            }
        }
    }

    Ok(None)
}

/// Cursor over the text of one line. Byte indices passed to its methods are
/// relative to `line`; `line_offset` and `column_offset` place them in the
/// source.
struct LineTokenizer<'a> {
    line: &'a str,
    line_offset: usize,
    column_offset: usize,
    line_number: usize,
    chars: Peekable<CharIndices<'a>>,
    tokens: &'a mut Vec<SpannedToken>,
    paren_depth: &'a mut i32,
    /// The byte index and character count of the last pushed token's start.
    /// Tokens come in order, so each column counts on from the last one
    /// instead of from the start of a possibly very long line.
    last_column: (usize, usize),
}

impl LineTokenizer<'_> {
    fn push(&mut self, token: Token, start: usize, end: usize) {
        let (last_byte, last_chars) = self.last_column;
        let chars = if start >= last_byte && self.line.is_char_boundary(start) {
            last_chars + self.line[last_byte..start].chars().count()
        } else {
            self.line[..start].chars().count()
        };
        self.last_column = (start, chars);
        let column = self.column_offset + chars + 1;
        push_token(
            self.tokens,
            token,
            self.line_offset + start,
            self.line_offset + end,
            self.line_number,
            column,
        );
    }

    fn error_at(&self, start: usize, message: String) -> TokenizeError {
        TokenizeError::at(
            message,
            self.line_offset + start,
            self.line_number,
            column_from_byte(self.line, start, self.column_offset),
        )
    }

    /// Consume characters up to the byte index `end`.
    fn skip_to(&mut self, end: usize) {
        while self.chars.next_if(|&(index, _)| index < end).is_some() {}
    }

    /// Append characters to `text` while `accept(index, ch)` holds. Returns
    /// the byte index just past the last one taken, or `end` if none was.
    fn take_while(
        &mut self,
        text: &mut String,
        mut end: usize,
        accept: impl Fn(usize, char) -> bool,
    ) -> usize {
        while let Some((index, ch)) = self.chars.next_if(|&(index, ch)| accept(index, ch)) {
            text.push(ch);
            end = index + ch.len_utf8();
        }
        end
    }

    fn single(&mut self, start: usize, token: Token) {
        self.chars.next();
        self.push(token, start, start + 1);
    }

    fn open_group(&mut self, start: usize, token: Token) {
        self.single(start, token);
        *self.paren_depth += 1;
    }

    fn close_group(&mut self, start: usize, token: Token) {
        self.single(start, token);
        // Clamp at zero: a stray closer must not drive the depth
        // negative and suppress Newline/Indent emission for the
        // rest of the file.
        *self.paren_depth = (*self.paren_depth - 1).max(0);
    }

    fn tag_or_comment(&mut self, start: usize) {
        if hash_starts_comment(self.line, start) {
            self.skip_to(self.line.len());
            return;
        }
        let rest = &self.line[start + 1..];
        let len = tag_reference_len(rest).unwrap_or_else(|| {
            rest.find(|ch: char| !is_score_holder_char(ch))
                .unwrap_or(rest.len())
        });
        let end = start + 1 + len;
        self.push(Token::Ident(self.line[start..end].to_string()), start, end);
        self.skip_to(end);
    }

    fn command_or_slash(&mut self, start: usize) {
        // Check if this is a Minecraft command (starts with / followed by letter)
        // or a division operator
        self.chars.next();
        match self.chars.peek() {
            // Minecraft command only if followed immediately by a letter (no
            // space) and not after an operand: `a/b` and `f(x)/y` divide,
            // while a statement-leading `/say` stays a raw command.
            Some(&(next_index, next_ch))
                if next_ch.is_alphabetic()
                    && !should_be_binary_minus(self.tokens)
                    && !matches!(self.tokens.last(), Some(token) if token.token == Token::Slash) =>
            {
                // Minecraft command - consume rest of line
                let rest = &self.line[next_index..];
                let stripped = strip_minecraft_inline_comment(rest).trim_end();
                self.push(
                    Token::MinecraftCommand(stripped.to_string()),
                    start,
                    next_index + stripped.len(),
                );
                self.skip_to(self.line.len());
            }
            // Division operator, or a `/` at the end of the line
            _ => self.push(Token::Slash, start, start + 1),
        }
    }

    /// Triple-quoted string: closed on this line, or returned open so the
    /// caller continues it over the following raw lines.
    fn triple_quoted_string(&mut self, start: usize, quote: char) -> Option<OpenTripleQuote> {
        let pattern = triple_quote_pattern(quote);
        let after_open = start + pattern.len();
        let Some(close) = self.line[after_open..].find(pattern) else {
            return Some(OpenTripleQuote {
                quote,
                start: self.line_offset + start,
                line: self.line_number,
                column: column_from_byte(self.line, start, self.column_offset),
                text: self.line[after_open..].to_string(),
            });
        };
        let end = after_open + close + pattern.len();
        let text = self.line[after_open..after_open + close].to_string();
        self.push(Token::String(text), start, end);
        self.skip_to(end);
        None
    }

    fn string_literal(&mut self, start: usize) {
        let (quote_index, quote) = self.chars.next().unwrap();
        let mut s = String::new();
        let mut escaped = false;
        let mut end = quote_index + quote.len_utf8();
        while let Some((index, ch)) = self.chars.next() {
            end = index + ch.len_utf8();
            if escaped {
                escaped = false;
                // The escapes Python decodes. Any other backslash stays, as
                // in Python.
                match ch {
                    'n' => s.push('\n'),
                    't' => s.push('\t'),
                    'r' => s.push('\r'),
                    '\\' | '"' | '\'' => s.push(ch),
                    'u' => match self.unicode_escape() {
                        Some((decoded, last)) => {
                            s.push(decoded);
                            end = last;
                        }
                        None => s.push_str("\\u"),
                    },
                    _ => {
                        s.push('\\');
                        s.push(ch);
                    }
                }
            } else if ch == '\\' {
                escaped = true;
            } else if ch == quote {
                break;
            } else {
                s.push(ch);
            }
        }
        self.push(Token::String(s), start, end);
    }

    /// The character of a `\uXXXX` escape whose `\u` was just read, and the
    /// byte offset after it. Leaves the input alone when four hex digits
    /// don't follow.
    fn unicode_escape(&mut self) -> Option<(char, usize)> {
        let rest = self.line.get(self.chars.peek()?.0..)?;
        let hex = rest.get(..4)?;
        if !hex.chars().all(|ch| ch.is_ascii_hexdigit()) {
            return None;
        }
        let decoded = u32::from_str_radix(hex, 16).ok().and_then(char::from_u32)?;
        let mut last = 0;
        for _ in 0..4 {
            let (index, ch) = self.chars.next()?;
            last = index + ch.len_utf8();
        }
        Some((decoded, last))
    }

    /// The digits of a number literal starting at `start`. `num` holds what
    /// is already consumed: nothing, or the `-` of a negative literal.
    fn number(&mut self, start: usize, mut num: String) -> Result<(), TokenizeError> {
        let line = self.line;
        let after_sign = start + num.len();
        // A dot followed by another dot is the range operator (`1..5`) and
        // ends the number; a single dot is part of a decimal number.
        let end = self.take_while(&mut num, after_sign, |index, ch| {
            ch.is_ascii_digit() || (ch == '.' && !line[index..].starts_with(".."))
        });
        // Validate that the number can be parsed
        if num.parse::<f64>().is_err() {
            return Err(self.error_at(
                start,
                format!(
                    "Invalid number literal: '{}' at line {}",
                    num, self.line_number
                ),
            ));
        }
        self.push(Token::Number(num), start, end);
        Ok(())
    }

    fn minus_or_negative_number(&mut self, start: usize) -> Result<(), TokenizeError> {
        // Context-aware parsing: check if this should be binary minus or unary negative.
        // Only treat as negative number if:
        // 1. Next char is a digit
        // 2. Previous token suggests unary context (not a binary operator context)
        let digit_follows = self.line[start + 1..].starts_with(|ch: char| ch.is_ascii_digit());
        if digit_follows && !should_be_binary_minus(self.tokens) {
            self.chars.next();
            self.number(start, String::from("-"))
        } else {
            // Binary minus operator
            self.single(start, Token::Minus);
            Ok(())
        }
    }

    fn identifier_or_keyword(&mut self, start: usize) {
        // Identifier or keyword (may include namespace like minecraft:stone).
        // A colon followed by an identifier is a namespace separator, except
        // in a subscript, where it separates the bounds of `items[i:j]`.
        let line = self.line;
        let in_subscript = self.in_subscript();
        let mut ident = String::new();
        let end = self.take_while(&mut ident, start, |index, ch| {
            ch.is_alphanumeric()
                || ch == '_'
                || (ch == ':'
                    && !in_subscript
                    && line[index + 1..]
                        .starts_with(|next: char| next.is_alphabetic() || next == '_'))
        });
        self.push(keyword_or_ident(ident), start, end);
    }

    /// Whether the innermost open bracket follows a value, as in `items[`.
    fn in_subscript(&self) -> bool {
        let mut depth = 0usize;
        for (index, token) in self.tokens.iter().enumerate().rev() {
            match token.token {
                Token::RParen | Token::RBracket | Token::RBrace => depth += 1,
                Token::LParen | Token::LBrace if depth == 0 => return false,
                Token::LParen | Token::LBrace | Token::LBracket if depth > 0 => depth -= 1,
                Token::LBracket => {
                    let before = index.checked_sub(1).map(|index| &self.tokens[index].token);
                    return matches!(
                        before,
                        Some(Token::Ident(_) | Token::RBracket | Token::RParen | Token::String(_))
                    );
                }
                Token::Newline => return false,
                _ => {}
            }
        }
        false
    }

    fn selector(&mut self, start: usize) {
        // Selector (e.g., @a, @p, @s, @e[...], @Player)
        self.chars.next();
        let mut selector = String::from("@");
        // Collect all alphanumeric characters (for @Player, @Boss, etc.)
        let mut end = self.take_while(&mut selector, start + 1, |_, ch| {
            ch.is_alphanumeric() || ch == '_'
        });
        // Handle selector arguments
        if self.chars.peek().map(|&(_, ch)| ch) == Some('[') {
            // Brackets inside quoted values such as `name="a]b"` don't count.
            let mut bracket_depth = 0;
            let mut quote: Option<char> = None;
            let mut escaped = false;
            for (index, ch) in self.chars.by_ref() {
                selector.push(ch);
                end = index + ch.len_utf8();
                if let Some(active) = quote {
                    if escaped {
                        escaped = false;
                    } else if ch == '\\' {
                        escaped = true;
                    } else if ch == active {
                        quote = None;
                    }
                } else if ch == '"' || ch == '\'' {
                    quote = Some(ch);
                } else if ch == '[' {
                    bracket_depth += 1;
                } else if ch == ']' {
                    bracket_depth -= 1;
                    if bracket_depth == 0 {
                        break;
                    }
                }
            }
        }
        self.push(Token::Ident(selector), start, end);
    }

    /// A `~` or `^` coordinate marker with its offset (`~`, `~1`, `^-0.5`).
    fn coordinate(&mut self, start: usize) {
        let (_, marker) = self.chars.next().unwrap();
        let mut coord = String::from(marker);
        let end = self.take_while(&mut coord, start + marker.len_utf8(), |_, ch| {
            is_coordinate_offset_char(ch)
        });
        self.push(Token::Ident(coord), start, end);
    }

    fn caret_or_coordinate(&mut self, start: usize) {
        // Context-aware: check if it's a coordinate (^number) or power operator (^)
        // If previous token suggests binary operator context, it's power operator
        let offset_follows = self.line[start + 1..].starts_with(is_coordinate_offset_char);
        if offset_follows && !should_be_power_operator(self.tokens) {
            // It's a coordinate marker (in execute commands)
            self.coordinate(start);
        } else {
            self.single(start, Token::Caret);
        }
    }

    /// `bare` on its own, or `with_eq` when `=` follows directly (`<` / `<=`).
    fn operator_with_optional_eq(&mut self, start: usize, bare: Token, with_eq: Token) {
        self.chars.next();
        if self.chars.next_if(|&(_, ch)| ch == '=').is_some() {
            self.push(with_eq, start, start + 2);
        } else {
            self.push(bare, start, start + 1);
        }
    }

    fn not_equals(&mut self, start: usize) -> Result<(), TokenizeError> {
        self.chars.next();
        if self.chars.next_if(|&(_, ch)| ch == '=').is_none() {
            return Err(self.error_at(start, "Unexpected '!' character".to_string()));
        }
        self.push(Token::NotEq, start, start + 2);
        Ok(())
    }
}

/// Turns words that are keywords in only one form back into names, so `end`,
/// `to`, `at`, and the like work as variables and functions.
fn relax_soft_keywords(tokens: &mut [SpannedToken]) {
    let mut line_start = 0;
    for index in 0..tokens.len() {
        let previous = index.checked_sub(1).map(|index| &tokens[index].token);
        if matches!(
            previous,
            Some(Token::Newline | Token::Indent | Token::Dedent)
        ) {
            line_start = index;
        }
        let next = tokens.get(index + 1).map(|token| &token.token);
        let selector_follows = matches!(next, Some(Token::Ident(name)) if name.starts_with('@'));
        let starts_line = index == line_start;
        let header_ends_line = || {
            let rest = tokens[index..]
                .iter()
                .take_while(|token| token.token != Token::Newline);
            rest.last().is_some_and(|token| token.token == Token::Colon)
        };
        let word = match tokens[index].token {
            // `end` alone on its line closes `define`.
            Token::End if starts_line && matches!(next, Some(Token::Newline | Token::Eof)) => None,
            Token::End => Some("end"),
            // `case 1 to 10:`
            Token::To if tokens[line_start].token == Token::Case => None,
            Token::To => Some("to"),
            // `for i in items by 2:`
            Token::By
                if tokens[line_start].token == Token::For
                    && !matches!(previous, Some(Token::For | Token::Comma)) =>
            {
                None
            }
            Token::By => Some("by"),
            // `create @Boss`, or the block of a `define`.
            Token::Create if selector_follows || (starts_line && next == Some(&Token::LBrace)) => {
                None
            }
            Token::Create => Some("create"),
            Token::Define if selector_follows => None,
            Token::Define => Some("define"),
            // `at @s:` or `at {player}:`
            Token::At if selector_follows || next == Some(&Token::LBrace) => None,
            Token::At => Some("at"),
            Token::Asat if selector_follows || next == Some(&Token::LBrace) => None,
            Token::Asat => Some("asat"),
            Token::Match | Token::Case if starts_line && header_ends_line() => None,
            Token::Match => Some("match"),
            Token::Case => Some("case"),
            Token::Const if starts_line && matches!(next, Some(Token::Ident(_))) => None,
            Token::Const => Some("const"),
            _ => None,
        };
        if let Some(word) = word {
            tokens[index].token = Token::Ident(word.to_string());
        }
    }
}

fn is_coordinate_offset_char(ch: char) -> bool {
    ch.is_ascii_digit() || ch == '.' || ch == '-'
}

fn keyword_or_ident(ident: String) -> Token {
    match ident.as_str() {
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
    }
}

/// Byte length of the namespaced tag reference (`ns:path`) right after a
/// `#`, so `#minecraft:logs` in an execute header (`if block ~ ~ ~
/// #minecraft:logs:`) is a tag rather than a comment. The namespace is
/// required: `# note`, `#todo`, and `#TODO: later` stay comments.
pub(crate) fn tag_reference_len(after_hash: &str) -> Option<usize> {
    let is_namespace_byte = |b: u8| matches!(b, b'a'..=b'z' | b'0'..=b'9' | b'_' | b'-' | b'.');
    let namespace_len = after_hash
        .bytes()
        .take_while(|&b| is_namespace_byte(b))
        .count();
    if namespace_len == 0 || after_hash.as_bytes().get(namespace_len) != Some(&b':') {
        return None;
    }
    let path_len = after_hash[namespace_len + 1..]
        .bytes()
        .take_while(|&b| is_namespace_byte(b) || b == b'/')
        .count();
    (path_len > 0).then_some(namespace_len + 1 + path_len)
}

/// Whether the `#` at byte `hash_index` of `line` starts a comment. A `#`
/// that begins a namespaced tag (`#minecraft:logs`) is a value, and so is a
/// `#name` before the `:` that ends a block header, such as the fake player
/// in `if score #count obj matches 1:` or the tag in `as @a[tag=#x]:`.
pub(crate) fn hash_starts_comment(line: &str, hash_index: usize) -> bool {
    let rest = &line[hash_index + 1..];
    if tag_reference_len(rest).is_some() {
        return false;
    }
    if !rest.starts_with(is_score_holder_char) {
        return true;
    }
    !ends_block_header(rest)
}

/// Whether `text` contains the `:` that ends a block header, one followed
/// by nothing but whitespace or a comment.
fn ends_block_header(text: &str) -> bool {
    text.match_indices(':').any(|(index, _)| {
        let after = text[index + 1..].trim_start();
        after.is_empty() || after.starts_with('#')
    })
}

fn is_score_holder_char(ch: char) -> bool {
    ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+')
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

        // A quote right after a letter or digit is an apostrophe (`it's`),
        // which opens no string.
        let after_word = position > 0 && chars[position - 1].1.is_alphanumeric();
        if (*ch == '"' || *ch == '\'') && !after_word {
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

    /// `source` with its continued commands joined, of the same size.
    fn joined(source: &str) -> String {
        let joined = join_continued_commands(source);
        assert_eq!(joined.len(), source.len(), "{source:?}");
        assert_eq!(
            joined.matches('\n').count(),
            source.matches('\n').count(),
            "{source:?}"
        );
        joined.into_owned()
    }

    #[test]
    fn continued_commands_leave_other_sources_alone() {
        for source in [
            "def main():\n    /say a\n",
            "x = 1\n",
            "",
            // A `\` means nothing outside a raw command.
            "x = \"a\\\n",
            "# a \\\n/say b\n",
        ] {
            assert!(
                matches!(join_continued_commands(source), Cow::Borrowed(_)),
                "{source:?}"
            );
        }
    }

    #[test]
    fn continued_commands_keep_lines_and_offsets() {
        let source = "def main():\n    /say a \\\n        b \\\n    c\n    /say d\nx = 1\n";
        assert_eq!(
            joined(source),
            format!(
                "def main():\n    /say a b c{}\n\n\n    /say d\nx = 1\n",
                " ".repeat(14)
            )
        );

        let tokens = tokenize_spanned(source).unwrap();
        let command = |text: &str| {
            tokens
                .iter()
                .find(|token| token.token == Token::MinecraftCommand(text.to_string()))
                .unwrap_or_else(|| panic!("no command `{text}`: {tokens:?}"))
        };
        let first = command("say a b c");
        assert_eq!(
            (first.span.start, first.span.line, first.span.column),
            (source.find("/say a").unwrap(), 2, 5)
        );
        let after = command("say d");
        assert_eq!(
            (after.span.start, after.span.line, after.span.column),
            (source.find("/say d").unwrap(), 5, 5)
        );
        let x = tokens
            .iter()
            .find(|token| token.token == Token::Ident("x".to_string()))
            .unwrap();
        assert_eq!(
            (x.span.start, x.span.line),
            (source.rfind("x = 1").unwrap(), 6)
        );
    }

    #[test]
    fn continued_commands_read_crlf() {
        let source = "/say a \\\r\n    b\r\n/say c\r\n";
        assert_eq!(
            joined(source),
            format!("/say a b{}\n\n/say c\r\n", " ".repeat(7))
        );
        let after = tokenize_spanned(source)
            .unwrap()
            .into_iter()
            .find(|token| token.token == Token::MinecraftCommand("say c".to_string()))
            .unwrap();
        assert_eq!(after.span.start, source.find("/say c").unwrap());
        assert_eq!(after.span.line, 3);
    }

    #[test]
    fn continued_commands_need_no_final_newline() {
        assert_eq!(
            joined("/say a \\\n  b"),
            format!("/say a b{}\n", " ".repeat(3))
        );
        assert_eq!(
            joined("/say a \\\n  b \\\n"),
            format!("/say a b {}\n\n", " ".repeat(4))
        );
    }

    #[test]
    fn a_final_backslash_is_dropped() {
        for (source, command) in [
            ("/say a\\", "say a"),
            ("/say a \\", "say a"),
            ("/say a \\\n", "say a"),
            ("/say a \\\r\n", "say a"),
            ("/say a \\   ", "say a"),
            ("/say a \\ # note", "say a"),
            ("/say a \\\n  b \\", "say a b"),
        ] {
            let joined = joined(source);
            assert!(!joined.contains('\\'), "{source:?}: {joined:?}");
            assert_eq!(
                tokenize_spanned(source).unwrap()[0].token,
                Token::MinecraftCommand(command.to_string()),
                "{source:?}"
            );
        }
    }

    #[test]
    fn continued_commands_join_the_way_minecraft_does() {
        let command = |source: &str| tokenize_spanned(source).unwrap()[0].token.clone();
        let say = |text: &str| Token::MinecraftCommand(text.to_string());
        // Only the `\` goes, so a space before it stays.
        assert_eq!(command("/give @s\\\n stone\n"), say("give @sstone"));
        assert_eq!(command("/give @s \\\n stone\n"), say("give @s stone"));
        assert_eq!(command("/say a \\   \nb\n"), say("say a b"));
        // A blank line ends the command.
        assert_eq!(command("/say a \\\n\n"), say("say a"));
        // One `\` goes at each step, so a second one continues again.
        assert_eq!(command("/say a\\\\\nb\n"), say("say a\\b"));
        assert_eq!(command("/say a\\\\\n\n/say b\n"), say("say a/say b"));
    }

    #[test]
    fn continued_commands_keep_comments_apart() {
        let source = "/say a # see C:\\\n/say b\n";
        assert!(matches!(join_continued_commands(source), Cow::Borrowed(_)));
        let commands: Vec<Token> = tokenize_spanned(source)
            .unwrap()
            .into_iter()
            .map(|token| token.token)
            .filter(|token| matches!(token, Token::MinecraftCommand(_)))
            .collect();
        assert_eq!(
            commands,
            [
                Token::MinecraftCommand("say a".to_string()),
                Token::MinecraftCommand("say b".to_string()),
            ]
        );

        assert_eq!(
            tokenize_spanned("/say a \\ # first\n  b # second\n").unwrap()[0].token,
            Token::MinecraftCommand("say a b".to_string())
        );
    }

    #[test]
    fn continued_commands_leave_triple_quoted_text_alone() {
        for source in [
            "x = \"\"\"\n/say a \\\nb\n\"\"\"\n/say c \\\n  d\n",
            "def main():\n    '''\n    /say a \\\n    b\n    '''\n    /say c \\\n      d\n",
        ] {
            let joined = joined(source);
            assert!(joined.contains("/say a \\\n"), "{joined:?}");
            assert!(joined.contains("/say c d"), "{joined:?}");
        }
        assert_eq!(
            joined("# \"\"\"\n/say a \\\n  b\n"),
            format!("# \"\"\"\n/say a b{}\n\n", " ".repeat(3))
        );
        assert_eq!(
            joined("x = \"\"\"a\"\"\"\n/say a \\\n  b\n"),
            format!("x = \"\"\"a\"\"\"\n/say a b{}\n\n", " ".repeat(3))
        );
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

        let eof = tokens.last().unwrap();
        assert_eq!(eof.token, Token::Eof);
        assert_eq!(eof.span, SourceSpan::new(source.len(), source.len(), 3, 1));
    }
}
