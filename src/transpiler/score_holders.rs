//! Finds the score holders a command reads and writes on one objective.
//!
//! Function-local variables are renamed after a function is lowered (see
//! `docs/src/reference/calling-convention.md`), and a recursive function saves
//! every score it writes. Both work on finished commands, so they need to know
//! where a command names a score holder: `scoreboard players` arguments,
//! `execute if|unless score` and `store ... score` arguments, and score text
//! components. Commands reach this module after the `temp` objective was
//! mapped to the pack's own objective.

use std::borrow::Cow;
use std::collections::HashMap;

/// One score holder named in a command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) struct HolderRef {
    pub(in crate::transpiler) start: usize,
    pub(in crate::transpiler) end: usize,
    /// Whether the command can change this holder's score.
    pub(in crate::transpiler) written: bool,
}

/// Every holder on `objective` that `command` names, in text order.
pub(in crate::transpiler) fn holder_refs(command: &str, objective: &str) -> Vec<HolderRef> {
    if !command.contains(objective) {
        return Vec::new();
    }
    let tokens = tokenize(command);
    let mut refs = Vec::new();
    collect_command(command, &tokens, 0, objective, &mut refs);
    for &(start, end) in &tokens {
        let token = &command[start..end];
        if token.starts_with('{') || token.starts_with('[') {
            collect_component_names(token, start, objective, &mut refs);
        }
    }
    refs.sort_by_key(|holder| holder.start);
    refs.dedup_by_key(|holder| holder.start);
    refs
}

/// The holders on `objective` whose score `command` can change.
pub(in crate::transpiler) fn written_holders<'a>(
    command: &'a str,
    objective: &str,
) -> Vec<&'a str> {
    holder_refs(command, objective)
        .into_iter()
        .filter(|holder| holder.written)
        .map(|holder| &command[holder.start..holder.end])
        .collect()
}

/// Renames holders on `objective` that appear as keys of `renames`.
pub(in crate::transpiler) fn rename_holders<'a>(
    command: &'a str,
    objective: &str,
    renames: &HashMap<String, String>,
) -> Cow<'a, str> {
    let refs: Vec<(HolderRef, &String)> = holder_refs(command, objective)
        .into_iter()
        .filter_map(|holder| {
            renames
                .get(&command[holder.start..holder.end])
                .map(|renamed| (holder, renamed))
        })
        .collect();
    if refs.is_empty() {
        return Cow::Borrowed(command);
    }
    let mut renamed = String::with_capacity(command.len() + refs.len() * 8);
    let mut cursor = 0;
    for (holder, name) in refs {
        renamed.push_str(&command[cursor..holder.start]);
        renamed.push_str(name);
        cursor = holder.end;
    }
    renamed.push_str(&command[cursor..]);
    Cow::Owned(renamed)
}

/// Splits a command into whitespace-separated token spans, keeping brackets,
/// braces, and quoted strings inside one token.
fn tokenize(command: &str) -> Vec<(usize, usize)> {
    let mut tokens = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start: Option<usize> = None;

    for (index, ch) in command.char_indices() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == open {
                quote = None;
            }
            continue;
        }
        if ch.is_whitespace() && depth == 0 {
            if let Some(token_start) = start.take() {
                tokens.push((token_start, index));
            }
            continue;
        }
        if start.is_none() {
            start = Some(index);
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    if let Some(token_start) = start {
        tokens.push((token_start, command.len()));
    }
    tokens
}

fn collect_command(
    command: &str,
    tokens: &[(usize, usize)],
    first: usize,
    objective: &str,
    out: &mut Vec<HolderRef>,
) {
    let word = |index: usize| -> Option<&str> {
        tokens.get(index).map(|&(start, end)| {
            let text = &command[start..end];
            if index == 0 {
                text.strip_prefix('$').unwrap_or(text)
            } else {
                text
            }
        })
    };
    // A holder at `index` counts when the token after it is `objective`.
    let holder = |index: usize, written: bool, out: &mut Vec<HolderRef>| {
        let (Some(&(start, end)), Some(&(objective_start, objective_end))) =
            (tokens.get(index), tokens.get(index + 1))
        else {
            return;
        };
        if &command[objective_start..objective_end] != objective {
            return;
        }
        let start = if index == 0 && command[start..end].starts_with('$') {
            start + 1
        } else {
            start
        };
        out.push(HolderRef {
            start,
            end,
            written,
        });
    };

    match word(first) {
        Some("execute") => {
            let mut index = first + 1;
            while let Some(token) = word(index) {
                match token {
                    "run" => {
                        collect_command(command, tokens, index + 1, objective, out);
                        return;
                    }
                    "if" | "unless" if word(index + 1) == Some("score") => {
                        holder(index + 2, false, out);
                        match word(index + 4) {
                            Some("matches") => index += 6,
                            Some("<" | "<=" | "=" | ">=" | ">") => {
                                holder(index + 5, false, out);
                                index += 7;
                            }
                            _ => index += 4,
                        }
                    }
                    "store"
                        if matches!(word(index + 1), Some("result" | "success"))
                            && word(index + 2) == Some("score") =>
                    {
                        holder(index + 3, true, out);
                        index += 5;
                    }
                    _ => index += 1,
                }
            }
        }
        Some("return") if word(first + 1) == Some("run") => {
            collect_command(command, tokens, first + 2, objective, out);
        }
        Some("scoreboard") if word(first + 1) == Some("players") => match word(first + 2) {
            Some("set" | "add" | "remove" | "reset" | "enable") => holder(first + 3, true, out),
            Some("get") => holder(first + 3, false, out),
            Some("operation") => {
                holder(first + 3, true, out);
                // `><` swaps, so it writes both sides.
                holder(first + 6, word(first + 5) == Some("><"), out);
            }
            _ => {}
        },
        _ => {}
    }
}

/// Finds `name` values of score text components (`{"score":{"name":"x",
/// "objective":"pack.vars"}}`, or the SNBT form with unquoted keys) whose
/// object names `objective`. Text components only read scores.
fn collect_component_names(token: &str, offset: usize, objective: &str, out: &mut Vec<HolderRef>) {
    // Per open object: the span of its `name` value and whether its
    // `objective` value matched.
    let mut objects: Vec<(Option<(usize, usize)>, bool)> = Vec::new();
    let bytes = token.as_bytes();
    let mut index = 0;
    let mut pending_key: Option<&str> = None;
    while index < bytes.len() {
        match bytes[index] {
            b'{' => {
                objects.push((None, false));
                pending_key = None;
                index += 1;
            }
            b'}' => {
                if let Some((Some((start, end)), true)) = objects.pop() {
                    out.push(HolderRef {
                        start: offset + start,
                        end: offset + end,
                        written: false,
                    });
                }
                pending_key = None;
                index += 1;
            }
            b'"' | b'\'' => {
                let quote = bytes[index];
                let start = index + 1;
                let mut end = start;
                while end < bytes.len() && bytes[end] != quote {
                    end += if bytes[end] == b'\\' { 2 } else { 1 };
                }
                let end = end.min(bytes.len());
                let text = &token[start..end];
                index = end + 1;
                let after = token[index.min(token.len())..].trim_start();
                if after.starts_with(':') {
                    pending_key = Some(text);
                } else if let Some(key) = pending_key.take() {
                    record_value(&mut objects, key, (start, end), text, objective);
                }
            }
            b':' | b',' | b' ' | b'[' | b']' => index += 1,
            _ => {
                let start = index;
                index += 1;
                while index < bytes.len() && !b"{}[]:,\"' ".contains(&bytes[index]) {
                    index += 1;
                }
                let text = &token[start..index];
                if token[index..].trim_start().starts_with(':') {
                    pending_key = Some(text);
                } else if let Some(key) = pending_key.take() {
                    record_value(&mut objects, key, (start, index), text, objective);
                }
            }
        }
    }
}

fn record_value(
    objects: &mut [(Option<(usize, usize)>, bool)],
    key: &str,
    span: (usize, usize),
    text: &str,
    objective: &str,
) {
    let Some(object) = objects.last_mut() else {
        return;
    };
    match key {
        "name" => object.0 = Some(span),
        "objective" => object.1 = text == objective,
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const OBJ: &str = "pack.vars";

    fn names(command: &str) -> Vec<(String, bool)> {
        holder_refs(command, OBJ)
            .into_iter()
            .map(|holder| {
                (
                    command[holder.start..holder.end].to_string(),
                    holder.written,
                )
            })
            .collect()
    }

    #[test]
    fn finds_scoreboard_and_execute_holders() {
        assert_eq!(
            names("scoreboard players operation x pack.vars += y pack.vars"),
            [("x".to_string(), true), ("y".to_string(), false)]
        );
        assert_eq!(
            names("execute if score a pack.vars < b pack.vars store result score c pack.vars run scoreboard players get d pack.vars"),
            [
                ("a".to_string(), false),
                ("b".to_string(), false),
                ("c".to_string(), true),
                ("d".to_string(), false)
            ]
        );
        assert_eq!(
            names("$scoreboard players set x pack.vars $(n)"),
            [("x".to_string(), true)]
        );
        assert_eq!(names("scoreboard players set x kills 1"), []);
        assert_eq!(names("say x pack.vars"), []);
    }

    #[test]
    fn finds_score_component_names() {
        assert_eq!(
            names(r#"tellraw @a [{"text":"a"},{"score":{"name":"x","objective":"pack.vars"}}]"#),
            [("x".to_string(), false)]
        );
        assert_eq!(
            names(r#"tellraw @a {"score":{"objective":"pack.vars","name":"y"}}"#),
            [("y".to_string(), false)]
        );
        assert_eq!(
            names("tellraw @a {score:{name:'z',objective:'pack.vars'}}"),
            [("z".to_string(), false)]
        );
        assert_eq!(
            names(r#"tellraw @a {"score":{"name":"x","objective":"kills"}}"#),
            []
        );
    }

    #[test]
    fn renames_only_listed_holders() {
        let renames = HashMap::from([("x".to_string(), "#f.x".to_string())]);
        assert_eq!(
            rename_holders(
                "execute if score x pack.vars matches 1 run scoreboard players operation y pack.vars = x pack.vars",
                OBJ,
                &renames
            ),
            "execute if score #f.x pack.vars matches 1 run scoreboard players operation y pack.vars = #f.x pack.vars"
        );
        assert_eq!(
            rename_holders(
                r#"tellraw @a {"score":{"name":"x","objective":"pack.vars"},"text":"x"}"#,
                OBJ,
                &renames
            ),
            r##"tellraw @a {"score":{"name":"#f.x","objective":"pack.vars"},"text":"x"}"##
        );
        assert_eq!(
            rename_holders("say x pack.vars", OBJ, &renames),
            "say x pack.vars"
        );
    }
}
