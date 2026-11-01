//! Maps the source-level `temp` objective to the pack's own variable objective.
//!
//! Cobble source, the compiler's own lowering, and raw commands all name the
//! variable objective `temp`. Every command that enters the data pack passes
//! through [`map_command`], which rewrites `temp` to `<namespace>.vars` where a
//! command expects an objective: `scoreboard` objective arguments, `execute`
//! `if|unless score` and `store ... score` arguments, `trigger`,
//! `scores={...}` selector filters, the `scores` map of an `entity_scores`
//! predicate, score text components and number providers, also inside an
//! item's components, and the command of a `run_command` click event. A
//! macro placeholder that stands for a word, as `$(op)` in
//! `scoreboard players $(op) x temp 1`, leaves the objective where it is
//! for every word the placeholder can be. Other text, such as chat messages,
//! a fake player named `temp`, or a `scores` key in `custom_data`, stays
//! unchanged. Resource JSON, such as a predicate, goes through [`map_json`].

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

    let mut replacements: Vec<(usize, usize)> = Vec::new();
    collect_all(command, 0, &mut replacements);

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

/// Collects the spans of every `temp` objective in `command`, moved by
/// `offset`.
fn collect_all(command: &str, offset: usize, out: &mut Vec<(usize, usize)>) {
    if !command.contains(SOURCE_OBJECTIVE) {
        return;
    }
    let tokens = tokenize(command);
    let mut found = Vec::new();
    collect_command(command, &tokens, 0, &mut found);
    for &(start, end) in &tokens {
        let token = &command[start..end];
        if token.contains(['[', '{']) {
            collect_scores_filters(token, start, &mut found);
            collect_snbt_objectives(token, start, &mut found);
        }
    }
    out.extend(
        found
            .into_iter()
            .map(|(start, end)| (start + offset, end + offset)),
    );
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
                    // A placeholder before `score` stands for `if`,
                    // `unless`, or `store result|success`, and each has the
                    // objective third.
                    _ if (matches!(token, "if" | "unless") || placeholder(index))
                        && word(index + 1) == Some("score") =>
                    {
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
            // A placeholder for `players` or `objectives`: the subcommand
            // after it tells where the objective is, when every word the
            // placeholder can be puts one there.
            (Some(_), Some(sub)) if placeholder(first + 1) => match sub {
                "set" | "get" | "reset" | "enable" => {
                    objective(first + 4, out);
                }
                "operation" => {
                    objective(first + 4, out);
                    objective(first + 7, out);
                }
                "display" => objective(first + 5, out),
                // `players add|remove` name the objective fourth;
                // `objectives add` has a criterion there, never `temp`.
                "add" | "remove" => objective(first + 4, out),
                "modify" => objective(first + 3, out),
                "setdisplay" => objective(first + 4, out),
                _ if placeholder(first + 2) => {
                    objective(first + 4, out);
                    objective(first + 7, out);
                }
                _ => {}
            },
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

/// Rewrites `temp` keys inside `scores={...}` selector filters.
fn collect_scores_filters(token: &str, offset: usize, out: &mut Vec<(usize, usize)>) {
    let bytes = token.as_bytes();
    let opener = "scores={";
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
                b'=' if depth == 1 && !in_value => {
                    let key = &token[entry_start..index];
                    if key.trim() == SOURCE_OBJECTIVE {
                        let key_start = entry_start + (key.len() - key.trim_start().len());
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

/// A value of SNBT or JSON text, with the byte spans the rewrite needs.
enum Node {
    Object(Vec<Entry>),
    List(Vec<Node>),
    /// A string or another scalar: `start..end` is its text without quotes.
    Scalar {
        start: usize,
        end: usize,
    },
}

struct Entry {
    /// The key without quotes, and its span.
    key: String,
    key_start: usize,
    key_end: usize,
    value: Node,
}

/// Reads SNBT or JSON loosely: enough to know which key holds which value.
/// Text it can't read gives `None` and is left alone.
struct NodeParser<'a> {
    text: &'a str,
    pos: usize,
    depth: usize,
}

impl NodeParser<'_> {
    /// Deeper text is left alone rather than read with a deep stack.
    const MAX_DEPTH: usize = 256;

    fn byte(&self) -> Option<u8> {
        self.text.as_bytes().get(self.pos).copied()
    }

    fn skip_space(&mut self) {
        while self.byte().is_some_and(|byte| byte.is_ascii_whitespace()) {
            self.pos += 1;
        }
    }

    fn value(&mut self) -> Option<Node> {
        self.skip_space();
        match self.byte()? {
            b'{' => self.nested(Self::object),
            b'[' => self.nested(Self::list),
            b'"' | b'\'' => {
                let (start, end) = self.quoted()?;
                Some(Node::Scalar { start, end })
            }
            _ => {
                let start = self.pos;
                while self.byte().is_some_and(|byte| {
                    !matches!(byte, b',' | b'}' | b']') && !byte.is_ascii_whitespace()
                }) {
                    self.pos += 1;
                }
                (self.pos > start).then_some(Node::Scalar {
                    start,
                    end: self.pos,
                })
            }
        }
    }

    fn nested(&mut self, read: fn(&mut Self) -> Option<Node>) -> Option<Node> {
        if self.depth >= Self::MAX_DEPTH {
            return None;
        }
        self.depth += 1;
        let node = read(self);
        self.depth -= 1;
        node
    }

    /// The quoted string at `pos`, as the span inside its quotes.
    fn quoted(&mut self) -> Option<(usize, usize)> {
        let bytes = self.text.as_bytes();
        let quote = self.byte()?;
        let start = self.pos + 1;
        let mut index = start;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                byte if byte == quote => {
                    self.pos = index + 1;
                    return Some((start, index));
                }
                _ => index += 1,
            }
        }
        None
    }

    fn object(&mut self) -> Option<Node> {
        self.pos += 1;
        let mut entries = Vec::new();
        loop {
            self.skip_space();
            let (key_start, key_end) = match self.byte()? {
                b'}' => {
                    self.pos += 1;
                    return Some(Node::Object(entries));
                }
                b'"' | b'\'' => self.quoted()?,
                _ => {
                    let start = self.pos;
                    while self.byte().is_some_and(|byte| {
                        !matches!(byte, b':' | b',' | b'{' | b'}' | b'[' | b']' | b'"' | b'\'')
                            && !byte.is_ascii_whitespace()
                    }) {
                        self.pos += 1;
                    }
                    if self.pos == start {
                        return None;
                    }
                    (start, self.pos)
                }
            };
            self.skip_space();
            if self.byte()? != b':' {
                return None;
            }
            self.pos += 1;
            let value = self.value()?;
            entries.push(Entry {
                key: self.text[key_start..key_end].to_string(),
                key_start,
                key_end,
                value,
            });
            self.skip_space();
            match self.byte()? {
                b',' => self.pos += 1,
                b'}' => {}
                _ => return None,
            }
        }
    }

    fn list(&mut self) -> Option<Node> {
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_space();
        // A typed array: `[I; 1, 2]`.
        if ["B;", "I;", "L;"]
            .iter()
            .any(|prefix| self.text[self.pos..].starts_with(prefix))
        {
            self.pos += 2;
        }
        loop {
            self.skip_space();
            if self.byte()? == b']' {
                self.pos += 1;
                return Some(Node::List(items));
            }
            items.push(self.value()?);
            self.skip_space();
            match self.byte()? {
                b',' => self.pos += 1,
                b']' => {}
                _ => return None,
            }
        }
    }
}

/// Rewrites `temp` where SNBT or JSON in `token` names an objective: the
/// keys of an `entity_scores` predicate's `scores`, the `score` of a score
/// number provider, the `objective` of a score text component, and the
/// command of a `run_command` click event. Other data, such as a
/// `custom_data` map with a `scores` key, stays as it is.
fn collect_snbt_objectives(token: &str, offset: usize, out: &mut Vec<(usize, usize)>) {
    let bytes = token.as_bytes();
    let mut quote: Option<u8> = None;
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if byte == b'\\' {
                index += 1;
            } else if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'"' | b'\'' => quote = Some(byte),
            b'{' => {
                let mut parser = NodeParser {
                    text: token,
                    pos: index,
                    depth: 0,
                };
                if let Some(node) = parser.value() {
                    walk_node(token, &node, None, offset, out);
                    index = parser.pos;
                    continue;
                }
            }
            _ => {}
        }
        index += 1;
    }
}

fn scalar_text<'a>(text: &'a str, node: &Node) -> Option<&'a str> {
    match node {
        Node::Scalar { start, end } => Some(&text[*start..*end]),
        _ => None,
    }
}

/// Whether `kind`, as a `type`, `condition`, or `action` value, is `name`.
fn is_kind(kind: &str, name: &str) -> bool {
    kind.strip_prefix("minecraft:").unwrap_or(kind) == name
}

fn walk_node(
    text: &str,
    node: &Node,
    parent_key: Option<&str>,
    offset: usize,
    out: &mut Vec<(usize, usize)>,
) {
    match node {
        Node::Object(entries) => {
            let field = |name: &str| {
                entries
                    .iter()
                    .find(|entry| entry.key == name)
                    .and_then(|entry| scalar_text(text, &entry.value))
            };
            let is_type = |name: &str| {
                ["type", "condition", "action"]
                    .into_iter()
                    .any(|key| field(key).is_some_and(|kind| is_kind(kind, name)))
            };
            let has = |name: &str| entries.iter().any(|entry| entry.key == name);
            let entity_scores = is_type("entity_scores");
            let score_provider = is_type("score") || has("target");
            // A score text component: `{score:{name:...,objective:...}}`.
            let score_component = parent_key == Some("score") && has("name");
            let run_command = is_type("run_command");
            for entry in entries {
                let value = scalar_text(text, &entry.value);
                match (entry.key.as_str(), &entry.value) {
                    ("scores", Node::Object(scores)) if entity_scores => {
                        for score in scores {
                            if score.key == SOURCE_OBJECTIVE {
                                out.push((offset + score.key_start, offset + score.key_end));
                            }
                        }
                    }
                    ("score", Node::Scalar { start, end })
                        if score_provider && value == Some(SOURCE_OBJECTIVE) =>
                    {
                        out.push((offset + start, offset + end));
                    }
                    ("objective", Node::Scalar { start, end })
                        if score_component && value == Some(SOURCE_OBJECTIVE) =>
                    {
                        out.push((offset + start, offset + end));
                    }
                    ("command", Node::Scalar { start, .. }) if run_command => {
                        let command = value.unwrap_or_default();
                        let slash = command.len() - command.trim_start_matches('/').len();
                        collect_all(&command[slash..], offset + start + slash, out);
                    }
                    _ => {}
                }
                walk_node(text, &entry.value, Some(&entry.key), offset, out);
            }
        }
        Node::List(items) => {
            for item in items {
                walk_node(text, item, None, offset, out);
            }
        }
        Node::Scalar { .. } => {}
    }
}

/// Rewrites `temp` in resource JSON as [`map_command`] does in commands:
/// the `objective` of a score text component, the `score` of a score number
/// provider, the keys of an `entity_scores` predicate's `scores`, and the
/// command of a `run_command` action.
pub fn map_json(value: &mut serde_json::Value, target: &str) {
    map_json_in(value, None, target);
}

fn map_json_in(value: &mut serde_json::Value, parent_key: Option<&str>, target: &str) {
    match value {
        serde_json::Value::Object(object) => {
            let is_type = |name: &str| {
                ["type", "condition", "action"].into_iter().any(|key| {
                    object
                        .get(key)
                        .and_then(|kind| kind.as_str())
                        .is_some_and(|kind| is_kind(kind, name))
                })
            };
            let entity_scores = is_type("entity_scores");
            let score_provider = is_type("score") || object.contains_key("target");
            let run_command = is_type("run_command");
            let score_component = parent_key == Some("score") && object.contains_key("name");
            for (key, field) in object.iter_mut() {
                match (key.as_str(), &mut *field) {
                    ("objective", serde_json::Value::String(name))
                        if score_component && name == SOURCE_OBJECTIVE =>
                    {
                        *name = target.to_string();
                    }
                    ("score", serde_json::Value::String(name))
                        if score_provider && name == SOURCE_OBJECTIVE =>
                    {
                        *name = target.to_string();
                    }
                    ("scores", serde_json::Value::Object(scores))
                        if entity_scores && scores.contains_key(SOURCE_OBJECTIVE) =>
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
                    ("command", serde_json::Value::String(command)) if run_command => {
                        let slash = command.len() - command.trim_start_matches('/').len();
                        if let Cow::Owned(mapped) = map_command(&command[slash..], target) {
                            *command = format!("{}{mapped}", &command[..slash]);
                        }
                    }
                    _ => {}
                }
                map_json_in(field, Some(key), target);
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                map_json_in(item, None, target);
            }
        }
        _ => {}
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
    fn leaves_user_data_named_like_objectives_alone() {
        for command in [
            "data modify storage ns:a q set value {scores:{temp:36,wind:5}}",
            "give @s stone[custom_data={scores:{temp:1}}]",
            r#"give @s stone[custom_data={score:"temp"}]"#,
            r#"give @s stone[custom_data={objective:"temp",score:{objective:temp}}]"#,
            r#"data merge storage ns:a {"scores":{"temp":1},"score":"temp"}"#,
            r#"tellraw @a {"text":"x","click_event":{"action":"suggest_command","command":"/trigger temp"}}"#,
        ] {
            assert_eq!(map(command), command);
        }
        let mut json = serde_json::json!({
            "function": "minecraft:set_custom_data",
            "tag": {"scores": {"temp": 1}, "score": "temp", "objective": "temp"}
        });
        let before = json.clone();
        super::map_json(&mut json, "pack.vars");
        assert_eq!(json, before);
    }

    #[test]
    fn maps_number_providers_and_click_commands() {
        assert_eq!(
            map(
                r#"item modify entity @s weapon {function:"set_count",count:{type:"minecraft:score",target:"this",score:"temp"}}"#
            ),
            r#"item modify entity @s weapon {function:"set_count",count:{type:"minecraft:score",target:"this",score:"pack.vars"}}"#
        );
        assert_eq!(
            map(
                r#"tellraw @a {"text":"x","click_event":{"action":"run_command","command":"/trigger temp set 1"}}"#
            ),
            r#"tellraw @a {"text":"x","click_event":{"action":"run_command","command":"/trigger pack.vars set 1"}}"#
        );
        let mut json = serde_json::json!({
            "type": "minecraft:run_command", "command": "scoreboard players add @s temp 1"
        });
        super::map_json(&mut json, "pack.vars");
        assert_eq!(json["command"], "scoreboard players add @s pack.vars 1");
    }

    #[test]
    fn maps_the_objective_after_a_placeholder_for_a_subcommand() {
        assert_eq!(
            map("$execute $(m) score x temp matches 7 run say a"),
            "$execute $(m) score x pack.vars matches 7 run say a"
        );
        assert_eq!(
            map("$execute as @a $(m) score x temp matches 4 run say a"),
            "$execute as @a $(m) score x pack.vars matches 4 run say a"
        );
        assert_eq!(
            map("$execute if score x temp matches 4 $(m) score x temp matches 4 run say a"),
            "$execute if score x pack.vars matches 4 $(m) score x pack.vars matches 4 run say a"
        );
        assert_eq!(
            map("$execute store result storage ns:x temp int 1 $(m) score x temp matches 1 run say a"),
            "$execute store result storage ns:x temp int 1 $(m) score x pack.vars matches 1 run say a"
        );
        assert_eq!(
            map("$scoreboard $(group) $(op) coins temp 1"),
            "$scoreboard $(group) $(op) coins pack.vars 1"
        );
        assert_eq!(
            map("$scoreboard $(sub) get x temp"),
            "$scoreboard $(sub) get x pack.vars"
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
