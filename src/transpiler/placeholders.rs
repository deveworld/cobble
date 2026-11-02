//! Placeholders in command text, read as a Python format string reads them:
//! `{{` and `}}` are literal braces, they nest, and a `{name}` between them
//! is still a placeholder, so `{{{x}}}` is `{`, the placeholder `x`, and `}`.
//!
//! Unlike a format string, a single brace that doesn't hold a name is text,
//! because commands are full of SNBT and JSON such as `{Tags:["a"]}`. A `}`
//! closes the innermost open brace: one `}` closes a `{`, and `}}` closes a
//! `{{`. Each quoted string keeps its own count, so a brace in a string
//! doesn't change what closes outside it.

/// One part of a command's text.
#[derive(Debug, Clone, PartialEq)]
pub(in crate::transpiler) enum Piece<'a> {
    /// Text: `raw` as written, `text` with each `{{` and `}}` that is an
    /// escape read as one brace.
    Text { raw: &'a str, text: String },
    /// A `{name}` that may be a placeholder. `start` is its byte offset.
    Field {
        raw: &'a str,
        name: &'a str,
        start: usize,
    },
}

#[derive(Clone, Copy, PartialEq)]
enum Open {
    /// A `{{` escape, closed by `}}`.
    Escape,
    /// A single `{` of SNBT or JSON, closed by `}`.
    Brace,
}

/// The pieces of `text`, in order.
pub(in crate::transpiler) fn pieces(text: &str) -> Vec<Piece<'_>> {
    scan(text, false).0
}

/// Source command fields: quoted strings interpolate only with an `f`
/// prefix. Text pieces retain their original slices for source rewrites.
pub(in crate::transpiler) fn command_pieces(text: &str) -> Vec<Piece<'_>> {
    scan(text, true).0
}

/// Translate source string syntax to the internal command template. Literal
/// braces are protected from the internal template reader; `f` is removed.
pub(in crate::transpiler) fn command_template(text: &str) -> Result<String, String> {
    crate::parser::strings::validate_command_strings(text)?;
    let (pieces, single_close) = scan(text, true);
    if single_close.is_some() {
        return Err(
            "A single `}` closes the `{{` escape. Use `}}` for a literal closing brace."
                .to_string(),
        );
    }
    Ok(pieces
        .into_iter()
        .map(|piece| match piece {
            Piece::Text { text, .. } => text,
            Piece::Field { raw, .. } => raw.to_string(),
        })
        .collect())
}

/// The pieces of `text`, or an error when a single `}` closes a `{{`
/// escape, as in `{{k}:1}`, which a Python format string refuses too.
pub(in crate::transpiler) fn checked_pieces(text: &str) -> Result<Vec<Piece<'_>>, String> {
    let (pieces, single_close) = scan(text, false);
    match single_close {
        None => Ok(pieces),
        Some(_) => Err(format!(
            "A single `}}` closes the `{{{{` escape in `{}`, which a Python format string refuses. \
             Write `}}}}` for a literal brace: `{{{{{{name}}}}}}` is `{{`, the placeholder `name`, and `}}`.",
            text.trim()
        )),
    }
}

/// The first `{name}` in a string of `json` whose name `is_runtime` says
/// holds a value known only when a function runs, such as the text
/// `text.plain(p)` writes for a parameter. A file can't hold one.
pub(in crate::transpiler) fn runtime_placeholder_in_json(
    json: &serde_json::Value,
    is_runtime: &dyn Fn(&str) -> bool,
) -> Option<String> {
    match json {
        serde_json::Value::String(text) => pieces(text).into_iter().find_map(|piece| match piece {
            Piece::Field { raw, name, .. } if is_runtime(name) => Some(raw.to_string()),
            _ => None,
        }),
        serde_json::Value::Array(items) => items
            .iter()
            .find_map(|item| runtime_placeholder_in_json(item, is_runtime)),
        serde_json::Value::Object(object) => object
            .values()
            .find_map(|value| runtime_placeholder_in_json(value, is_runtime)),
        _ => None,
    }
}

/// `text` with each brace doubled, so the placeholder reading gives `text`
/// back as it is.
pub(in crate::transpiler) fn escape_braces(text: &str) -> String {
    text.replace('{', "{{").replace('}', "}}")
}

/// A resolved alias enters source-string normalization once more. Protect
/// only its unquoted braces here; quoted literal braces are protected there.
pub(in crate::transpiler) fn escape_unquoted_braces(text: &str) -> String {
    let mut out = String::new();
    let mut quote = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
        } else if matches!(ch, '\'' | '"')
            && crate::parser::strings::command_quote_start(text, index).is_some()
        {
            quote = Some(ch);
        } else if matches!(ch, '{' | '}') {
            out.push(ch);
        }
        out.push(ch);
    }
    out
}

/// The pieces of `text`, and the offset of the first single `}` that
/// closes a `{{` escape.
fn scan(text: &str, source_strings: bool) -> (Vec<Piece<'_>>, Option<usize>) {
    let mut single_close = None;
    let bytes = text.as_bytes();
    let mut pieces: Vec<Piece<'_>> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    // The quote of the string being read, and the braces open outside it.
    let mut quote: Option<(u8, Vec<Open>, bool)> = None;
    let mut escaped = false;
    let mut text_start = 0;
    let mut decoded = String::new();
    let mut index = 0;

    fn flush<'a>(
        pieces: &mut Vec<Piece<'a>>,
        decoded: &mut String,
        text: &'a str,
        start: usize,
        end: usize,
    ) {
        if start < end {
            pieces.push(Piece::Text {
                raw: &text[start..end],
                text: std::mem::take(decoded),
            });
        }
    }

    while index < bytes.len() {
        let byte = bytes[index];
        if escaped {
            escaped = false;
        } else if quote.is_some() && byte == b'\\' {
            escaped = true;
        } else if byte == b'"' || byte == b'\'' {
            match &quote {
                Some((open_quote, _, _)) if *open_quote == byte => {
                    let (_, outside, _) = quote.take().expect("checked above");
                    open = outside;
                }
                Some(_) => {}
                None => {
                    let start = crate::parser::strings::command_quote_start(text, index);
                    if !source_strings || start.is_some() {
                        let formatted = !source_strings || start.is_some_and(|start| start < index);
                        if source_strings && formatted {
                            decoded.pop();
                        }
                        quote = Some((byte, std::mem::take(&mut open), formatted));
                    }
                }
            }
        } else if source_strings
            && quote.as_ref().is_some_and(|(_, _, formatted)| !formatted)
            && matches!(byte, b'{' | b'}')
        {
            decoded.push(byte as char);
        } else if byte == b'{' {
            if bytes.get(index + 1) == Some(&b'{') {
                open.push(Open::Escape);
                decoded.push('{');
                if source_strings {
                    decoded.push('{');
                }
                index += 2;
                continue;
            }
            if let Some(end) = field_end(text, index) {
                let name = text[index + 1..end].trim();
                if is_field_name(name) {
                    flush(&mut pieces, &mut decoded, text, text_start, index);
                    pieces.push(Piece::Field {
                        raw: &text[index..=end],
                        name,
                        start: index,
                    });
                    index = end + 1;
                    text_start = index;
                    continue;
                }
            }
            open.push(Open::Brace);
        } else if byte == b'}' {
            let double = bytes.get(index + 1) == Some(&b'}');
            match open.last() {
                Some(Open::Brace) => {
                    open.pop();
                }
                Some(Open::Escape) if double => {
                    open.pop();
                    decoded.push('}');
                    if source_strings {
                        decoded.push('}');
                    }
                    index += 2;
                    continue;
                }
                None if double => {
                    decoded.push('}');
                    if source_strings {
                        decoded.push('}');
                    }
                    index += 2;
                    continue;
                }
                Some(Open::Escape) => {
                    single_close.get_or_insert(index);
                }
                None => {}
            }
        }
        let ch = text[index..].chars().next().expect("index is on a char");
        decoded.push(ch);
        index += ch.len_utf8();
    }
    flush(&mut pieces, &mut decoded, text, text_start, text.len());
    (pieces, single_close)
}

/// The byte offset of the `}` that ends a field starting at `open`, when
/// no other brace comes first.
fn field_end(text: &str, open: usize) -> Option<usize> {
    let rest = &text[open + 1..];
    let end = rest.find(['{', '}'])?;
    (rest.as_bytes()[end] == b'}').then_some(open + 1 + end)
}

/// Whether `name` may name a placeholder. Other text in braces, such as
/// `Tags:["a"]`, `a,b`, or the selector score `obj=1..`, is SNBT or JSON.
fn is_field_name(name: &str) -> bool {
    !name.is_empty() && !name.contains([':', ',', '=', '"', '\''])
}

/// `text` with each escape read as one brace and each field as written.
#[cfg(test)]
fn decoded(text: &str) -> String {
    pieces(text)
        .into_iter()
        .map(|piece| match piece {
            Piece::Text { text, .. } => text,
            Piece::Field { raw, .. } => raw.to_string(),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fields(text: &str) -> Vec<&str> {
        pieces(text)
            .into_iter()
            .filter_map(|piece| match piece {
                Piece::Field { name, .. } => Some(name),
                Piece::Text { .. } => None,
            })
            .collect()
    }

    #[test]
    fn escapes_nest_as_in_a_python_format_string() {
        assert_eq!(
            decoded("data merge storage a:b {{a:{{b:1}},c:2}}"),
            "data merge storage a:b {a:{b:1},c:2}"
        );
        assert_eq!(decoded("say {{{{a}}}}"), "say {{a}}");
        assert_eq!(decoded("say {{name}}"), "say {name}");
        assert_eq!(decoded("say a}}"), "say a}");
    }

    #[test]
    fn a_field_between_escapes_is_a_placeholder() {
        assert_eq!(fields("say {{y={x}}}"), ["x"]);
        assert_eq!(fields("say {{{x}}}"), ["x"]);
        assert_eq!(fields("say {{ {x} }}"), ["x"]);
        assert_eq!(fields("data merge storage a:b {{{k}:1}}"), ["k"]);
        assert_eq!(fields(r#"summon pig ~ ~ ~ {{Tags:["{n}"]}}"#), ["n"]);
        assert_eq!(decoded("say {{{x}}}"), "say {{x}}");
        assert_eq!(fields("say {{x}}"), Vec::<&str>::new());
    }

    #[test]
    fn a_single_close_inside_an_escape_is_refused() {
        for text in [
            "data merge storage a:c4 {{k}:1}",
            "say {{a}",
            "say {{a:{b:1}}",
        ] {
            assert!(checked_pieces(text).is_err(), "{text}");
        }
        for text in [
            "data merge storage a:c4 {{{k}:1}}",
            "say {{a}}}",
            "say a}",
            "say {{a:{b:1}}}",
            r#"say {{"}"}}"#,
        ] {
            assert!(checked_pieces(text).is_ok(), "{text}");
        }
        assert_eq!(decoded(&escape_braces("{{a}} {b}")), "{{a}} {b}");
    }

    #[test]
    fn single_braces_of_snbt_and_json_stay() {
        for text in [
            r#"tellraw @a {"text":"a","hover_event":{"action":"show_text","value":"b"}}"#,
            "data merge storage a:b {a:{b:{c:1}}}",
            "kill @e[scores={obj=1..}]",
            "kill @e[scores={a=1,b=2}]",
            r#"tellraw @a {"text":"}"}"#,
            r#"give @s stone[custom_name='{"text":"a","b":{"c":1}}']"#,
        ] {
            assert_eq!(decoded(text), text);
        }
        assert_eq!(fields(r#"tellraw @a {"text":"{x}"}"#), ["x"]);
    }
}
