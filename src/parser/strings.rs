/// The start of a quoted command argument, including its optional `f`.
/// Apostrophes inside unquoted words, such as `it's`, aren't delimiters.
pub(crate) fn command_quote_start(text: &str, quote: usize) -> Option<usize> {
    let boundary = |before: &str| {
        before.chars().next_back().is_none_or(|ch| {
            ch.is_whitespace()
                || matches!(
                    ch,
                    ':' | '='
                        | ','
                        | '{'
                        | '['
                        | '('
                        | '.'
                        | '+'
                        | '-'
                        | '*'
                        | '/'
                        | '%'
                        | '^'
                        | '<'
                        | '>'
                        | '!'
                )
        })
    };
    if quote > 0
        && matches!(text.as_bytes()[quote - 1], b'f' | b'F')
        && boundary(&text[..quote - 1])
    {
        Some(quote - 1)
    } else {
        boundary(&text[..quote]).then_some(quote)
    }
}

#[derive(Debug, PartialEq)]
pub(crate) enum FormatPart<'a> {
    Text(String),
    Name(&'a str),
}

pub(crate) fn format_literal_body(raw: &str) -> Option<(char, &str)> {
    if !matches!(raw.as_bytes().first(), Some(b'f' | b'F')) {
        return None;
    }
    let quote = *raw.as_bytes().get(1)?;
    if !matches!(quote, b'\'' | b'"') || raw.as_bytes().last() != Some(&quote) {
        return None;
    }
    Some((quote as char, raw.get(2..raw.len().checked_sub(1)?)?))
}

/// Named fields and doubled literal braces in a formatted string. This
/// release shares the `{name}` field syntax with command arguments.
pub(crate) fn format_parts(text: &str) -> Result<Vec<FormatPart<'_>>, String> {
    let mut parts = Vec::new();
    let mut literal = String::new();
    let mut index = 0;
    let mut fields = 0;
    while index < text.len() {
        let rest = &text[index..];
        if rest.starts_with("{{") || rest.starts_with("}}") {
            literal.push(rest.chars().next().unwrap());
            index += 2;
            continue;
        }
        if rest.starts_with('{') {
            fields += 1;
            if fields > 256 {
                return Err("An f-string can contain at most 256 fields.".to_string());
            }
            let close = rest
                .find('}')
                .ok_or("Unclosed f-string field: add `}` after the name.")?;
            let name = rest[1..close].trim();
            let mut chars = name.chars();
            if !chars
                .next()
                .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
                || !chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
            {
                return Err("An f-string field needs a name, as in `{wave}`. Expressions, conversions, and format specifiers aren't supported.".to_string());
            }
            if !literal.is_empty() {
                parts.push(FormatPart::Text(std::mem::take(&mut literal)));
            }
            parts.push(FormatPart::Name(name));
            index += close + 1;
            continue;
        }
        if rest.starts_with('}') {
            return Err("A literal `}` in an f-string must be written `}}`.".to_string());
        }
        let ch = rest.chars().next().unwrap();
        literal.push(ch);
        index += ch.len_utf8();
    }
    if !literal.is_empty() {
        parts.push(FormatPart::Text(literal));
    }
    Ok(parts)
}

pub(crate) fn validate_command_strings(text: &str) -> Result<(), String> {
    formatted_string_ranges(text).map(|_| ())
}

/// Quoted f-string bodies, for distinguishing textual command fields from
/// values that a numeric expression reads.
pub(crate) fn formatted_string_ranges(text: &str) -> Result<Vec<std::ops::Range<usize>>, String> {
    let mut ranges = Vec::new();
    let mut quote: Option<(char, usize, bool)> = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some((delimiter, start, formatted)) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == delimiter {
                if formatted {
                    format_parts(&text[start + 1..index])?;
                    ranges.push(start + 1..index);
                }
                quote = None;
            }
        } else if matches!(ch, '\'' | '"') {
            if let Some(start) = command_quote_start(text, index) {
                if start < index
                    && (text[index..].starts_with("\"\"\"") || text[index..].starts_with("'''"))
                {
                    return Err(
                        "Triple-quoted f-strings aren't supported. Use a single-line f-string."
                            .to_string(),
                    );
                }
                quote = Some((ch, index, start < index));
            }
        }
    }
    if quote.is_some_and(|(_, _, formatted)| formatted) {
        return Err("Unterminated f-string: add the closing quote.".to_string());
    }
    Ok(ranges)
}
