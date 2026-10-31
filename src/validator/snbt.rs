//! SNBT as Minecraft 26.3 reads it. A value it can't read stops the whole
//! function from loading, and every function that calls it. Each rule was
//! confirmed against a 26.3 server.
//!
//! Compounds and lists become their JSON form, which is how the checks for
//! text components and inline resources read them: `1b` is the number 1, and a
//! typed array such as `[I;1,2]` is a list of numbers.

use super::string_reader::{is_java_whitespace, StringReader};
use serde_json::{Map, Number, Value};

/// How many levels of a value are read. Minecraft runs out of stack from about
/// here, at a depth that changes between runs, so a compound or a list below
/// this level goes unchecked and its JSON form is `null`.
const MAX_NESTING: usize = 512;

/// The JSON form of `text`, or what stops Minecraft from reading it.
pub fn parse(text: &str) -> Result<Value, String> {
    let mut parser = Parser {
        chars: text.chars().collect(),
        position: 0,
        depth: 0,
    };
    let value = parser.value()?;
    parser.skip_spaces();
    if parser.position < parser.chars.len() {
        return Err(parser.expected("the end of the value"));
    }
    Ok(value)
}

/// The JSON form of the value at the cursor of `reader`, which moves past it.
pub fn read(reader: &mut StringReader) -> Result<Value, String> {
    let mut parser = Parser {
        chars: reader.remaining().chars().collect(),
        position: 0,
        depth: 0,
    };
    let value = parser.value()?;
    reader.set_cursor(reader.cursor() + parser.position);
    Ok(value)
}

#[derive(Clone, Copy, PartialEq, PartialOrd)]
enum Width {
    Byte,
    Short,
    Int,
    Long,
}

impl Width {
    fn bits(self) -> u32 {
        match self {
            Width::Byte => 8,
            Width::Short => 16,
            Width::Int => 32,
            Width::Long => 64,
        }
    }
}

/// A whole number, with the width its suffix names, if any.
struct Integer {
    negative: bool,
    magnitude: u128,
    width: Option<Width>,
    /// `Some(true)` for `s`, `Some(false)` for `u`. Without either, hex and
    /// binary digits are unsigned and decimal digits are signed.
    signed: Option<bool>,
    decimal: bool,
}

impl Integer {
    fn fits(&self, width: Width) -> bool {
        let bits = width.bits();
        if self.signed.unwrap_or(self.decimal) {
            let limit = 1u128 << (bits - 1);
            if self.negative {
                self.magnitude <= limit
            } else {
                self.magnitude < limit
            }
        } else {
            !self.negative && self.magnitude < 1u128 << bits
        }
    }

    fn value(&self) -> Value {
        if self.negative {
            return Value::Number((-(self.magnitude as i128) as i64).into());
        }
        u64::try_from(self.magnitude)
            .map_or(Value::Null, |magnitude| Value::Number(magnitude.into()))
    }
}

enum Numeric {
    Whole(Integer),
    Decimal(f64),
}

struct Parser {
    chars: Vec<char>,
    position: usize,
    depth: usize,
}

impl Parser {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(is_java_whitespace) {
            self.position += 1;
        }
    }

    fn eat(&mut self, expected: char) -> bool {
        self.skip_spaces();
        let found = self.peek() == Some(expected);
        if found {
            self.position += 1;
        }
        found
    }

    fn expected(&self, what: &str) -> String {
        match self.peek() {
            Some(found) => format!("Invalid SNBT: expected {what} but found `{found}`"),
            None => format!("Invalid SNBT: expected {what}"),
        }
    }

    fn value(&mut self) -> Result<Value, String> {
        self.skip_spaces();
        match self.peek() {
            Some('{') => self.nested(Self::compound),
            Some('[') => self.nested(Self::list),
            Some('"' | '\'') => self.quoted().map(Value::String),
            Some(ch) if ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.') => {
                let word = self.word();
                match number(&word)? {
                    Numeric::Whole(integer)
                        if integer.fits(integer.width.unwrap_or(Width::Int)) =>
                    {
                        Ok(integer.value())
                    }
                    Numeric::Whole(_) => Err(format!("Invalid SNBT: `{word}` is out of range")),
                    Numeric::Decimal(decimal) => {
                        Ok(Number::from_f64(decimal).map_or(Value::Null, Value::Number))
                    }
                }
            }
            Some(ch) if is_word_char(ch) => self.word_or_call(),
            _ => Err(self.expected("a value")),
        }
    }

    /// Reads what holds other values with `read`, or skips it past the limit.
    fn nested(&mut self, read: fn(&mut Self) -> Result<Value, String>) -> Result<Value, String> {
        if self.depth == MAX_NESTING {
            let rest: String = self.chars[self.position..].iter().collect();
            let mut reader = StringReader::new(&rest);
            if !reader.read_nbt() {
                return Err(self.expected("the end of the value"));
            }
            self.position += reader.cursor();
            return Ok(Value::Null);
        }
        self.depth += 1;
        let value = read(self);
        self.depth -= 1;
        value
    }

    fn compound(&mut self) -> Result<Value, String> {
        self.position += 1;
        let mut entries = Map::new();
        loop {
            if self.eat('}') {
                return Ok(Value::Object(entries));
            }
            self.skip_spaces();
            let quoted = matches!(self.peek(), Some('"' | '\''));
            let key = if quoted { self.quoted()? } else { self.word() };
            if key.is_empty() {
                return Err(if quoted {
                    "Invalid SNBT: a key can't be empty".to_string()
                } else {
                    self.expected("a key")
                });
            }
            if !self.eat(':') {
                return Err(self.expected("`:`"));
            }
            let value = self.value()?;
            entries.insert(key, value);
            if self.eat('}') {
                return Ok(Value::Object(entries));
            }
            if !self.eat(',') {
                return Err(self.expected("`,` or `}`"));
            }
        }
    }

    fn list(&mut self) -> Result<Value, String> {
        self.position += 1;
        self.skip_spaces();
        if let Some(width) = self.array_prefix() {
            return self.array(width);
        }
        let mut items = Vec::new();
        loop {
            if self.eat(']') {
                return Ok(Value::Array(items));
            }
            items.push(self.value()?);
            if self.eat(']') {
                return Ok(Value::Array(items));
            }
            if !self.eat(',') {
                return Err(self.expected("`,` or `]`"));
            }
        }
    }

    /// Reads the `I;` of `[I;1,2]`, if it is there.
    fn array_prefix(&mut self) -> Option<Width> {
        let width = match self.peek()? {
            'B' => Width::Byte,
            'I' => Width::Int,
            'L' => Width::Long,
            _ => return None,
        };
        let start = self.position;
        self.position += 1;
        if self.eat(';') {
            return Some(width);
        }
        self.position = start;
        None
    }

    fn array(&mut self, width: Width) -> Result<Value, String> {
        let mut items = Vec::new();
        loop {
            if self.eat(']') {
                return Ok(Value::Array(items));
            }
            self.skip_spaces();
            if !self
                .peek()
                .is_some_and(|ch| ch.is_ascii_digit() || matches!(ch, '-' | '+' | '.'))
            {
                return Err(self.expected("a number in the array"));
            }
            let word = self.word();
            let Numeric::Whole(integer) = number(&word)? else {
                return Err(format!(
                    "Invalid SNBT: `{word}` can't go in a whole number array"
                ));
            };
            if integer.width.is_some_and(|element| element > width) {
                return Err(format!("Invalid SNBT: `{word}` is too wide for this array"));
            }
            if !integer.fits(integer.width.unwrap_or(width)) {
                return Err(format!("Invalid SNBT: `{word}` is out of range"));
            }
            items.push(integer.value());
            if self.eat(']') {
                return Ok(Value::Array(items));
            }
            if !self.eat(',') {
                return Err(self.expected("`,` or `]`"));
            }
        }
    }

    fn word(&mut self) -> String {
        let start = self.position;
        while self.peek().is_some_and(is_word_char) {
            self.position += 1;
        }
        self.chars[start..self.position].iter().collect()
    }

    /// A bare word, or the `bool(...)` and `uuid(...)` operations.
    fn word_or_call(&mut self) -> Result<Value, String> {
        let word = self.word();
        let after_word = self.position;
        if !self.eat('(') {
            self.position = after_word;
            return Ok(if word.eq_ignore_ascii_case("true") {
                Value::Bool(true)
            } else if word.eq_ignore_ascii_case("false") {
                Value::Bool(false)
            } else {
                Value::String(word)
            });
        }
        let argument = self.nested(Self::value)?;
        self.eat(',');
        if !self.eat(')') {
            return Err(self.expected("`)`"));
        }
        match (word.as_str(), argument) {
            ("bool", Value::Bool(flag)) => Ok(Value::Bool(flag)),
            ("bool", Value::Number(number)) => {
                Ok(Value::Bool(number.as_f64().is_some_and(|n| n != 0.0)))
            }
            ("uuid", Value::String(uuid)) if is_uuid(&uuid) => Ok(Value::String(uuid)),
            ("bool" | "uuid", _) => Err(format!(
                "Invalid SNBT: `{word}(...)` can't take this argument"
            )),
            _ => Err(format!("Invalid SNBT: there is no operation `{word}`")),
        }
    }

    fn quoted(&mut self) -> Result<String, String> {
        let quote = self.peek().unwrap_or('"');
        self.position += 1;
        let mut text = String::new();
        loop {
            let Some(ch) = self.peek() else {
                return Err("Invalid SNBT: a quoted string isn't closed".to_string());
            };
            self.position += 1;
            if ch == quote {
                return Ok(text);
            }
            if ch == '\\' {
                self.escape(&mut text)?;
            } else {
                text.push(ch);
            }
        }
    }

    fn escape(&mut self, text: &mut String) -> Result<(), String> {
        let Some(ch) = self.peek() else {
            return Err("Invalid SNBT: a quoted string isn't closed".to_string());
        };
        self.position += 1;
        match ch {
            '\\' | '"' | '\'' => text.push(ch),
            'b' => text.push('\u{8}'),
            's' => text.push(' '),
            't' => text.push('\t'),
            'n' => text.push('\n'),
            'f' => text.push('\u{c}'),
            'r' => text.push('\r'),
            'x' => text.push(self.hex_escape(2)?),
            'u' => text.push(self.hex_escape(4)?),
            'U' => text.push(self.hex_escape(8)?),
            'N' => self.named_escape()?,
            other => return Err(format!("Invalid SNBT: `\\{other}` is not an escape")),
        }
        Ok(())
    }

    fn hex_escape(&mut self, digits: usize) -> Result<char, String> {
        let end = self.position + digits;
        let hex: String = self.chars.iter().skip(self.position).take(digits).collect();
        let code = (hex.chars().count() == digits && hex.chars().all(|ch| ch.is_ascii_hexdigit()))
            .then(|| u32::from_str_radix(&hex, 16).ok())
            .flatten();
        let Some(code) = code.filter(|code| *code <= 0x10FFFF) else {
            return Err("Invalid SNBT: a `\\x`, `\\u`, or `\\U` escape is broken".to_string());
        };
        self.position = end;
        // A lone surrogate reads as text in Minecraft and has no `char`.
        Ok(char::from_u32(code).unwrap_or('\u{FFFD}'))
    }

    /// `\N{LATIN SMALL LETTER A}`. Only the shape of the name is checked: the
    /// list of Unicode names isn't available here.
    fn named_escape(&mut self) -> Result<(), String> {
        let broken = || "Invalid SNBT: a `\\N{...}` escape is broken".to_string();
        if self.peek() != Some('{') {
            return Err(broken());
        }
        self.position += 1;
        let start = self.position;
        while self
            .peek()
            .is_some_and(|ch| ch.is_ascii_alphanumeric() || matches!(ch, ' ' | '-'))
        {
            self.position += 1;
        }
        if self.position == start || self.peek() != Some('}') {
            return Err(broken());
        }
        self.position += 1;
        Ok(())
    }
}

fn is_word_char(ch: char) -> bool {
    ch.is_ascii_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+')
}

/// Five groups of hex digits, as Java's `UUID.fromString` reads: at most 36
/// characters, and each group a long, which can start with `+`.
pub(super) fn is_uuid(text: &str) -> bool {
    let groups: Vec<&str> = text.split('-').collect();
    text.len() <= 36
        && groups.len() == 5
        && groups
            .iter()
            .all(|group| i64::from_str_radix(group, 16).is_ok())
}

/// Digits with `_` between them, but not at either end.
fn digit_run(text: &str, is_digit: impl Fn(char) -> bool) -> usize {
    if text.starts_with('_') {
        return 0;
    }
    let end = text
        .char_indices()
        .find(|(_, ch)| !is_digit(*ch) && *ch != '_')
        .map_or(text.len(), |(index, _)| index);
    text[..end].trim_end_matches('_').len()
}

fn number(word: &str) -> Result<Numeric, String> {
    let invalid = || format!("Invalid SNBT: `{word}` is not a number; quote it if it is text");
    let (negative, unsigned) = match word.as_bytes().first() {
        Some(b'-') => (true, &word[1..]),
        Some(b'+') => (false, &word[1..]),
        _ => (false, word),
    };
    let radix = match unsigned.get(..2) {
        Some("0x" | "0X") => Some((16, &unsigned[2..])),
        Some("0b" | "0B") if digit_run(&unsigned[2..], |ch| matches!(ch, '0' | '1')) > 0 => {
            Some((2, &unsigned[2..]))
        }
        _ => None,
    };
    if let Some((radix, digits)) = radix {
        let length = digit_run(digits, |ch| ch.is_digit(radix));
        if length == 0 {
            return Err(invalid());
        }
        return integer(word, negative, &digits[..length], radix, &digits[length..]);
    }
    let length = digit_run(unsigned, |ch| ch.is_ascii_digit());
    let (whole, rest) = unsigned.split_at(length);
    if rest.starts_with(['.', 'e', 'E', 'f', 'F', 'd', 'D']) {
        return decimal(word, negative, whole, rest);
    }
    if whole.is_empty() || (whole.len() > 1 && whole.starts_with('0')) {
        return Err(invalid());
    }
    integer(word, negative, whole, 10, rest)
}

fn integer(
    word: &str,
    negative: bool,
    digits: &str,
    radix: u32,
    suffix: &str,
) -> Result<Numeric, String> {
    let invalid = || format!("Invalid SNBT: `{word}` is not a number; quote it if it is text");
    let (signed, width) = match suffix.to_ascii_lowercase().as_bytes() {
        [] => (None, None),
        [sign @ (b's' | b'u'), kind @ (b'b' | b's' | b'i' | b'l')] => {
            (Some(*sign == b's'), Some(*kind))
        }
        [kind @ (b'b' | b's' | b'i' | b'l')] => (None, Some(*kind)),
        _ => return Err(invalid()),
    };
    let width = width.map(|kind| match kind {
        b'b' => Width::Byte,
        b's' => Width::Short,
        b'i' => Width::Int,
        _ => Width::Long,
    });
    let digits = digits.replace('_', "");
    Ok(Numeric::Whole(Integer {
        negative,
        magnitude: u128::from_str_radix(&digits, radix).unwrap_or(u128::MAX),
        width,
        signed,
        decimal: radix == 10,
    }))
}

fn decimal(word: &str, negative: bool, whole: &str, rest: &str) -> Result<Numeric, String> {
    let invalid = || format!("Invalid SNBT: `{word}` is not a number; quote it if it is text");
    let mut text = whole.replace('_', "");
    let mut rest = rest;
    let mut digits = !whole.is_empty();
    if let Some(after_dot) = rest.strip_prefix('.') {
        let length = digit_run(after_dot, |ch| ch.is_ascii_digit());
        digits |= length > 0;
        text.push('.');
        text.push_str(&after_dot[..length].replace('_', ""));
        rest = &after_dot[length..];
    }
    if !digits {
        return Err(invalid());
    }
    if let Some(after_e) = rest.strip_prefix(['e', 'E']) {
        let unsigned = after_e.strip_prefix(['+', '-']).unwrap_or(after_e);
        let length = digit_run(unsigned, |ch| ch.is_ascii_digit());
        if length == 0 {
            return Err(invalid());
        }
        text.push('e');
        text.push_str(&after_e[..after_e.len() - unsigned.len() + length].replace('_', ""));
        rest = &unsigned[length..];
    }
    let single = match rest {
        "" | "d" | "D" => false,
        "f" | "F" => true,
        _ => return Err(invalid()),
    };
    let value: f64 = text.parse().map_err(|_| invalid())?;
    let finite = if single {
        (value as f32).is_finite()
    } else {
        value.is_finite()
    };
    if !finite {
        return Err(format!("Invalid SNBT: `{word}` is out of range"));
    }
    Ok(Numeric::Decimal(if negative { -value } else { value }))
}

#[cfg(test)]
mod tests {
    use super::{parse, MAX_NESTING};
    use serde_json::json;

    #[test]
    fn reads_what_minecraft_reads() {
        for text in [
            "0",
            "-0",
            "+0",
            "1",
            "-1",
            "+1",
            "1b",
            "1B",
            "1s",
            "1l",
            "1L",
            "1f",
            "1d",
            "1i",
            "1ub",
            "1sb",
            "1us",
            "1ss",
            "1ui",
            "1si",
            "1ul",
            "1sl",
            "1Sb",
            "1UL",
            "127b",
            "-128b",
            "255ub",
            "127sb",
            "-128sb",
            "32767s",
            "65535us",
            "2147483647",
            "-2147483648",
            "4294967295ui",
            "9223372036854775807L",
            "-9223372036854775808L",
            "18446744073709551615ul",
            "1_000",
            "1__0",
            "1_0_0",
            "1.0_0",
            "1.5_5e1_0",
            "1_000f",
            "1_000.5",
            "1_000e1",
            "0x10",
            "0xFFb",
            "0XFF",
            "0xf",
            "0xF_F",
            "0x0_0",
            "+0xF",
            "0xFFub",
            "-0x1sb",
            "-0x1si",
            "0xFFFFFFFF",
            "0xFFFFFFFFui",
            "0x7FFFFFFFsi",
            "0xFFFFFFFFFFFFFFFFL",
            "0x7FFFFFFFFFFFFFFFsl",
            "0xBb",
            "0xDd",
            "0xFf",
            "0b101",
            "0B1",
            "0b1_0",
            "0b0",
            "0b1b",
            "0b1ub",
            "0b",
            "-0b1sb",
            "1.",
            "1.e3",
            ".5",
            "-.5",
            "+.5",
            "0.5",
            "00.5",
            "01.5",
            "01f",
            "01d",
            "01e1",
            "00f",
            "1.f",
            "1.d",
            ".5f",
            ".5e1f",
            "1e3",
            "1E3",
            "1e+3",
            "1e-3",
            "1e3f",
            "1e3d",
            "1.5f",
            "1.5d",
            "0e0",
            "0.",
            "1e38f",
            "1e308d",
            "1e308",
            "1e-999",
            "1e-50f",
            "true",
            "false",
            "True",
            "TRUE",
            "abc",
            "a.b",
            "a-b",
            "a+b",
            "a_b",
            "A",
            "_1",
            "_",
            "a_",
            "__",
            "_-",
            "Infinity",
            "NaN",
            "bool(1)",
            "bool(0)",
            "bool(true)",
            "bool(1b)",
            "bool(1.5)",
            "bool(True)",
            "bool(1,)",
            "bool (1)",
            "bool( 1 )",
            "uuid(\"00000000-0000-0000-0000-000000000000\")",
            "uuid(\"a-b-c-d-e\")",
            "uuid('0-0-0-0-0')",
            "uuid( \"0-0-0-0-0\" )",
            "uuid(\"123456789-0-0-0-0\")",
            r#""a""#,
            "'a'",
            r#""a'b""#,
            r#"'a"b'"#,
            r#""\"""#,
            r"'\''",
            r#""\n""#,
            r#""\t""#,
            r#""\b""#,
            r#""\s""#,
            r#""\f""#,
            r#""\r""#,
            r#""\\""#,
            r#""\x41""#,
            r#""\u0041""#,
            r#""\U00000041""#,
            r#""\N{LATIN SMALL LETTER A}""#,
            r#""\'""#,
            r#"'\"'"#,
            r#""\U0010FFFF""#,
            r#""$""#,
            r#""""#,
            "''",
            "{}",
            "{a:1}",
            "{a:1,}",
            r#"{"a":1}"#,
            "{'a':1}",
            r#"{"a b":1}"#,
            "{1:2}",
            "{1a:2}",
            "{-a:1}",
            "{+a:1}",
            "{.a:1}",
            "{a.b:1}",
            "{a:1,a:2}",
            "{ a : 1 }",
            "{\ta:1}",
            "{true:1}",
            "{a:{b:{c:1}}}",
            "{a:[{b:1}]}",
            "[]",
            "[1]",
            "[1,]",
            "[1,\"a\"]",
            "[{},1]",
            "[[],[]]",
            "[ 1 , 2 ]",
            "[a]",
            "[a,b]",
            "[1,2.5]",
            "[1b,2s]",
            "[\"a\",1,{}]",
            "[[1],\"a\"]",
            "[I]",
            "[I,1]",
            "[I;]",
            "[B;]",
            "[L;]",
            "[I;1,2]",
            "[I;1,]",
            "[B;1b,2b]",
            "[L;1l]",
            "[I;1b]",
            "[B;1]",
            "[L;1]",
            "[ I;1]",
            "[I ;1]",
            "[I; 1]",
            "[I;0x10]",
            "[L;1,2]",
            "[I;-1]",
            "[B;-1]",
            "[B;127]",
            "[B;0xFF]",
            "[B;0xFFub]",
            "[I;0xFFFFFFFF]",
            "[L;1s]",
            "[L;1b]",
            "[I;1s]",
            "[I;1i]",
            "[B;1b,1]",
            "[B;1ub]",
            "[B;255ub]",
            "[I;4294967295ui]",
            "[L;18446744073709551615ul]",
        ] {
            assert!(parse(text).is_ok(), "{text}: {:?}", parse(text));
        }
    }

    #[test]
    fn refuses_what_minecraft_refuses() {
        for text in [
            "1abc",
            "1.2.3",
            "1a",
            "0b2",
            "0x",
            "1_",
            "1e",
            ".",
            "-",
            "+",
            "--1",
            "1.5b",
            "1.5s",
            "1.5l",
            "1u",
            "1uf",
            "256b",
            "128b",
            "256ub",
            "-1ub",
            "99999999999",
            "1e999",
            "1e999f",
            "0xFFFFFFFFF",
            "0xFFsb",
            "-0xF",
            "1e3b",
            "0_1",
            "0x_1",
            ".e3",
            "1.5e",
            "1e_3",
            "00",
            "01",
            "007",
            "-0x1",
            "-0b1",
            "0xFFFFFFFFsi",
            "0x80000000si",
            "0x8000000000000000sl",
            "2147483648",
            "-2147483649",
            "-129b",
            "128sb",
            "-129sb",
            "32768s",
            "65536us",
            "-32769s",
            "2147483648si",
            "4294967296ui",
            "9223372036854775808L",
            "-9223372036854775809L",
            "18446744073709551616ul",
            "1e39f",
            "3.5e38f",
            "1e309d",
            "1.8e308",
            "1.5_",
            "1._5",
            "1_.5",
            "1e1_",
            "1_e1",
            "1.0e",
            "0_0",
            "0_",
            "0xG",
            "0b_1",
            "0b1_",
            "00b1",
            "1ib",
            "1bb",
            "1ll",
            "1dd",
            "1fd",
            "1.5i",
            "1.5ui",
            "0x1p3",
            "1e1e1",
            "1.5.5",
            "1..5",
            ".5.5",
            "..5",
            "1e1.5",
            "0xFFFFFFFFFFFFFFFFF",
            "-0xFFsb",
            "-a",
            "+a",
            ".a",
            "a:b",
            "a/b",
            "a$b",
            "한글",
            "é",
            "abc()",
            "abc(1)",
            "bool(\"a\")",
            "uuid(\"bad\")",
            "uuid(1)",
            "bool()",
            "bool(1,2)",
            "BOOL(1)",
            "bool(abc)",
            "bool([])",
            "bool({})",
            "uuid(\"00000000000000000000000000000000\")",
            "uuid(\"0-0-0-0-0-0\")",
            "uuid(\"g-0-0-0-0\")",
            "abc def",
            r#""\a""#,
            r#""\N{}""#,
            r#""\x4""#,
            r#""\u004""#,
            r#""\q""#,
            r#""unterminated"#,
            "'x",
            r#""a"b"#,
            r#""a""b""#,
            r#""\U00110000""#,
            r#""\0""#,
            "{,}",
            "{a:1,,b:2}",
            "{a}",
            "{a:}",
            "{:1}",
            "{a:1 b:2}",
            "{a b:1}",
            "{a:1}x",
            r#"{"":1}"#,
            "{한글:1}",
            "{é:1}",
            "{a:b:c}",
            "{a$b:1}",
            "{a/b:1}",
            "{(:1}",
            "{a:(}",
            "{a:)}",
            "{a:;}",
            "{a:\\}",
            "{a:#}",
            "{a:@}",
            "{a:!}",
            "{a:~}",
            "{a:*}",
            "{a:&}",
            "{a:%}",
            "{a:=}",
            "{a:<}",
            "{a:?}",
            "{a:{{b:1}}}",
            "{a:f\"x\"}",
            "{a:\"b\"c}",
            "{\"a\"b:1}",
            "{,a:1}",
            "{a:nope(1)}",
            "[,]",
            "[1,,2]",
            "[1 2]",
            "[B;1s]",
            "[I;1L]",
            "[I;1.5]",
            "[I;a]",
            "[I;\"a\"]",
            "[i;1]",
            "[F;1]",
            "[D;1]",
            "[S;1]",
            "[S;1s]",
            "[A;1]",
            "[I;true]",
            "[B;true]",
            "[I;,]",
            "[I;1,,2]",
            "[I;[]]",
            "[B;256b]",
            "[B;1000]",
            "[I;99999999999]",
            "[B;-1ub]",
            "[B;200]",
            "[B;255]",
            "[B;128]",
            "[B;1i]",
        ] {
            assert!(parse(text).is_err(), "{text}: {:?}", parse(text));
        }
    }

    #[test]
    fn gives_the_json_form() {
        assert_eq!(
            parse(r#"[{text:"Hi",color:gold,bold:1b},{'text':'it\'s',italic:true}]"#).unwrap(),
            json!([
                {"text": "Hi", "color": "gold", "bold": 1},
                {"text": "it's", "italic": true}
            ])
        );
        assert_eq!(
            parse("{a:[I;1,-2],b:2.5f}").unwrap(),
            json!({"a": [1, -2], "b": 2.5})
        );
        assert_eq!(parse("bool(2)").unwrap(), json!(true));
    }

    #[test]
    fn stops_reading_where_minecraft_runs_out_of_stack() {
        let nested = |open: &str, inner: &str, close: &str, levels: usize| {
            format!("{}{inner}{}", open.repeat(levels), close.repeat(levels))
        };
        let run = move || {
            for levels in [MAX_NESTING, MAX_NESTING + 1, 100_000] {
                for text in [
                    nested("[", "", "]", levels),
                    nested("{a:", "1", "}", levels),
                    nested("[{a:", "1", "}]", levels / 2),
                ] {
                    assert!(parse(&text).is_ok(), "{levels} levels");
                }
            }
            // What was read keeps its form, and what wasn't counts as balanced.
            let deep = nested("[", "1abc", "]", MAX_NESTING);
            assert_eq!(parse(&format!("[1,{deep}]")).unwrap()[0], json!(1));
            assert!(parse(&format!("{}1", nested("[", "", "]", 100_000))).is_err());
            assert!(parse(&"[".repeat(100_000)).is_err());
            assert!(parse(&nested("[", "1abc", "]", MAX_NESTING - 1)).is_err());
            assert!(parse(&nested("bool(", "1", ")", MAX_NESTING)).is_ok());
            assert!(parse(&nested("bool(", "1", ")", 100_000)).is_err());
        };
        // The size of a test thread, in a debug build's larger stack frames.
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(run)
            .unwrap()
            .join()
            .unwrap();
    }
}
