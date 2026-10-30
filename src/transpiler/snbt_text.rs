//! SNBT text components as JSON.
//!
//! Minecraft reads `{text:"Score",color:gold}`, and that is how text is
//! usually written since 1.21.5. Placeholders are filled in on the JSON form of
//! a component, so a component with a placeholder is read here first.

use serde_json::{Map, Number, Value};

/// The JSON form of `text`, when it is a component in SNBT. JSON itself and
/// anything that doesn't parse give `None`.
pub(in crate::transpiler) fn component_json(text: &str) -> Option<String> {
    if !text.starts_with(['{', '[']) || serde_json::from_str::<Value>(text).is_ok() {
        return None;
    }
    let chars: Vec<char> = text.chars().collect();
    let mut reader = Reader { chars, position: 0 };
    let value = reader.value()?;
    reader.skip_spaces();
    (reader.position == reader.chars.len()).then(|| value.to_string())
}

struct Reader {
    chars: Vec<char>,
    position: usize,
}

impl Reader {
    fn peek(&self) -> Option<char> {
        self.chars.get(self.position).copied()
    }

    fn skip_spaces(&mut self) {
        while self.peek().is_some_and(char::is_whitespace) {
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

    fn value(&mut self) -> Option<Value> {
        self.skip_spaces();
        match self.peek()? {
            '{' => self.compound(),
            '[' => self.list(),
            '"' | '\'' => self.quoted().map(Value::String),
            _ => self.bare().map(bare_value),
        }
    }

    fn compound(&mut self) -> Option<Value> {
        self.position += 1;
        let mut entries = Map::new();
        if self.eat('}') {
            return Some(Value::Object(entries));
        }
        loop {
            self.skip_spaces();
            let key = match self.peek()? {
                '"' | '\'' => self.quoted()?,
                _ => self.bare()?,
            };
            if !self.eat(':') {
                return None;
            }
            entries.insert(key, self.value()?);
            if self.eat('}') {
                return Some(Value::Object(entries));
            }
            if !self.eat(',') {
                return None;
            }
        }
    }

    fn list(&mut self) -> Option<Value> {
        self.position += 1;
        // A typed array, as a UUID in `[I;1,2,3,4]`, holds plain numbers.
        if matches!(self.peek(), Some('B' | 'I' | 'L'))
            && self.chars.get(self.position + 1) == Some(&';')
        {
            self.position += 2;
        }
        let mut items = Vec::new();
        if self.eat(']') {
            return Some(Value::Array(items));
        }
        loop {
            items.push(self.value()?);
            if self.eat(']') {
                return Some(Value::Array(items));
            }
            if !self.eat(',') {
                return None;
            }
        }
    }

    fn quoted(&mut self) -> Option<String> {
        let quote = self.peek()?;
        self.position += 1;
        let mut text = String::new();
        loop {
            let ch = self.peek()?;
            self.position += 1;
            match ch {
                '\\' => {
                    let escaped = self.peek()?;
                    self.position += 1;
                    text.push(match escaped {
                        'n' => '\n',
                        't' => '\t',
                        other => other,
                    });
                }
                ch if ch == quote => return Some(text),
                ch => text.push(ch),
            }
        }
    }

    fn bare(&mut self) -> Option<String> {
        let start = self.position;
        while self
            .peek()
            .is_some_and(|ch| ch.is_alphanumeric() || matches!(ch, '_' | '-' | '.' | '+'))
        {
            self.position += 1;
        }
        (self.position > start).then(|| self.chars[start..self.position].iter().collect())
    }
}

/// `true`, `1b`, `2.5f`, or a word such as a color name.
fn bare_value(text: String) -> Value {
    match text.as_str() {
        "true" => return Value::Bool(true),
        "false" => return Value::Bool(false),
        _ => {}
    }
    let digits = text.trim_end_matches(['b', 'B', 's', 'S', 'l', 'L', 'f', 'F', 'd', 'D']);
    if let Ok(number) = digits.parse::<i64>() {
        return Value::Number(number.into());
    }
    match digits.parse::<f64>().ok().and_then(Number::from_f64) {
        Some(number) => Value::Number(number),
        None => Value::String(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_forms_text_components_use() {
        let json = component_json(
            r#"[{text:"Hi {name}",color:gold,bold:1b},{'text':'it\'s',italic:true}]"#,
        )
        .unwrap();
        assert_eq!(
            serde_json::from_str::<Value>(&json).unwrap(),
            serde_json::json!([
                {"text": "Hi {name}", "color": "gold", "bold": 1},
                {"text": "it's", "italic": true}
            ])
        );
    }

    #[test]
    fn leaves_json_and_broken_text_alone() {
        assert_eq!(component_json(r#"{"text":"a"}"#), None);
        assert_eq!(component_json(r#"{text:"a""#), None);
        assert_eq!(component_json("plain"), None);
    }
}
