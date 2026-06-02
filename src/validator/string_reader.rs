/// Brigadier-style string reader for parsing Minecraft commands.
/// Tracks a cursor position and provides methods for reading various token types.
#[derive(Debug, Clone)]
pub struct StringReader {
    input: Vec<char>,
    cursor: usize,
}

impl StringReader {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            cursor: 0,
        }
    }

    pub fn cursor(&self) -> usize {
        self.cursor
    }

    pub fn set_cursor(&mut self, pos: usize) {
        self.cursor = pos;
    }

    pub fn can_read(&self) -> bool {
        self.cursor < self.input.len()
    }

    pub fn can_read_n(&self, n: usize) -> bool {
        self.cursor + n <= self.input.len()
    }

    pub fn peek(&self) -> Option<char> {
        self.input.get(self.cursor).copied()
    }

    pub fn peek_at(&self, offset: usize) -> Option<char> {
        self.input.get(self.cursor + offset).copied()
    }

    pub fn read_char(&mut self) -> Option<char> {
        let ch = self.input.get(self.cursor).copied()?;
        self.cursor += 1;
        Some(ch)
    }

    pub fn skip_whitespace(&mut self) {
        while self.cursor < self.input.len() && self.input[self.cursor] == ' ' {
            self.cursor += 1;
        }
    }

    pub fn skip_required_whitespace(&mut self) -> bool {
        let start = self.cursor;
        self.skip_whitespace();
        self.cursor > start
    }

    pub fn remaining(&self) -> String {
        self.input[self.cursor..].iter().collect()
    }

    pub fn slice(&self, start: usize, end: usize) -> String {
        self.input[start..end].iter().collect()
    }

    pub fn remaining_len(&self) -> usize {
        self.input.len() - self.cursor
    }

    /// Check if the next character is whitespace or end of input (i.e., a token boundary).
    pub fn at_token_boundary(&self) -> bool {
        !self.can_read() || self.peek() == Some(' ')
    }

    /// Try to read a specific literal followed by end-of-input or whitespace.
    pub fn try_read_literal(&mut self, literal: &str) -> bool {
        let chars: Vec<char> = literal.chars().collect();
        let len = chars.len();

        if self.cursor + len > self.input.len() {
            return false;
        }

        for (i, &expected) in chars.iter().enumerate() {
            if self.input[self.cursor + i] != expected {
                return false;
            }
        }

        // Must be followed by space or end of input
        if self.cursor + len < self.input.len() && self.input[self.cursor + len] != ' ' {
            return false;
        }

        self.cursor += len;
        true
    }

    /// Read an unquoted string: [a-zA-Z0-9_:.+\-/#]
    /// Returns the string read, which may be empty.
    pub fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.cursor < self.input.len() && is_unquoted_char(self.input[self.cursor]) {
            self.cursor += 1;
        }
        self.input[start..self.cursor].iter().collect()
    }

    /// Read a quoted string ("..." or '...') with escape handling.
    /// Returns true if successfully read.
    pub fn read_quoted_string(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }
        let quote = self.input[self.cursor];
        if quote != '"' && quote != '\'' {
            return false;
        }
        self.cursor += 1;

        while self.can_read() {
            let ch = self.input[self.cursor];
            self.cursor += 1;

            if ch == '\\' {
                // Escape: skip next char
                if self.can_read() {
                    self.cursor += 1;
                }
                continue;
            }
            if ch == quote {
                return true;
            }
        }
        false
    }

    /// Read a string: if it starts with quote, read quoted; otherwise read unquoted.
    pub fn read_string(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }
        let ch = self.input[self.cursor];
        if ch == '"' || ch == '\'' {
            self.read_quoted_string()
        } else {
            let s = self.read_unquoted_string();
            !s.is_empty()
        }
    }

    /// Read everything remaining.
    pub fn read_greedy(&mut self) -> String {
        let result: String = self.input[self.cursor..].iter().collect();
        self.cursor = self.input.len();
        result
    }

    /// Read an integer (optional sign + digits).
    pub fn read_integer(&mut self) -> Option<i64> {
        let start = self.cursor;
        if self.can_read() && (self.input[self.cursor] == '-' || self.input[self.cursor] == '+') {
            self.cursor += 1;
        }
        if !self.can_read() || !self.input[self.cursor].is_ascii_digit() {
            self.cursor = start;
            return None;
        }
        while self.can_read() && self.input[self.cursor].is_ascii_digit() {
            self.cursor += 1;
        }
        let s: String = self.input[start..self.cursor].iter().collect();
        s.parse().ok()
    }

    /// Read a float/double (integer with optional decimal part and/or exponent).
    pub fn read_float(&mut self) -> Option<f64> {
        let start = self.cursor;
        if self.can_read() && (self.input[self.cursor] == '-' || self.input[self.cursor] == '+') {
            self.cursor += 1;
        }
        let has_digits_before_dot = self.can_read() && self.input[self.cursor].is_ascii_digit();

        while self.can_read() && self.input[self.cursor].is_ascii_digit() {
            self.cursor += 1;
        }

        if self.can_read() && self.input[self.cursor] == '.' {
            self.cursor += 1;
            let has_digits_after_dot = self.can_read() && self.input[self.cursor].is_ascii_digit();
            while self.can_read() && self.input[self.cursor].is_ascii_digit() {
                self.cursor += 1;
            }
            if !has_digits_before_dot && !has_digits_after_dot {
                self.cursor = start;
                return None;
            }
        } else if !has_digits_before_dot {
            self.cursor = start;
            return None;
        }

        // Exponent part
        if self.can_read() && (self.input[self.cursor] == 'e' || self.input[self.cursor] == 'E') {
            self.cursor += 1;
            if self.can_read() && (self.input[self.cursor] == '+' || self.input[self.cursor] == '-')
            {
                self.cursor += 1;
            }
            while self.can_read() && self.input[self.cursor].is_ascii_digit() {
                self.cursor += 1;
            }
        }

        let s: String = self.input[start..self.cursor].iter().collect();
        s.parse().ok()
    }

    /// Read balanced braces: {} or []. Used for NBT/JSON.
    pub fn read_nbt(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }
        let open = self.input[self.cursor];
        let close = match open {
            '{' => '}',
            '[' => ']',
            _ => return false,
        };

        let mut depth = 0;
        let mut quote: Option<char> = None;
        let mut escape_next = false;

        while self.can_read() {
            let ch = self.input[self.cursor];
            self.cursor += 1;

            if escape_next {
                escape_next = false;
                continue;
            }

            if ch == '\\' && quote.is_some() {
                escape_next = true;
                continue;
            }

            if let Some(active_quote) = quote {
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }

            if ch == '"' || ch == '\'' {
                quote = Some(ch);
                continue;
            }

            if ch == open {
                depth += 1;
            } else if ch == close {
                depth -= 1;
                if depth == 0 {
                    return true;
                }
            }
        }

        false
    }

    /// Read a target selector: @a, @s, @p, @e, @r, optionally with [...]
    pub fn read_selector(&mut self) -> bool {
        if !self.can_read() || self.input[self.cursor] != '@' {
            return false;
        }
        self.cursor += 1;

        // Read selector type letter
        if !self.can_read() || !self.input[self.cursor].is_ascii_alphabetic() {
            self.cursor -= 1;
            return false;
        }
        self.cursor += 1;

        // Optional [...] selector arguments
        if self.can_read() && self.input[self.cursor] == '[' {
            let mut depth = 0;
            let mut in_string = false;
            let mut escape_next = false;
            while self.can_read() {
                let ch = self.input[self.cursor];
                self.cursor += 1;

                if escape_next {
                    escape_next = false;
                    continue;
                }
                if ch == '\\' && in_string {
                    escape_next = true;
                    continue;
                }
                if ch == '"' {
                    in_string = !in_string;
                    continue;
                }
                if in_string {
                    continue;
                }

                if ch == '[' {
                    depth += 1;
                } else if ch == ']' {
                    depth -= 1;
                    if depth == 0 {
                        return true;
                    }
                }
                // Also handle nested {} inside selectors (NBT)
                if ch == '{' {
                    // Unconsume the '{' and delegate to read_nbt
                    self.cursor -= 1;
                    self.read_nbt();
                    continue;
                }
            }
            return false;
        }

        true
    }

    /// Read a coordinate value: ~, ~N, ^, ^N, or a plain number.
    /// Returns true if a coordinate was read.
    pub fn read_coordinate(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }

        // Handle macro placeholder
        if self.try_read_macro() {
            return true;
        }

        let ch = self.input[self.cursor];

        if ch == '~' || ch == '^' {
            self.cursor += 1;
            // Optionally followed by a number (no space between)
            let _ = self.read_float();
            return true;
        }

        // Plain number
        self.read_float().is_some()
    }

    /// Try to read a $(macro) placeholder. Consumes $( ... ) if found.
    pub fn try_read_macro(&mut self) -> bool {
        let saved = self.cursor;
        if self.cursor + 1 < self.input.len()
            && self.input[self.cursor] == '$'
            && self.input[self.cursor + 1] == '('
        {
            self.cursor += 2;
            while self.can_read() {
                let ch = self.input[self.cursor];
                self.cursor += 1;
                if ch == ')' {
                    return true;
                }
            }
            self.cursor = saved;
            false
        } else {
            false
        }
    }
}

/// Characters allowed in unquoted strings in Minecraft/Brigadier.
fn is_unquoted_char(c: char) -> bool {
    c.is_ascii_alphanumeric()
        || c == '_'
        || c == ':'
        || c == '.'
        || c == '/'
        || c == '+'
        || c == '-'
        || c == '#'
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_read_literal() {
        let mut r = StringReader::new("say hello");
        assert!(r.try_read_literal("say"));
        assert_eq!(r.cursor(), 3);

        r.skip_whitespace();
        assert_eq!(r.remaining(), "hello");
    }

    #[test]
    fn test_read_quoted_string() {
        let mut r = StringReader::new("\"hello world\" rest");
        assert!(r.read_quoted_string());
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_selector() {
        let mut r = StringReader::new("@a[tag=foo,limit=1] rest");
        assert!(r.read_selector());
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_coordinate() {
        let mut r = StringReader::new("~10 ~ ^-3.5");
        assert!(r.read_coordinate());
        assert_eq!(r.remaining(), " ~ ^-3.5");
        r.skip_whitespace();
        assert!(r.read_coordinate());
        assert_eq!(r.remaining(), " ^-3.5");
        r.skip_whitespace();
        assert!(r.read_coordinate());
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn test_read_nbt() {
        let mut r = StringReader::new("{key:\"value\",nested:{a:1}} rest");
        assert!(r.read_nbt());
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_integer() {
        let mut r = StringReader::new("-42 rest");
        assert_eq!(r.read_integer(), Some(-42));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_float() {
        let mut r = StringReader::new("2.5 rest");
        assert_eq!(r.read_float(), Some(2.5));
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_macro() {
        let mut r = StringReader::new("$(arg) rest");
        assert!(r.try_read_macro());
        assert_eq!(r.remaining(), " rest");
    }
}
