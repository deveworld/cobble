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
    let bytes = text.as_bytes();
    let mut pieces: Vec<Piece<'_>> = Vec::new();
    let mut open: Vec<Open> = Vec::new();
    // The quote of the string being read, and the braces open outside it.
    let mut quote: Option<(u8, Vec<Open>)> = None;
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
                Some((open_quote, _)) if *open_quote == byte => {
                    let (_, outside) = quote.take().expect("checked above");
                    open = outside;
                }
                Some(_) => {}
                None => quote = Some((byte, std::mem::take(&mut open))),
            }
        } else if byte == b'{' {
            if bytes.get(index + 1) == Some(&b'{') {
                open.push(Open::Escape);
                decoded.push('{');
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
                    index += 2;
                    continue;
                }
                None if double => {
                    decoded.push('}');
                    index += 2;
                    continue;
                }
                _ => {}
            }
        }
        let ch = text[index..].chars().next().expect("index is on a char");
        decoded.push(ch);
        index += ch.len_utf8();
    }
    flush(&mut pieces, &mut decoded, text, text_start, text.len());
    pieces
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
