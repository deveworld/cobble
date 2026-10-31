/// Brigadier-style string reader for parsing Minecraft commands.
/// Tracks a cursor position and provides methods for reading various token types.
#[derive(Debug, Clone)]
pub struct StringReader {
    input: Vec<char>,
    cursor: usize,
    macro_line: bool,
    loose_spaces: bool,
}

impl StringReader {
    pub fn new(input: &str) -> Self {
        Self {
            input: input.chars().collect(),
            cursor: 0,
            macro_line: false,
            loose_spaces: false,
        }
    }

    /// A reader for a macro line without its `$`. Its `$(name)` placeholders
    /// get their values when the function runs.
    pub fn for_macro_line(input: &str) -> Self {
        Self {
            macro_line: true,
            ..Self::new(input)
        }
    }

    /// The same reader, but any run of spaces separates two arguments.
    pub fn with_loose_spaces(self) -> Self {
        Self {
            loose_spaces: true,
            ..self
        }
    }

    pub fn is_macro_line(&self) -> bool {
        self.macro_line
    }

    /// Skips the rest of the current token if it holds a `$(name)`
    /// placeholder, as in `kills_$(mob)`.
    pub fn skip_token_with_macro(&mut self) -> bool {
        let mut end = self.cursor;
        while end < self.input.len() && self.input[end] != ' ' {
            end += 1;
        }
        let token: String = self.input[self.cursor..end].iter().collect();
        if !token.contains("$(") {
            return false;
        }
        self.cursor = end;
        true
    }

    /// Skips the rest of a macro line if a placeholder follows a word that
    /// opens a bracket or a quote: the value can hold what closes it.
    pub fn skip_open_word_before_macro(&mut self) -> bool {
        let rest = self.remaining();
        let word = rest.split(' ').next().unwrap_or_default();
        if !word.contains(['[', '{', '"', '\'']) || !rest.contains("$(") {
            return false;
        }
        self.cursor = self.input.len();
        true
    }

    /// Whether this is a macro line whose last word holds a placeholder.
    pub fn ends_in_macro(&self) -> bool {
        let line: String = self.input.iter().collect();
        self.macro_line
            && line
                .rsplit(' ')
                .next()
                .is_some_and(|word| word.contains("$("))
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
        while self.peek().is_some_and(is_java_whitespace) {
            self.cursor += 1;
        }
    }

    /// Skips the one space between two arguments. Any more spaces belong to
    /// the next argument, and only some parsers take them.
    pub fn skip_separator(&mut self) -> bool {
        if self.peek() != Some(' ') {
            return false;
        }
        self.cursor += 1;
        if self.loose_spaces {
            self.skip_whitespace();
        }
        true
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

    /// Read an unquoted string: [a-zA-Z0-9_.+\-]
    /// Returns the string read, which may be empty.
    pub fn read_unquoted_string(&mut self) -> String {
        let start = self.cursor;
        while self.cursor < self.input.len() && is_unquoted_char(self.input[self.cursor]) {
            self.cursor += 1;
        }
        self.input[start..self.cursor].iter().collect()
    }

    /// Read everything up to the next space, for the arguments with their own
    /// characters, such as a criterion's `:` and a slot range's `*`.
    pub fn read_until_space(&mut self) -> String {
        let start = self.cursor;
        while !self.at_token_boundary() {
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

    /// Read a string the way Brigadier does and return its value. Without
    /// quotes it can be empty. In quotes, only `\\` and the quote itself can be
    /// escaped, unlike in SNBT, and `None` means the string is broken.
    pub fn read_string(&mut self) -> Option<String> {
        let quote = self.peek().filter(|ch| matches!(ch, '"' | '\''));
        let Some(quote) = quote else {
            return Some(self.read_unquoted_string());
        };
        self.cursor += 1;
        let mut value = String::new();
        loop {
            match self.read_char()? {
                '\\' => {
                    let escaped = self.read_char()?;
                    if escaped != quote && escaped != '\\' {
                        return None;
                    }
                    value.push(escaped);
                }
                ch if ch == quote => return Some(value),
                ch => value.push(ch),
            }
        }
    }

    /// Read everything remaining.
    pub fn read_greedy(&mut self) -> String {
        let result: String = self.input[self.cursor..].iter().collect();
        self.cursor = self.input.len();
        result
    }

    /// Read the characters of a Brigadier number: digits, `.`, and `-`. It has
    /// no exponent and no `+`.
    fn read_number_text(&mut self) -> String {
        let start = self.cursor;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '.' | '-'))
        {
            self.cursor += 1;
        }
        self.input[start..self.cursor].iter().collect()
    }

    /// Read an integer. The cursor stays if it can't be read.
    pub fn read_integer(&mut self) -> Option<i64> {
        let start = self.cursor;
        let value = self.read_number_text().parse().ok();
        if value.is_none() {
            self.cursor = start;
        }
        value
    }

    /// Read a float or a double. The cursor stays if it can't be read.
    pub fn read_float(&mut self) -> Option<f64> {
        let start = self.cursor;
        let value = self.read_number_text().parse().ok();
        if value.is_none() {
            self.cursor = start;
        }
        value
    }

    /// Read a float, as Java rounds it to 32 bits. The cursor stays if it can't
    /// be read.
    pub fn read_f32(&mut self) -> Option<f32> {
        let start = self.cursor;
        let value = self.read_number_text().parse().ok();
        if value.is_none() {
            self.cursor = start;
        }
        value
    }

    /// Read one end of a range. A `..` ends it.
    fn read_range_end(&mut self) -> String {
        let start = self.cursor;
        while self.peek().is_some_and(|ch| {
            ch.is_ascii_digit() || ch == '-' || (ch == '.' && self.peek_at(1) != Some('.'))
        }) {
            self.cursor += 1;
        }
        self.input[start..self.cursor].iter().collect()
    }

    /// Read `1`, `1..`, `..5`, or `1..5`, where `is_number` accepts each end.
    fn read_range(&mut self, is_number: fn(&str) -> bool) -> bool {
        let low = self.read_range_end();
        if !low.is_empty() && !is_number(&low) {
            return false;
        }
        if self.peek() != Some('.') || self.peek_at(1) != Some('.') {
            return !low.is_empty();
        }
        self.cursor += 2;
        let high = self.read_range_end();
        !(low.is_empty() && high.is_empty()) && (high.is_empty() || is_number(&high))
    }

    pub fn read_int_range(&mut self) -> bool {
        self.read_range(|number| number.parse::<i32>().is_ok())
    }

    pub fn read_float_range(&mut self) -> bool {
        self.read_range(|number| number.parse::<f64>().is_ok())
    }

    /// Read balanced braces: {} or []. Used for NBT/JSON.
    pub fn read_nbt(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }
        let open = self.input[self.cursor];
        let first_close = match open {
            '{' => '}',
            '[' => ']',
            _ => return false,
        };

        let mut stack = vec![first_close];
        let mut quote: Option<char> = None;
        let mut escape_next = false;
        self.cursor += 1;

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

            match ch {
                '{' => stack.push('}'),
                '[' => stack.push(']'),
                '}' | ']' => {
                    if stack.pop() != Some(ch) {
                        return false;
                    }
                    if stack.is_empty() {
                        return true;
                    }
                }
                _ => {}
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
                if let Some(open) = quote {
                    if ch == open {
                        quote = None;
                    }
                    continue;
                }
                if ch == '"' || ch == '\'' {
                    quote = Some(ch);
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

    /// Read a coordinate value: ~, ~N, ^, ^N, or a plain number. Nothing
    /// before the next space counts as a coordinate too, one that reads as 0.
    /// Returns true if a coordinate was read.
    pub fn read_coordinate(&mut self) -> bool {
        if !self.can_read() {
            return false;
        }

        // A placeholder can stand anywhere in a coordinate, as in `~-$(dist)`.
        if self.macro_line && self.skip_token_with_macro() {
            return true;
        }

        // Handle macro placeholder
        if self.try_read_macro() {
            return true;
        }

        let ch = self.input[self.cursor];

        if ch == '~' || ch == '^' {
            self.cursor += 1;
            // Optionally followed by a number or a macro offset (`^$(dist)`),
            // with no space between.
            if !self.try_read_macro() {
                let _ = self.read_float();
            }
            return true;
        }

        // Plain number
        ch == ' ' || self.read_float().is_some()
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

/// Java's `Character.isWhitespace`, which leaves out the non-breaking spaces
/// and takes four control characters that Rust's `is_whitespace` doesn't.
pub fn is_java_whitespace(ch: char) -> bool {
    matches!(ch, '\u{1c}'..='\u{1f}')
        || (ch.is_whitespace() && !matches!(ch, '\u{85}' | '\u{a0}' | '\u{2007}' | '\u{202f}'))
}

/// Characters allowed in unquoted strings in Minecraft/Brigadier.
fn is_unquoted_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || c == '_' || c == '.' || c == '+' || c == '-'
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
    fn test_read_unquoted_string_stops_at_other_characters() {
        for (input, word) in [
            ("a.b+c-d_E1 rest", "a.b+c-d_E1"),
            ("a:b", "a"),
            ("a/b", "a"),
            ("a#b", "a"),
            ("a*b", "a"),
            (":a", ""),
        ] {
            let mut r = StringReader::new(input);
            assert_eq!(r.read_unquoted_string(), word, "{input}");
        }
    }

    #[test]
    fn test_read_until_space_takes_any_character() {
        let mut r = StringReader::new("a:b*c/#d rest");
        assert_eq!(r.read_until_space(), "a:b*c/#d");
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_quoted_string() {
        let mut r = StringReader::new("\"hello world\" rest");
        assert!(r.read_quoted_string());
        assert_eq!(r.remaining(), " rest");
    }

    #[test]
    fn test_read_string_follows_brigadier() {
        for (input, value) in [
            ("a.b rest", "a.b"),
            (" rest", ""),
            ("a:b", "a"),
            ("\"a b\" rest", "a b"),
            ("'a b' rest", "a b"),
            ("\"\" rest", ""),
            (r#""a\\b" rest"#, "a\\b"),
            (r#""a\"b" rest"#, "a\"b"),
            (r"'a\'b' rest", "a'b"),
            (r#""a'b" rest"#, "a'b"),
        ] {
            let mut r = StringReader::new(input);
            assert_eq!(r.read_string().as_deref(), Some(value), "{input}");
        }
        for input in [
            r#""a\nb""#,
            r#""a\'b""#,
            r#"'a\"b'"#,
            r#""\u0041""#,
            r#""a"#,
            r#""a\"#,
            r#""a\""#,
        ] {
            assert_eq!(StringReader::new(input).read_string(), None, "{input}");
        }
    }

    #[test]
    fn test_skip_whitespace_follows_java() {
        for space in [
            '\t', '\u{b}', '\u{1c}', '\u{1f}', ' ', '\u{2003}', '\u{2028}', '\u{3000}',
        ] {
            let mut r = StringReader::new(&format!("{space}a"));
            r.skip_whitespace();
            assert_eq!(r.remaining(), "a", "{space:?}");
        }
        for space in ['\u{85}', '\u{a0}', '\u{2007}', '\u{202f}', '\u{0}'] {
            let mut r = StringReader::new(&format!("{space}a"));
            r.skip_whitespace();
            assert_eq!(r.cursor(), 0, "{space:?}");
        }
    }

    #[test]
    fn test_read_selector() {
        let mut r = StringReader::new("@a[tag=foo,limit=1] rest");
        assert!(r.read_selector());
        assert_eq!(r.remaining(), " rest");

        for selector in [
            "@e[name=\"a]b\"] rest",
            "@e[name='a]b'] rest",
            "@e[nbt={a:\"]\"}] rest",
        ] {
            let mut r = StringReader::new(selector);
            assert!(r.read_selector(), "{selector}");
            assert_eq!(r.remaining(), " rest", "{selector}");
        }
    }

    #[test]
    fn test_read_coordinate() {
        let mut r = StringReader::new("~10 ~ ^-3.5");
        assert!(r.read_coordinate());
        assert_eq!(r.remaining(), " ~ ^-3.5");
        assert!(r.skip_separator());
        assert!(r.read_coordinate());
        assert_eq!(r.remaining(), " ^-3.5");
        assert!(r.skip_separator());
        assert!(r.read_coordinate());
        assert!(r.remaining().is_empty());
    }

    #[test]
    fn test_read_coordinate_counts_nothing_before_a_space() {
        let mut r = StringReader::new(" 1");
        assert!(r.read_coordinate());
        assert_eq!(r.remaining(), " 1");
        assert!(!StringReader::new("").read_coordinate());
    }

    #[test]
    fn test_skip_separator_takes_one_space() {
        let mut r = StringReader::new("  a");
        assert!(r.skip_separator());
        assert_eq!(r.remaining(), " a");
        assert!(r.skip_separator());
        assert!(!r.skip_separator());

        let mut loose = StringReader::new("  a").with_loose_spaces();
        assert!(loose.skip_separator());
        assert_eq!(loose.remaining(), "a");
    }

    #[test]
    fn test_read_nbt() {
        let mut r = StringReader::new("{key:\"value\",nested:{a:1}} rest");
        assert!(r.read_nbt());
        assert_eq!(r.remaining(), " rest");

        let mut array = StringReader::new("[{key:\"value\"}] rest");
        assert!(array.read_nbt());
        assert_eq!(array.remaining(), " rest");
    }

    #[test]
    fn test_read_nbt_rejects_mismatched_nested_delimiters() {
        let mut r = StringReader::new("{Items:[{id:\"stone\"}} rest");
        assert!(!r.read_nbt());
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
    fn test_read_f32_rounds_like_java() {
        let mut r = StringReader::new("1.00000001 rest");
        assert_eq!(r.read_f32(), Some(1.0));
        assert_eq!(r.remaining(), " rest");
        let mut r = StringReader::new("340282356779733661637539395458142568447");
        assert_eq!(r.read_f32(), Some(f32::MAX));
        let mut r = StringReader::new("340282356779733661637539395458142568448");
        assert_eq!(r.read_f32(), Some(f32::INFINITY));
        let mut r = StringReader::new("+1");
        assert_eq!(r.read_f32(), None);
        assert_eq!(r.cursor(), 0);
    }

    #[test]
    fn test_read_numbers_have_no_exponent_and_no_plus() {
        for number in ["1", "-1", "1.5", ".5", "5.", "-.5"] {
            let mut r = StringReader::new(number);
            assert!(r.read_float().is_some(), "{number}");
            assert!(!r.can_read(), "{number}");
        }
        // The reader stops at what is not a number character.
        for (text, rest) in [("1e3", "e3"), ("1E3", "E3"), ("1d", "d")] {
            let mut r = StringReader::new(text);
            assert_eq!(r.read_float(), Some(1.0), "{text}");
            assert_eq!(r.remaining(), rest, "{text}");
        }
        for text in ["+1", "-", ".", "1.2.3", "--1", "1..5", "1-2", "NaN", "inf"] {
            let mut r = StringReader::new(text);
            assert_eq!(r.read_float(), None, "{text}");
            assert_eq!(r.cursor(), 0, "{text}");
        }
        for text in ["+1", "1.5", "1.0", "-", "1-2"] {
            let mut r = StringReader::new(text);
            assert_eq!(r.read_integer(), None, "{text}");
            assert_eq!(r.cursor(), 0, "{text}");
        }
        assert_eq!(StringReader::new("01").read_integer(), Some(1));
    }

    #[test]
    fn test_read_ranges() {
        for range in ["1", "-1", "1..", "..1", "1..2", "-5..-1"] {
            let mut r = StringReader::new(range);
            assert!(r.read_int_range(), "{range}");
            assert!(!r.can_read(), "{range}");
        }
        for range in [
            "..",
            "",
            "1.5",
            "1..2.5",
            "+1",
            "1..+2",
            "a",
            "-",
            "2147483648",
        ] {
            let mut r = StringReader::new(range);
            let whole = r.read_int_range() && !r.can_read();
            assert!(!whole, "{range}");
        }
        for range in ["1.5", ".5...7", "1...5", "..1.5", "1.."] {
            let mut r = StringReader::new(range);
            assert!(r.read_float_range(), "{range}");
            assert!(!r.can_read(), "{range}");
        }
        for range in ["..", ".", "+1", "1..+2", "1e1", "1..2..3", "1.2.3"] {
            let mut r = StringReader::new(range);
            let whole = r.read_float_range() && !r.can_read();
            assert!(!whole, "{range}");
        }
    }

    #[test]
    fn test_read_macro() {
        let mut r = StringReader::new("$(arg) rest");
        assert!(r.try_read_macro());
        assert_eq!(r.remaining(), " rest");
    }
}
