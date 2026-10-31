//! Maps the source-level `temp` objective to the pack's own variable objective.
//!
//! Cobble source, the compiler's own lowering, and raw commands all name the
//! variable objective `temp`. Every command that enters the data pack passes
//! through [`map_command`], which rewrites `temp` to `<namespace>.vars` where a
//! command expects an objective: `scoreboard` objective arguments, `execute`
//! `if|unless score` and `store ... score` arguments, `trigger`,
//! `scores={...}` selector filters and `scores:{...}` predicate maps, and
//! score text components and number providers, also inside an item's
//! components. A macro placeholder that stands for a word, as `$(op)` in
//! `scoreboard players $(op) x temp 1`, leaves the objective where it is
//! for every word the placeholder can be. Other text, such as chat messages
//! or a fake player named `temp`, stays unchanged. Resource JSON, such as a
//! predicate, goes through [`map_json`].

use std::borrow::Cow;

/// The objective name Cobble source uses for the pack's variables.
pub const SOURCE_OBJECTIVE: &str = "temp";

/// The objective that holds a pack's variables and internal helper scores.
pub fn vars_objective(namespace: &str) -> String {
    format!("{namespace}.vars")
}

/// Rewrites `temp` objective references in one command to `target`.
pub fn map_command<'a>(command: &'a str, target: &str) -> Cow<'a, str> {
    if !command.contains(SOURCE_OBJECTIVE) {
        return Cow::Borrowed(command);
    }

    let tokens = tokenize(command);
    let mut replacements: Vec<(usize, usize)> = Vec::new();
    collect_command(command, &tokens, 0, &mut replacements);

    for &(start, end) in &tokens {
        let token = &command[start..end];
        if token.contains(['[', '{']) {
            collect_scores_filters(token, start, &mut replacements);
            collect_component_objectives(token, start, &mut replacements);
        }
    }

    if replacements.is_empty() {
        return Cow::Borrowed(command);
    }
    replacements.sort_unstable();
    replacements.dedup();

    let mut mapped = String::with_capacity(command.len() + replacements.len() * target.len());
    let mut cursor = 0;
    for (start, end) in replacements {
        mapped.push_str(&command[cursor..start]);
        mapped.push_str(target);
        cursor = end;
    }
    mapped.push_str(&command[cursor..]);
    Cow::Owned(mapped)
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
    out: &mut Vec<(usize, usize)>,
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
    let objective = |index: usize, out: &mut Vec<(usize, usize)>| {
        if let Some(&(start, end)) = tokens.get(index) {
            if &command[start..end] == SOURCE_OBJECTIVE {
                out.push((start, end));
            }
        }
    };
    // A macro placeholder standing for a whole word, such as `$(op)`.
    let placeholder = |index: usize| word(index).is_some_and(is_placeholder_word);

    match word(first) {
        Some("execute") => {
            let mut index = first + 1;
            while let Some(token) = word(index) {
                match token {
                    "run" => {
                        collect_command(command, tokens, index + 1, out);
                        return;
                    }
                    "if" | "unless" if word(index + 1) == Some("score") => {
                        objective(index + 3, out);
                        match word(index + 4) {
                            Some("matches") => index += 6,
                            Some("<" | "<=" | "=" | ">=" | ">") => {
                                objective(index + 6, out);
                                index += 7;
                            }
                            // `matches <range>` or an operator: after a
                            // range comes the next subcommand, never `temp`.
                            _ if placeholder(index + 4) => {
                                objective(index + 6, out);
                                index += 5;
                            }
                            _ => index += 4,
                        }
                    }
                    "store"
                        if (matches!(word(index + 1), Some("result" | "success"))
                            || placeholder(index + 1))
                            && word(index + 2) == Some("score") =>
                    {
                        objective(index + 4, out);
                        index += 5;
                    }
                    _ => index += 1,
                }
            }
        }
        Some("return") if word(first + 1) == Some("run") => {
            collect_command(command, tokens, first + 2, out);
        }
        Some("trigger") => objective(first + 1, out),
        Some("scoreboard") => match (word(first + 1), word(first + 2)) {
            (Some("players"), Some("set" | "add" | "remove" | "get" | "reset" | "enable")) => {
                objective(first + 4, out);
            }
            // Every `players` subcommand but `display` has its objective
            // fourth, and `operation` has its second seventh. `display`
            // has a holder there, which would not be named `temp`.
            (Some("players"), Some(_)) if placeholder(first + 2) => {
                objective(first + 4, out);
                objective(first + 7, out);
            }
            // `objectives add|remove|modify` name the objective third;
            // `setdisplay` has a slot there, which is never `temp`.
            (Some("objectives"), Some(_)) if placeholder(first + 2) => {
                objective(first + 3, out);
            }
            (Some("players"), Some("operation")) => {
                objective(first + 4, out);
                objective(first + 7, out);
            }
            (Some("players"), Some("display")) => objective(first + 5, out),
            (Some("objectives"), Some("add" | "remove" | "modify")) => objective(first + 3, out),
            (Some("objectives"), Some("setdisplay")) => objective(first + 4, out),
            _ => {}
        },
        _ => {}
    }
}

/// Whether `word` is a macro placeholder and nothing else, as `$(op)`.
fn is_placeholder_word(word: &str) -> bool {
    word.strip_prefix("$(")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|name| !name.is_empty() && !name.contains(['(', ')']))
}

/// Rewrites `temp` keys inside `scores={...}` selector filters and the
/// `scores:{...}` map of an `entity_scores` predicate.
fn collect_scores_filters(token: &str, offset: usize, out: &mut Vec<(usize, usize)>) {
    for (opener, separator) in [("scores={", b'='), ("scores:{", b':'), ("scores\":{", b':')] {
        collect_scores_maps(token, offset, opener, separator, out);
    }
}

fn collect_scores_maps(
    token: &str,
    offset: usize,
    opener: &str,
    separator: u8,
    out: &mut Vec<(usize, usize)>,
) {
    let bytes = token.as_bytes();
    let mut search = 0;
    while let Some(found) = token[search..].find(opener) {
        let key_start = search + found;
        let body_start = key_start + opener.len();
        // A key of its own, not the end of a longer name.
        let before = token[..key_start].chars().next_back();
        if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
            search = body_start;
            continue;
        }
        let mut depth = 1usize;
        let mut index = body_start;
        let mut entry_start = body_start;
        let mut in_value = false;
        while index < bytes.len() && depth > 0 {
            match bytes[index] {
                b'{' => depth += 1,
                b'}' => depth -= 1,
                byte if byte == separator && depth == 1 && !in_value => {
                    let key = &token[entry_start..index];
                    let trimmed = key.trim();
                    let unquoted = trimmed
                        .strip_prefix(['"', '\''])
                        .and_then(|rest| rest.strip_suffix(['"', '\'']))
                        .unwrap_or(trimmed);
                    if unquoted == SOURCE_OBJECTIVE {
                        let key_start = entry_start
                            + (key.len() - key.trim_start().len())
                            + (trimmed.len() - unquoted.len()) / 2;
                        out.push((
                            offset + key_start,
                            offset + key_start + SOURCE_OBJECTIVE.len(),
                        ));
                    }
                    in_value = true;
                }
                b',' if depth == 1 => {
                    entry_start = index + 1;
                    in_value = false;
                }
                _ => {}
            }
            index += 1;
        }
        search = index;
    }
}

/// Rewrites `temp` in resource JSON as [`map_command`] does in commands:
/// the `objective` of a score text component, the `score` of a score number
/// provider, and the keys of a `scores` map.
pub fn map_json(value: &mut serde_json::Value, target: &str) {
    match value {
        serde_json::Value::Object(object) => {
            for (key, field) in object.iter_mut() {
                match (key.as_str(), &mut *field) {
                    ("objective" | "score", serde_json::Value::String(name))
                        if name == SOURCE_OBJECTIVE =>
                    {
                        *name = target.to_string();
                    }
                    ("scores", serde_json::Value::Object(scores))
                        if scores.contains_key(SOURCE_OBJECTIVE) =>
                    {
                        let mapped = std::mem::take(scores)
                            .into_iter()
                            .map(|(name, range)| {
                                if name == SOURCE_OBJECTIVE {
                                    (target.to_string(), range)
                                } else {
                                    (name, range)
                                }
                            })
                            .collect();
                        *scores = mapped;
                    }
                    _ => {}
                }
                map_json(field, target);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                map_json(item, target);
            }
        }
        _ => {}
    }
}

/// Rewrites `temp` values of `objective` keys in JSON or SNBT text components,
/// and of `score` keys in score number providers.
fn collect_component_objectives(token: &str, offset: usize, out: &mut Vec<(usize, usize)>) {
    for key in ["objective", "score"] {
        let mut search = 0;
        while let Some(found) = token[search..].find(key) {
            let key_start = search + found;
            search = key_start + key.len();
            let before = token[..key_start].chars().next_back();
            if before.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_') {
                continue;
            }
            let rest = &token[search..];
            let mut cursor = 0;
            if before == Some('"') {
                if !rest.starts_with('"') {
                    continue;
                }
                cursor += 1;
            } else {
                // An unquoted SNBT key follows `{` or `,`; anything else is text.
                let previous = token[..key_start].trim_end().chars().next_back();
                if !matches!(previous, Some('{' | ',')) {
                    continue;
                }
            }
            cursor += rest[cursor..].len() - rest[cursor..].trim_start().len();
            if !rest[cursor..].starts_with(':') {
                continue;
            }
            cursor += 1;
            cursor += rest[cursor..].len() - rest[cursor..].trim_start().len();
            let quote = rest[cursor..]
                .chars()
                .next()
                .filter(|ch| *ch == '"' || *ch == '\'');
            if quote.is_some() {
                cursor += 1;
            }
            if !rest[cursor..].starts_with(SOURCE_OBJECTIVE) {
                continue;
            }
            let value_end = cursor + SOURCE_OBJECTIVE.len();
            let after = rest[value_end..].chars().next();
            let closed = match quote {
                Some(open) => after == Some(open),
                None => !after.is_some_and(|ch| ch.is_ascii_alphanumeric() || ch == '_'),
            };
            if closed {
                let start = offset + search + cursor;
                out.push((start, start + SOURCE_OBJECTIVE.len()));
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::map_command;

    fn map(command: &str) -> String {
        map_command(command, "pack.vars").into_owned()
    }

    #[test]
    fn maps_scoreboard_objective_arguments() {
        assert_eq!(
            map("scoreboard players set temp temp 5"),
            "scoreboard players set temp pack.vars 5"
        );
        assert_eq!(
            map("scoreboard players operation x temp *= #multiplier temp"),
            "scoreboard players operation x pack.vars *= #multiplier pack.vars"
        );
        assert_eq!(
            map("scoreboard objectives add temp dummy"),
            "scoreboard objectives add pack.vars dummy"
        );
        assert_eq!(
            map("scoreboard objectives setdisplay sidebar temp"),
            "scoreboard objectives setdisplay sidebar pack.vars"
        );
        assert_eq!(
            map("scoreboard players display name @s temp \"x\""),
            "scoreboard players display name @s pack.vars \"x\""
        );
        assert_eq!(
            map("scoreboard players reset @s temp"),
            "scoreboard players reset @s pack.vars"
        );
    }

    #[test]
    fn maps_execute_score_conditions_and_stores() {
        assert_eq!(
            map("execute if score x temp matches 1.. unless score a temp < b temp run say temp"),
            "execute if score x pack.vars matches 1.. unless score a pack.vars < b pack.vars run say temp"
        );
        assert_eq!(
            map("execute store result score x temp run scoreboard players get @s pk_time"),
            "execute store result score x pack.vars run scoreboard players get @s pk_time"
        );
        assert_eq!(
            map("execute store result storage ns:global v int 1 run scoreboard players get v temp"),
            "execute store result storage ns:global v int 1 run scoreboard players get v pack.vars"
        );
        assert_eq!(
            map("$execute if score $(x) temp matches 1 run return run scoreboard players get y temp"),
            "$execute if score $(x) pack.vars matches 1 run return run scoreboard players get y pack.vars"
        );
    }

    #[test]
    fn maps_selector_filters_and_text_components() {
        assert_eq!(
            map("tp @a[scores={temp=1.., other=2}, tag=x] ~ ~ ~"),
            "tp @a[scores={pack.vars=1.., other=2}, tag=x] ~ ~ ~"
        );
        assert_eq!(
            map(r#"tellraw @a {"score":{"name":"x","objective":"temp"}}"#),
            r#"tellraw @a {"score":{"name":"x","objective":"pack.vars"}}"#
        );
        assert_eq!(
            map("tellraw @a {score:{name:'x',objective:temp}}"),
            "tellraw @a {score:{name:'x',objective:pack.vars}}"
        );
    }

    #[test]
    fn maps_trigger_items_and_predicate_scores() {
        assert_eq!(map("trigger temp set 1"), "trigger pack.vars set 1");
        assert_eq!(
            map(r#"give @s stone[custom_name={"score":{"name":"x","objective":"temp"}}]"#),
            r#"give @s stone[custom_name={"score":{"name":"x","objective":"pack.vars"}}]"#
        );
        assert_eq!(
            map(
                r#"execute if predicate {type:"minecraft:entity_scores",entity:"this",scores:{temp:1}} run say a"#
            ),
            r#"execute if predicate {type:"minecraft:entity_scores",entity:"this",scores:{pack.vars:1}} run say a"#
        );
        assert_eq!(
            map(
                r#"execute if predicate {"condition":"minecraft:entity_scores","scores":{"temp":{"min":1}}} run say a"#
            ),
            r#"execute if predicate {"condition":"minecraft:entity_scores","scores":{"pack.vars":{"min":1}}} run say a"#
        );
    }

    #[test]
    fn maps_the_objective_after_a_placeholder_word() {
        assert_eq!(
            map("$scoreboard players $(op) coins temp 1"),
            "$scoreboard players $(op) coins pack.vars 1"
        );
        assert_eq!(
            map("$scoreboard players $(op) a temp += b temp"),
            "$scoreboard players $(op) a pack.vars += b pack.vars"
        );
        assert_eq!(
            map("$scoreboard objectives $(op) temp dummy"),
            "$scoreboard objectives $(op) pack.vars dummy"
        );
        assert_eq!(
            map("$execute store $(a) score x temp run say hi"),
            "$execute store $(a) score x pack.vars run say hi"
        );
        assert_eq!(
            map("$execute if score x temp $(o) y temp run say hi"),
            "$execute if score x pack.vars $(o) y pack.vars run say hi"
        );
    }

    #[test]
    fn maps_resource_json() {
        let mut json = serde_json::json!({
            "condition": "minecraft:entity_scores",
            "scores": {"temp": 1, "other": 2},
            "value": {"type": "minecraft:score", "score": "temp"},
            "text": {"score": {"name": "x", "objective": "temp"}},
            "name": "temp"
        });
        super::map_json(&mut json, "pack.vars");
        assert_eq!(
            json,
            serde_json::json!({
                "condition": "minecraft:entity_scores",
                "scores": {"pack.vars": 1, "other": 2},
                "value": {"type": "minecraft:score", "score": "pack.vars"},
                "text": {"score": {"name": "x", "objective": "pack.vars"}},
                "name": "temp"
            })
        );
    }

    #[test]
    fn leaves_other_text_alone() {
        for command in [
            "say temp score is temp",
            "scoreboard players set x points 5",
            "scoreboard objectives add temperature dummy",
            "tellraw @a {\"text\":\"temp\"}",
            "tellraw @a {\"text\":\"my objective: temp\"}",
            "data modify storage ns:global temp set value 1",
            "scoreboard players list temp",
        ] {
            assert_eq!(map(command), command);
        }
    }
}
