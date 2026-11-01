//! Checks for text components. Minecraft parses a component when it loads a
//! function, so one it rejects stops the whole function from loading, and
//! every function that calls it. Each rule was confirmed against a 26.3
//! server. Values with a `$(name)` placeholder are known only when the
//! function runs and are skipped.

use super::arg_parsers::{is_id, parse_argument};
use super::registries::{self, entity_type_problem, shown_item_problem, unquoted, whole_number};
use super::string_reader::StringReader;
use super::{snbt, values};
use crate::transpiler::resource_format;
use serde_json::{Map, Value};

const NAMED_COLORS: &[&str] = &[
    "black",
    "dark_blue",
    "dark_green",
    "dark_aqua",
    "dark_red",
    "dark_purple",
    "gold",
    "gray",
    "dark_gray",
    "blue",
    "green",
    "aqua",
    "red",
    "light_purple",
    "yellow",
    "white",
];
const STYLE_FLAGS: &[&str] = &[
    "bold",
    "italic",
    "underlined",
    "strikethrough",
    "obfuscated",
];
const CONTENT_KEYS: &[&str] = &[
    "text",
    "translate",
    "score",
    "selector",
    "keybind",
    "nbt",
    "object",
    "sprite",
    "player",
];
const NBT_SOURCES: &[&str] = &["storage", "entity", "block"];
const COMPONENT_TYPES: &[&str] = &[
    "text",
    "translatable",
    "score",
    "selector",
    "keybind",
    "nbt",
    "object",
];
/// Fields of `player` that hold an id.
const PLAYER_TEXTURES: &[&str] = &["texture", "cape", "elytra"];
/// The longest name a player can have.
const MAX_NAME_LENGTH: usize = 16;

/// What is wrong with the text component `source`, in JSON or SNBT.
pub fn problem(source: &str) -> Option<String> {
    match snbt::parse(source) {
        Ok(value) => check_component(&without_placeholders(value)),
        Err(_) if source.contains("$(") => None,
        Err(problem) => Some(problem),
    }
}

/// What is wrong with the style `source` of a scoreboard number format.
pub fn style_problem(source: &str) -> Option<String> {
    let value = without_placeholders(snbt::parse(source).ok()?);
    check_style(value.as_object()?)
}

/// `value` with `null` for each string that holds a `$(name)` placeholder. In
/// these checks `null` stands for a value they can't know.
fn without_placeholders(value: Value) -> Value {
    match value {
        Value::String(text) if text.contains("$(") => Value::Null,
        Value::Array(items) => Value::Array(items.into_iter().map(without_placeholders).collect()),
        Value::Object(fields) => Value::Object(
            fields
                .into_iter()
                .map(|(key, field)| (key, without_placeholders(field)))
                .collect(),
        ),
        other => other,
    }
}

/// What is wrong with a text component already read as JSON.
pub fn value_problem(value: &Value) -> Option<String> {
    check_component(value)
}

/// The ids of the dialogs that the click events of `source` show. Minecraft
/// looks them up when it loads a function.
pub fn dialog_ids(source: &str) -> Vec<String> {
    let mut ids = Vec::new();
    if let Ok(value) = snbt::parse(source) {
        collect_dialog_ids(&value, &mut ids);
    }
    ids
}

fn collect_dialog_ids(value: &Value, ids: &mut Vec<String>) {
    match value {
        Value::Array(items) => items.iter().for_each(|item| collect_dialog_ids(item, ids)),
        Value::Object(fields) => {
            if fields.get("action").and_then(Value::as_str) == Some("show_dialog") {
                ids.extend(
                    fields
                        .get("dialog")
                        .and_then(Value::as_str)
                        .map(String::from),
                );
            }
            fields
                .values()
                .for_each(|field| collect_dialog_ids(field, ids));
        }
        _ => {}
    }
}

fn check_component(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) if items.is_empty() => {
            Some("an empty list is not a text component".to_string())
        }
        Value::Array(items) => items.iter().find_map(check_component),
        Value::Object(fields) => check_object(fields),
        Value::String(_) | Value::Null => None,
        _ => Some("a number or a boolean is not a text component".to_string()),
    }
}

fn check_object(fields: &Map<String, Value>) -> Option<String> {
    for (old, new) in [("clickEvent", "click_event"), ("hoverEvent", "hover_event")] {
        if fields.contains_key(old) {
            return Some(format!(
                "`{old}` is now `{new}`; Minecraft ignores the old name"
            ));
        }
    }
    // A placeholder can be the name of any field.
    if fields.keys().any(|key| key.contains("$(")) {
        return None;
    }
    if !CONTENT_KEYS.iter().any(|key| fields.contains_key(*key)) {
        return Some("a text component needs content, such as `text`".to_string());
    }
    if let Some(problem) = check_style(fields).or_else(|| check_content(fields)) {
        return Some(problem);
    }
    let extra = fields.get("extra").filter(|extra| !extra.is_null())?;
    let Value::Array(items) = extra else {
        return Some("`extra` must be a list".to_string());
    };
    if items.is_empty() {
        return Some("`extra` can't be an empty list".to_string());
    }
    if items.iter().any(Value::is_number) {
        return Some("`extra` items must be text components".to_string());
    }
    items.iter().find_map(check_component)
}

/// The fields of a style, which any component can have.
fn check_style(fields: &Map<String, Value>) -> Option<String> {
    let field = |key: &str| fields.get(key);
    field("color")
        .and_then(check_color)
        .or_else(|| check_flags(fields))
        .or_else(|| field("shadow_color").and_then(check_shadow_color))
        .or_else(|| field("font").and_then(|font| check_id("font", font)))
        .or_else(|| field("insertion").and_then(|text| check_string("insertion", text)))
        .or_else(|| field("click_event").and_then(check_click_event))
        .or_else(|| field("hover_event").and_then(check_hover_event))
}

fn check_string(key: &str, value: &Value) -> Option<String> {
    (!value.is_string() && !value.is_null()).then(|| format!("`{key}` must be a string"))
}

fn check_id(key: &str, value: &Value) -> Option<String> {
    let Some(id) = value.as_str() else {
        return check_string(key, value);
    };
    (!is_id(id)).then(|| format!("`{key}` must be an id, not `{id}`"))
}

/// `key` of `fields`, which a kind of content can't go without.
fn required<'a>(
    fields: &'a Map<String, Value>,
    kind: &str,
    key: &str,
) -> Result<&'a Value, String> {
    fields
        .get(key)
        .ok_or_else(|| format!("a `{kind}` component needs `{key}`"))
}

/// A `type` names the kind of content. Without one, a component takes the first
/// kind that reads, so each kind ignores the fields of the others.
fn check_content(fields: &Map<String, Value>) -> Option<String> {
    if let Some(kind) = fields.get("type") {
        if kind.is_null() {
            return None;
        }
        let Some(kind) = kind.as_str() else {
            return Some("`type` must be a string".to_string());
        };
        if !COMPONENT_TYPES.contains(&kind) {
            return Some(format!("`{kind}` is not a text component type"));
        }
        return check_kind(kind, fields);
    }
    let kinds = COMPONENT_TYPES.iter().filter(|kind| match **kind {
        "translatable" => fields.contains_key("translate"),
        "object" => ["object", "sprite", "player"]
            .iter()
            .any(|key| fields.contains_key(*key)),
        kind => fields.contains_key(kind),
    });
    any_reads(kinds.map(|kind| check_kind(kind, fields)))
}

/// `None` if any one of the alternatives reads, or else the first problem.
fn any_reads(alternatives: impl Iterator<Item = Option<String>>) -> Option<String> {
    let mut first = None;
    for problem in alternatives {
        problem.as_ref()?;
        first = first.or(problem);
    }
    first
}

fn check_kind(kind: &str, fields: &Map<String, Value>) -> Option<String> {
    let string_field = |key: &str| match required(fields, kind, key) {
        Ok(value) => check_string(key, value),
        Err(problem) => Some(problem),
    };
    match kind {
        "text" => string_field("text"),
        "keybind" => string_field("keybind"),
        "translatable" => string_field("translate").or_else(|| check_translation(fields)),
        "score" => check_score(fields),
        "selector" => match required(fields, kind, "selector") {
            Ok(selector) => check_target("selector", selector)
                .or_else(|| fields.get("separator").and_then(check_component)),
            Err(problem) => Some(problem),
        },
        "nbt" => check_nbt(fields),
        _ => check_object_content(fields),
    }
}

fn check_translation(fields: &Map<String, Value>) -> Option<String> {
    let with = fields.get("with").filter(|with| !with.is_null())?;
    let Value::Array(items) = with else {
        return Some("`with` must be a list".to_string());
    };
    // Translation arguments can also be plain numbers and booleans.
    items
        .iter()
        .filter(|item| item.is_object() || item.is_array())
        .find_map(check_component)
}

fn check_score(fields: &Map<String, Value>) -> Option<String> {
    let score = match required(fields, "score", "score") {
        Ok(score) if score.is_null() => return None,
        Ok(score) => score.as_object(),
        Err(problem) => return Some(problem),
    };
    let Some(score) = score else {
        return Some("`score` must be a compound".to_string());
    };
    ["name", "objective"]
        .iter()
        .find_map(|key| match score.get(*key) {
            Some(value) => check_string(key, value),
            None => Some(format!("a `score` component needs `{key}`")),
        })
}

/// Whether all of `text` reads as an argument of `parser_type`.
fn reads_as(parser_type: &str, text: &str) -> bool {
    let mut reader = StringReader::new(text);
    parse_argument(&mut reader, parser_type, None) && !reader.can_read()
}

/// A selector or a name, as `selector` and an `nbt` component's `entity` take.
fn check_target(key: &str, target: &Value) -> Option<String> {
    let Some(target) = target.as_str() else {
        return check_string(key, target);
    };
    let name = unquoted(target);
    let fits = target.starts_with('@')
        || snbt::is_uuid(name)
        || name.encode_utf16().count() <= MAX_NAME_LENGTH;
    if !reads_as("minecraft:entity", target) || !fits {
        return Some(format!("`{target}` is not a selector"));
    }
    values::problem("minecraft:entity", target)
        .or_else(|| registries::problem("minecraft:entity", None, target))
}

fn check_nbt(fields: &Map<String, Value>) -> Option<String> {
    let path = match required(fields, "nbt", "nbt") {
        Ok(path) => path,
        Err(problem) => return Some(problem),
    };
    if let Some(text) = path.as_str() {
        let parser = "minecraft:nbt_path";
        if !text.is_empty() && !(reads_as(parser, text) && values::problem(parser, text).is_none())
        {
            return Some(format!("`{text}` is not a valid NBT path"));
        }
    } else if let Some(problem) = check_string("nbt", path) {
        return Some(problem);
    }
    let check_source = |source: &str| match (source, required(fields, "nbt", source)) {
        (_, Err(problem)) => Some(problem),
        ("storage", Ok(storage)) => check_id("storage", storage),
        ("entity", Ok(entity)) => check_target("entity", entity),
        (_, Ok(block)) => check_block_pos(block),
    };
    match fields.get("source") {
        Some(Value::Null) => None,
        Some(Value::String(source)) if NBT_SOURCES.contains(&source.as_str()) => {
            check_source(source)
        }
        Some(_) => Some("`source` must be `storage`, `entity`, or `block`".to_string()),
        // Without a `source`, any one of them can be the one that reads.
        None => {
            let mut sources = NBT_SOURCES
                .iter()
                .filter(|source| fields.contains_key(**source))
                .peekable();
            if sources.peek().is_none() {
                return Some(
                    "an `nbt` component needs `storage`, `entity`, or `block`".to_string(),
                );
            }
            any_reads(sources.map(|source| check_source(source)))
        }
    }
}

fn check_block_pos(block: &Value) -> Option<String> {
    let Some(block) = block.as_str() else {
        return check_string("block", block);
    };
    let parser = "minecraft:block_pos";
    (!reads_as(parser, block) || values::problem(parser, block).is_some())
        .then(|| format!("`{block}` is not a block position"))
}

/// The `atlas` and `player` kinds of an `object` component. `object` picks one,
/// and without it either field can be the one that reads.
fn check_object_content(fields: &Map<String, Value>) -> Option<String> {
    let problem = match fields.get("object") {
        Some(Value::Null) => None,
        Some(Value::String(object)) if object == "atlas" => check_sprite(fields),
        Some(Value::String(object)) if object == "player" => check_player_field(fields),
        Some(_) => Some("`object` must be `atlas` or `player`".to_string()),
        None if !fields.contains_key("sprite") && !fields.contains_key("player") => {
            Some("an `object` component needs `sprite` or `player`".to_string())
        }
        None => any_reads(
            [
                fields.contains_key("sprite").then(|| check_sprite(fields)),
                fields
                    .contains_key("player")
                    .then(|| check_player_field(fields)),
            ]
            .into_iter()
            .flatten(),
        ),
    };
    problem.or_else(|| fields.get("fallback").and_then(check_component))
}

fn check_sprite(fields: &Map<String, Value>) -> Option<String> {
    match required(fields, "object", "sprite") {
        Ok(sprite) => check_id("sprite", sprite),
        Err(problem) => Some(problem),
    }
    .or_else(|| {
        fields
            .get("atlas")
            .and_then(|atlas| check_id("atlas", atlas))
    })
}

fn check_player_field(fields: &Map<String, Value>) -> Option<String> {
    let player = match required(fields, "object", "player") {
        Ok(player) => player,
        Err(problem) => return Some(problem),
    };
    let problem = match player {
        Value::String(name) => check_player_name(name),
        Value::Object(profile) => check_profile(profile),
        Value::Null => None,
        _ => Some("`player` must be a name or a compound".to_string()),
    };
    problem.or_else(|| match fields.get("hat") {
        Some(hat) if !is_flag(hat) => Some("`hat` must be true or false".to_string()),
        _ => None,
    })
}

fn check_profile(profile: &Map<String, Value>) -> Option<String> {
    let name_problem = profile.get("name").and_then(|name| match name.as_str() {
        Some(name) => check_player_name(name),
        None => check_string("name", name),
    });
    if name_problem.is_some() {
        return name_problem;
    }
    if profile
        .get("id")
        .is_some_and(|id| !is_uuid_list(id) && !id.is_null())
    {
        return Some("`id` of `player` must be a list of four numbers".to_string());
    }
    if let Some(model) = profile.get("model").filter(|model| !model.is_null()) {
        if !matches!(model.as_str(), Some("wide" | "slim")) {
            return Some("`model` of `player` must be `wide` or `slim`".to_string());
        }
    }
    let properties_read = match profile.get("properties") {
        None | Some(Value::Object(_) | Value::Null) => true,
        Some(Value::Array(items)) => items.iter().all(Value::is_object),
        Some(_) => false,
    };
    if !properties_read {
        return Some("`properties` of `player` must be a list or a compound".to_string());
    }
    PLAYER_TEXTURES
        .iter()
        .find_map(|key| profile.get(*key).and_then(|texture| check_id(key, texture)))
}

fn is_uuid_list(value: &Value) -> bool {
    value
        .as_array()
        .is_some_and(|items| items.len() == 4 && items.iter().all(is_flag))
}

fn check_player_name(name: &str) -> Option<String> {
    let valid =
        name.len() <= MAX_NAME_LENGTH && name.bytes().all(|byte| (33..=126).contains(&byte));
    (!valid).then(|| format!("`{name}` is not a player name"))
}

/// A number or a boolean, which is all a flag or an RGBA channel reads.
fn is_flag(value: &Value) -> bool {
    value.is_number() || value.is_boolean() || value.is_null()
}

fn check_flags(fields: &Map<String, Value>) -> Option<String> {
    STYLE_FLAGS.iter().find_map(|flag| match fields.get(*flag) {
        Some(Value::String(_)) => Some(format!("`{flag}` must be true or false, not a string")),
        Some(value) if !is_flag(value) => Some(format!("`{flag}` must be true or false")),
        _ => None,
    })
}

fn check_color(color: &Value) -> Option<String> {
    let Some(color) = color.as_str() else {
        return check_string("color", color);
    };
    let known = NAMED_COLORS.contains(&color)
        || color.strip_prefix('#').is_some_and(|hex| {
            i32::from_str_radix(hex, 16).is_ok_and(|rgb| (0..=0xFF_FFFF).contains(&rgb))
        });
    (!known).then(|| format!("`{color}` is not a color: use a lowercase color name or #RRGGBB"))
}

/// A whole RGBA number, or four channels.
fn check_shadow_color(color: &Value) -> Option<String> {
    let valid = match color {
        Value::Array(channels) => channels.len() == 4 && channels.iter().all(is_flag),
        other => is_flag(other),
    };
    (!valid).then(|| "`shadow_color` must be a number or a list of four numbers".to_string())
}

/// The action's text, if it is known.
fn event_action<'a>(event: &'a Value, kind: &str) -> Result<Option<&'a str>, String> {
    if event.is_null() {
        return Ok(None);
    }
    if !event.is_object() {
        return Err(format!("a {kind} event must be a compound"));
    }
    match event.get("action") {
        None => Err(format!("a {kind} event needs `action`")),
        Some(Value::Null) => Ok(None),
        Some(Value::String(action)) => Ok(Some(action)),
        Some(_) => Err("`action` must be a string".to_string()),
    }
}

fn check_click_event(event: &Value) -> Option<String> {
    let action = match event_action(event, "click") {
        Ok(action) => action?,
        Err(problem) => return Some(problem),
    };
    let required = match action {
        "open_url" => "url",
        "run_command" | "suggest_command" => "command",
        "change_page" => "page",
        "copy_to_clipboard" => "value",
        "show_dialog" => "dialog",
        "custom" => "id",
        _ => return Some(format!("`{action}` is not a click action")),
    };
    let Some(value) = event.get(required) else {
        return Some(if event.get("value").is_some() {
            format!("`value` of the click action `{action}` is now `{required}`")
        } else {
            format!("the click action `{action}` needs `{required}`")
        });
    };
    match action {
        "open_url" => match value.as_str() {
            Some(url) => check_url(url),
            None => check_string("url", value),
        },
        "change_page" => check_page(value),
        "show_dialog" => check_shown_dialog(value),
        "custom" => check_id("id", value),
        _ => check_string(required, value),
    }
}

fn check_page(page: &Value) -> Option<String> {
    match page {
        Value::String(_) => Some("`page` must be a number, not a string".to_string()),
        Value::Null => None,
        page => match whole_number(page) {
            Some(number) if number > 0 => None,
            Some(_) => Some("`page` must be above 0".to_string()),
            None => Some("`page` must be a number".to_string()),
        },
    }
}

fn check_shown_dialog(dialog: &Value) -> Option<String> {
    if dialog.is_object() {
        let mut problems = resource_format::Problems {
            nbt: true,
            ..Default::default()
        };
        resource_format::check_dialog(dialog, &mut problems);
        return problems.errors.into_iter().next();
    }
    check_id("dialog", dialog)
}

/// What stops `url` from reading as an `http` or `https` link. Follows
/// `java.net.URI`, which Minecraft reads it with.
fn check_url(url: &str) -> Option<String> {
    let scheme_end = url
        .find([':', '/', '?', '#'])
        .filter(|end| url[*end..].starts_with(':'));
    let (scheme, rest) = match scheme_end {
        Some(end) => (&url[..end], &url[end + 1..]),
        None => ("", url),
    };
    if !scheme.eq_ignore_ascii_case("http") && !scheme.eq_ignore_ascii_case("https") {
        return Some(format!("`{url}` needs `http://` or `https://`"));
    }
    let is_illegal = |ch: &char| {
        ch.is_control()
            || ch.is_whitespace()
            || matches!(ch, '"' | '<' | '>' | '\\' | '^' | '`' | '{' | '|' | '}')
    };
    if let Some(ch) = rest.chars().find(is_illegal) {
        return Some(format!("`{url}` can't hold `{ch}`"));
    }
    let (body, fragment) = rest.split_once('#').unwrap_or((rest, ""));
    if fragment.contains('#') {
        return Some(format!("`{url}` has more than one `#`"));
    }
    let (authority, path) = match body.strip_prefix("//") {
        Some(after) => after.split_at(after.find(['/', '?']).unwrap_or(after.len())),
        None => ("", body),
    };
    let path = path.split('?').next().unwrap_or(path);
    let host = authority.rsplit('@').next().unwrap_or(authority);
    let is_ipv6 = host.starts_with('[') && host.contains(']');
    // A body without a leading `/` is opaque, and only has to hold something.
    let is_opaque = !body.starts_with('/');
    if rest == "//"
        || (is_opaque && body.is_empty())
        || (!is_opaque && path.contains(['[', ']']))
        || (!is_ipv6 && host.contains(['[', ']']))
    {
        return Some(format!("`{url}` is not a valid link"));
    }
    let is_escape = |part: &str| {
        part.get(..2)
            .is_some_and(|digits| digits.bytes().all(|byte| byte.is_ascii_hexdigit()))
    };
    // An IPv6 host can hold a `%` of its own.
    (!is_ipv6 && !rest.split('%').skip(1).all(is_escape))
        .then(|| format!("`{url}` has a `%` without two hex digits after it"))
}

fn check_uuid(uuid: &Value) -> Option<String> {
    let valid = match uuid {
        Value::String(uuid) => snbt::is_uuid(uuid),
        Value::Null => true,
        list => is_uuid_list(list),
    };
    (!valid).then(|| format!("`{uuid}` is not a UUID"))
}

fn check_hover_event(event: &Value) -> Option<String> {
    let action = match event_action(event, "hover") {
        Ok(action) => action?,
        Err(problem) => return Some(problem),
    };
    let required: &[&str] = match action {
        "show_text" => &["value"],
        "show_item" => &["id"],
        "show_entity" => &["id", "uuid"],
        _ => return Some(format!("`{action}` is not a hover action")),
    };
    for key in required {
        if event.get(*key).is_none() {
            return Some(if event.get("contents").is_some() {
                format!("`contents` of the hover action `{action}` is now `{key}`")
            } else {
                format!("the hover action `{action}` needs `{key}`")
            });
        }
    }
    let id_problem = |look_up: fn(&str) -> Option<String>| {
        let id = event.get("id")?;
        check_id("id", id).or_else(|| id.as_str().and_then(look_up))
    };
    match action {
        "show_text" => event.get("value").and_then(check_component),
        "show_item" => id_problem(shown_item_problem)
            .or_else(|| event.get("count").and_then(check_item_count))
            .or_else(|| match event.get("components") {
                Some(components) if !components.is_object() && !components.is_null() => {
                    Some("`components` must be a compound".to_string())
                }
                _ => None,
            }),
        _ => id_problem(entity_type_problem)
            .or_else(|| event.get("uuid").and_then(check_uuid))
            .or_else(|| event.get("name").and_then(check_component)),
    }
}

fn check_item_count(count: &Value) -> Option<String> {
    let valid = match count {
        Value::Null => true,
        count => whole_number(count).is_some_and(|count| (1..=99).contains(&count)),
    };
    (!valid).then(|| "`count` must be a number from 1 to 99".to_string())
}

#[cfg(test)]
mod tests {
    use super::{dialog_ids, problem, style_problem};

    #[test]
    fn components_minecraft_accepts_pass() {
        for component in [
            r#"{"text":"x"}"#,
            r#""plain""#,
            r#"["a",{"text":"b","bold":true}]"#,
            r#"{"text":"x","bold":1}"#,
            r##"{"text":"x","color":"#12345"}"##,
            r#"{"text":"x","color":"$(color)"}"#,
            r#"{"translate":"x","with":[]}"#,
            r#"{"translate":"x","with":[5]}"#,
            r#"{"text":"x","extra":["y"]}"#,
            r#"{"selector":"@a[tag=x]","separator":", "}"#,
            r#"{"selector":"$(who)"}"#,
            r#"{"keybind":"key.jump"}"#,
            r#"{"sprite":"minecraft:block/stone"}"#,
            r#"{"player":{"name":"Steve"}}"#,
            r#"{"nbt":"a","storage":"x:y","interpret":true}"#,
            r#"{"score":{"name":"x","objective":"y"}}"#,
            r#"{"text":"x","wibble":1}"#,
            r#"{"text":"x","click_event":{"action":"run_command","command":"/say hi"}}"#,
            r#"{"text":"x","click_event":{"action":"open_url","url":"https://a.b"}}"#,
            r#"{"text":"x","click_event":{"action":"change_page","page":2}}"#,
            r#"{"text":"x","click_event":{"action":"copy_to_clipboard","value":"a"}}"#,
            r#"{"text":"x","click_event":{"action":"show_dialog","dialog":"minecraft:server_links"}}"#,
            r#"{"text":"x","hover_event":{"action":"show_text","value":"tip"}}"#,
            r#"{"text":"x","hover_event":{"action":"show_item","id":"minecraft:stone"}}"#,
            r#"{"text":"x","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":"0-0-0-0-0"}}"#,
            "{text:\"snbt\",color:red}",
            // Colors take any hex number up to 0xFFFFFF.
            r##"{text:"a",color:"#fff"}"##,
            r##"{text:"a",color:"#1"}"##,
            r##"{text:"a",color:"#00ffffff"}"##,
            r##"{text:"a",color:"#+1"}"##,
            // Flags take any number, and so do the channels of a shadow.
            r#"{text:"a",bold:1.5}"#,
            r#"{text:"a",shadow_color:-1}"#,
            r#"{text:"a",shadow_color:1b}"#,
            r#"{text:"a",shadow_color:[1,0,0,1]}"#,
            r#"{text:"a",shadow_color:[B;1b,0b,0b,1b]}"#,
            // Ids can be empty, and `font` takes one without a namespace.
            r#"{text:"a",font:"default"}"#,
            r#"{text:"a",font:""}"#,
            r#"{text:"a",insertion:""}"#,
            // Selectors and names.
            r#"{selector:"Steve"}"#,
            r#"{selector:"0-0-0-0-0"}"#,
            r#"{selector:"\"Steve\""}"#,
            r#"{selector:"@a",separator:["x"]}"#,
            // A score takes any text: it is looked up when the text shows.
            r#"{score:{name:"@x",objective:"a b"}}"#,
            r#"{score:{name:"",objective:""}}"#,
            // Sources of an `nbt` component.
            r#"{nbt:"",storage:"a:b"}"#,
            r#"{nbt:"a",storage:""}"#,
            r#"{nbt:"a",block:"~1.5 2 3"}"#,
            r#"{nbt:"a",block:" 1 2"}"#,
            r#"{nbt:"a",block:"^ ^ ^"}"#,
            r#"{nbt:"a",entity:"@e[limit=2]"}"#,
            r#"{nbt:"a",storage:"a:b",separator:1}"#,
            r#"{nbt:"a",storage:"a:b",interpret:"x"}"#,
            // Any source can be the one that reads, unless `source` names one.
            r#"{nbt:"a",storage:"A",entity:"@s"}"#,
            r#"{nbt:"a",storage:"a:b",entity:"@x"}"#,
            r#"{nbt:"a",storage:"a:b",source:"storage"}"#,
            r#"{nbt:"a",storage:"A",block:"1 2 3",source:"block"}"#,
            // `object`, `sprite`, and `player`.
            r#"{object:"atlas",sprite:"a:b"}"#,
            r#"{object:"player",player:"Steve"}"#,
            r#"{object:"player",sprite:"A",player:"Steve"}"#,
            r#"{sprite:"a:b",atlas:""}"#,
            r#"{sprite:"A",player:"Steve"}"#,
            r#"{sprite:"a:b",player:"é"}"#,
            r#"{sprite:"a:b",fallback:{text:"x"}}"#,
            r#"{player:""}"#,
            r#"{player:"a:b"}"#,
            r#"{player:"abcdefghijklmnop"}"#,
            r#"{player:{}}"#,
            r#"{player:{id:[I;1,2,3,4]}}"#,
            r#"{player:{id:[1.5,2,3,4]}}"#,
            r#"{player:{name:"a",texture:"a:b",model:"slim",properties:[{name:"a",value:"b"}]}}"#,
            r#"{player:{properties:{a:["b"]}}}"#,
            r#"{player:"Steve",hat:1}"#,
            // `type` names the kind, and the other kinds' fields don't count.
            r#"{type:"translatable",text:1,translate:"a"}"#,
            r#"{type:"text",text:"a",selector:"@x"}"#,
            r#"{type:"object",sprite:"a:b"}"#,
            // A link follows `java.net.URI`.
            r#"{text:"a",click_event:{action:"open_url",url:"HTTP://a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https:a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https:?a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https:///a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://#a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://a/b?c=d&e=f#g[h]"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://a/%20é"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://[::1]/a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://[fe80::1%eth0]/a"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"https://a:b/c"}}"#,
            // Whole numbers read from any number or boolean.
            r#"{text:"a",click_event:{action:"change_page",page:1.5}}"#,
            r#"{text:"a",click_event:{action:"change_page",page:true}}"#,
            r#"{text:"a",click_event:{action:"run_command",command:""}}"#,
            r#"{text:"a",click_event:{action:"custom",id:"a"}}"#,
            r#"{text:"a",click_event:{action:"custom",id:"a:b",payload:1}}"#,
            r#"{text:"a",click_event:{action:"show_dialog",dialog:{type:"minecraft:notice",title:"x"}}}"#,
            r#"{text:"a",hover_event:{action:"show_text",value:["x"]}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:99.5}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:true}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"stone",components:{}}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"mymod:thing"}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"mymod:thing",uuid:"0-0-0-0-0"}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"+0-0-0-0-0"}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:[I;1,2,3,4]}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:[1.5,2,3,4]}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"0-0-0-0-0",name:"x"}}"#,
            // A kind of content ignores the fields of the others.
            r#"{text:"a",with:"x"}"#,
            r#"{text:"a",separator:1}"#,
            r#"{text:"a",entity:"@bad"}"#,
            // With two kinds, either one can be the one that reads.
            r#"{text:"a",score:{}}"#,
            r#"{text:"a",selector:"@x"}"#,
            r#"{translate:"a",nbt:1}"#,
            r#"{text:1,translate:"a"}"#,
            r#"{text:"a",object:"nope"}"#,
            r#"{text:1,sprite:"a:b"}"#,
            r#"{text:"x",nbt:"a..b",storage:"a:b"}"#,
        ] {
            assert_eq!(problem(component), None, "{component}");
        }
    }

    #[test]
    fn placeholders_stand_for_any_value() {
        for component in [
            r#"{text:"a",color:"$(c)"}"#,
            r#"{text:"a",color:$(c)}"#,
            r#"{text:"a",font:"a:$(f)"}"#,
            r#"{text:"a",bold:"$(b)"}"#,
            r#"{text:"a",shadow_color:"$(c)"}"#,
            r#"{selector:"@a[limit=$(n)]"}"#,
            r#"{nbt:"$(p)",storage:"$(s)"}"#,
            r#"{nbt:"a",block:"$(x) 1 2"}"#,
            r#"{nbt:"a",entity:"@e[tag=$(t)]"}"#,
            r#"{nbt:"a",storage:"a:b",source:"$(s)"}"#,
            r#"{score:{name:"$(n)",objective:"$(o)"}}"#,
            r#"{score:"$(s)"}"#,
            r#"{object:"$(o)",sprite:"a:b"}"#,
            r#"{sprite:"a:$(s)"}"#,
            r#"{player:"$(p)"}"#,
            r#"{player:{name:"$(p)",id:"$(i)",model:"$(m)",properties:"$(q)"}}"#,
            r#"{player:"Steve",hat:"$(h)"}"#,
            r#"{type:"$(kind)",text:"a"}"#,
            r#"{"$(key)":"a"}"#,
            r#"{text:"a",extra:"$(e)"}"#,
            r#"{translate:"a",with:"$(w)"}"#,
            r#"{text:"a",click_event:"$(e)"}"#,
            r#"{text:"a",click_event:{action:"$(a)",url:"x"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"$(u)"}}"#,
            r#"{text:"a",click_event:{action:"open_url",url:"ftp://$(u)"}}"#,
            r#"{text:"a",click_event:{action:"change_page",page:"$(n)"}}"#,
            r#"{text:"a",click_event:{action:"show_dialog",dialog:"a:$(d)"}}"#,
            r#"{text:"a",click_event:{action:"custom",id:"A$(i)"}}"#,
            r#"{text:"a",hover_event:{action:"$(a)"}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"minecraft:$(i)",count:"$(n)"}}"#,
            r#"{text:"a",hover_event:{action:"show_item",id:"stone",components:"$(c)"}}"#,
            r#"{text:"a",hover_event:{action:"show_entity",id:"$(i)",uuid:"$(u)"}}"#,
        ] {
            assert_eq!(problem(component), None, "{component}");
        }
    }

    #[test]
    fn styles_follow_the_same_rules() {
        for style in [
            "{}",
            r##"{color:"#fff"}"##,
            "{bold:1}",
            r#"{font:"a:b"}"#,
            "{shadow_color:1}",
            r#"{hover_event:{action:"show_text",value:"x"}}"#,
            r#"{click_event:{action:"run_command",command:"x"}}"#,
            r#"{color:"$(c)"}"#,
            // These belong to a component, and a style ignores them.
            "{text:1}",
            "{extra:1}",
            r#"{type:"nope"}"#,
        ] {
            assert_eq!(style_problem(style), None, "{style}");
        }
        for (style, expected) in [
            ("{color:1}", "`color` must be a string"),
            (r##"{color:"#gggggg"}"##, "is not a color"),
            ("{bold:[]}", "`bold` must be true or false"),
            (r#"{font:"A"}"#, "`font` must be an id"),
            ("{insertion:1}", "`insertion` must be a string"),
            (r#"{shadow_color:"x"}"#, "`shadow_color`"),
            ("{hover_event:{}}", "needs `action`"),
            ("{click_event:{}}", "needs `action`"),
            (
                r#"{hover_event:{action:"show_item",id:"nope"}}"#,
                "Unknown item",
            ),
            (r#"{click_event:{action:"open_url",url:"x"}}"#, "`http://`"),
        ] {
            let found = style_problem(style).unwrap_or_default();
            assert!(found.contains(expected), "{style}: {found}");
        }
    }

    #[test]
    fn dialog_ids_are_found_at_any_depth() {
        let component = r#"["a",{text:"b",click_event:{action:"show_dialog",dialog:"a:b"},
            extra:[{text:"c",hover_event:{action:"show_text",value:{text:"d",
            click_event:{action:"show_dialog",dialog:"c"}}}}]}]"#;
        assert_eq!(dialog_ids(component), ["a:b", "c"]);
        assert!(dialog_ids(r#"{text:"a"}"#).is_empty());
        let inline = r#"{text:"a",click_event:{action:"show_dialog",dialog:{type:"notice"}}}"#;
        assert!(dialog_ids(inline).is_empty());
    }

    #[test]
    fn components_too_deep_to_read_go_unchecked() {
        let levels = 600;
        let list = format!("{}\"a\"{}", "[".repeat(levels), "]".repeat(levels));
        assert_eq!(problem(&list), None);
        let extra = format!(
            "{}\"a\"{}",
            "{text:\"a\",extra:[".repeat(levels),
            "]}".repeat(levels)
        );
        assert_eq!(problem(&extra), None);
    }

    #[test]
    fn components_minecraft_rejects_or_ignores_are_reported() {
        for (component, expected) in [
            (
                r#"{"text":"x","clickEvent":{}}"#,
                "`clickEvent` is now `click_event`",
            ),
            (
                r#"{"text":"x","hoverEvent":{}}"#,
                "`hoverEvent` is now `hover_event`",
            ),
            ("[]", "an empty list"),
            (r#"["a",[]]"#, "an empty list"),
            ("{}", "needs content"),
            (r#"{"color":"red"}"#, "needs content"),
            (r#"{"text":5}"#, "`text` must be a string"),
            // Each field has one type, whatever the content is.
            (r#"{text:"a",color:1}"#, "`color` must be a string"),
            (r#"{text:"a",color:true}"#, "`color` must be a string"),
            (r#"{text:"a",color:[]}"#, "`color` must be a string"),
            (r##"{text:"a",color:"#"}"##, "is not a color"),
            (r##"{text:"a",color:"#gggggg"}"##, "is not a color"),
            (r##"{text:"a",color:"#ffffffff"}"##, "is not a color"),
            (r##"{text:"a",color:"#1000000"}"##, "is not a color"),
            (r##"{text:"a",color:"#-1"}"##, "is not a color"),
            (r##"{text:"a",color:"#0x1"}"##, "is not a color"),
            (r#"{text:"a",font:"A"}"#, "`font` must be an id"),
            (r#"{text:"a",font:"a:b:c"}"#, "`font` must be an id"),
            (r#"{text:"a",font:1}"#, "`font` must be a string"),
            (r#"{text:"a",font:{id:"a:b"}}"#, "`font` must be a string"),
            (r#"{text:"a",insertion:1}"#, "`insertion` must be a string"),
            (r#"{translate:1}"#, "`translate` must be a string"),
            (r#"{keybind:1}"#, "`keybind` must be a string"),
            (r#"{text:"a",bold:[]}"#, "`bold` must be true or false"),
            (r#"{text:"a",italic:{}}"#, "`italic` must be true or false"),
            (r##"{text:"a",shadow_color:"#ffffff"}"##, "`shadow_color`"),
            (r#"{text:"a",shadow_color:[1,0,0]}"#, "`shadow_color`"),
            (r#"{text:"a",shadow_color:[1,0,0,1,0]}"#, "`shadow_color`"),
            (r#"{text:"a",shadow_color:["a",0,0,1]}"#, "`shadow_color`"),
            (r#"{text:"a",shadow_color:{}}"#, "`shadow_color`"),
            // A style field counts next to any number of kinds of content.
            (
                r#"{text:"a",translate:"b",font:"A"}"#,
                "`font` must be an id",
            ),
            // Selectors and names read whole.
            (r#"{selector:"@a[tag=a:b]"}"#, "can't hold `:`"),
            (r#"{selector:"@a[limit=0]"}"#, "`limit`"),
            (r#"{selector:"@e[foo=1]"}"#, "not a selector argument"),
            (r#"{selector:"@e[type=zombi]"}"#, "Unknown entity type"),
            (r#"{selector:"Steve Jobs"}"#, "is not a selector"),
            (r#"{selector:""}"#, "is not a selector"),
            (r#"{selector:" @a"}"#, "is not a selector"),
            (r#"{selector:"@a "}"#, "is not a selector"),
            (r#"{selector:"a:b"}"#, "is not a selector"),
            (r#"{selector:"abcdefghijklmnopq"}"#, "is not a selector"),
            (r#"{selector:1}"#, "`selector` must be a string"),
            (r#"{score:1}"#, "`score` must be a compound"),
            (
                r#"{score:{name:1,objective:"b"}}"#,
                "`name` must be a string",
            ),
            (
                r#"{score:{name:"a",objective:1}}"#,
                "`objective` must be a string",
            ),
            // Paths and sources read whole.
            (r#"{nbt:"a..b",storage:"a:b"}"#, "not a valid NBT path"),
            (r#"{nbt:"a[",storage:"a:b"}"#, "not a valid NBT path"),
            (r#"{nbt:" ",storage:"a:b"}"#, "not a valid NBT path"),
            (r#"{nbt:"a ",storage:"a:b"}"#, "not a valid NBT path"),
            (r#"{nbt:1,storage:"a:b"}"#, "`nbt` must be a string"),
            (
                r#"{nbt:"a..b",storage:"a:b",entity:"@s"}"#,
                "not a valid NBT path",
            ),
            (r#"{nbt:"a",storage:"A"}"#, "`storage` must be an id"),
            (r#"{nbt:"a",storage:1}"#, "`storage` must be a string"),
            (r#"{nbt:"a",block:"1.5 2 3"}"#, "not a block position"),
            (r#"{nbt:"a",block:"1 2"}"#, "not a block position"),
            (r#"{nbt:"a",block:"1 2 3 4"}"#, "not a block position"),
            (r#"{nbt:"a",block:"1 2 "}"#, "not a block position"),
            (r#"{nbt:"a",block:"~ ^ ~"}"#, "not a block position"),
            (r#"{nbt:"a",block:""}"#, "not a block position"),
            (r#"{nbt:"a",entity:""}"#, "is not a selector"),
            (r#"{nbt:"a",entity:"Steve Jobs"}"#, "is not a selector"),
            (
                r#"{nbt:"a",storage:"A",block:"x"}"#,
                "`storage` must be an id",
            ),
            (r#"{nbt:"a",storage:"a:b",source:"block"}"#, "needs `block`"),
            (
                r#"{nbt:"a",storage:"a:b",source:"nope"}"#,
                "`source` must be",
            ),
            (r#"{nbt:"a",storage:"a:b",source:1}"#, "`source` must be"),
            (
                r#"{nbt:"a",storage:"A",source:"storage"}"#,
                "`storage` must be an id",
            ),
            // `object`, `sprite`, and `player`.
            (r#"{object:"atlas"}"#, "needs `sprite`"),
            (r#"{object:"player"}"#, "needs `player`"),
            (r#"{object:"nope"}"#, "`object` must be"),
            (r#"{object:1}"#, "`object` must be"),
            (r#"{object:"player",sprite:"a:b"}"#, "needs `player`"),
            (
                r#"{object:"atlas",sprite:"A",player:"Steve"}"#,
                "`sprite` must be an id",
            ),
            (r#"{sprite:"A"}"#, "`sprite` must be an id"),
            (r#"{sprite:1}"#, "`sprite` must be a string"),
            (r#"{sprite:"a:b",fallback:1}"#, "a number or a boolean"),
            (r#"{atlas:"A",sprite:"c"}"#, "`atlas` must be an id"),
            (r#"{player:"abcdefghijklmnopq"}"#, "not a player name"),
            (r#"{player:"a b"}"#, "not a player name"),
            (r#"{player:"é"}"#, "not a player name"),
            (r#"{player:1}"#, "`player` must be"),
            (
                r#"{player:{name:"abcdefghijklmnopq"}}"#,
                "not a player name",
            ),
            (r#"{player:{name:1}}"#, "`name` must be a string"),
            (r#"{player:{id:"0-0-0-0-0"}}"#, "`id` of `player`"),
            (r#"{player:{id:[I;1,2,3]}}"#, "`id` of `player`"),
            (r#"{player:{texture:"A"}}"#, "`texture` must be an id"),
            (r#"{player:{cape:1}}"#, "`cape` must be a string"),
            (r#"{player:{model:"nope"}}"#, "`model` of `player`"),
            (r#"{player:{properties:1}}"#, "`properties` of `player`"),
            (r#"{player:{properties:[1]}}"#, "`properties` of `player`"),
            (r#"{player:"Steve",hat:[]}"#, "`hat` must be true or false"),
            (r#"{player:"Steve",fallback:{}}"#, "needs content"),
            (r#"{atlas:"a:b"}"#, "needs content"),
            // `type` picks the kind.
            (r#"{type:"nbt",text:"a"}"#, "needs `nbt`"),
            (r#"{type:"translatable",text:"a"}"#, "needs `translate`"),
            (r#"{type:"object",text:"a"}"#, "needs `sprite` or `player`"),
            (r#"{type:"text",translate:"a"}"#, "needs `text`"),
            (
                r#"{type:"text",text:1,translate:"a"}"#,
                "`text` must be a string",
            ),
            (r#"{type:1,text:"a"}"#, "`type` must be a string"),
            // Without a `type`, a component needs one kind that reads.
            (r#"{text:1,translate:1}"#, "`text` must be a string"),
            (r#"{text:1,selector:"@x"}"#, "`text` must be a string"),
            (r#"{text:1,nbt:"a"}"#, "`text` must be a string"),
            (r#"{text:1,score:{}}"#, "`text` must be a string"),
            // Events.
            (r#"{text:"a",click_event:{}}"#, "needs `action`"),
            (r#"{text:"a",click_event:1}"#, "must be a compound"),
            (
                r#"{text:"a",click_event:{action:1}}"#,
                "`action` must be a string",
            ),
            (r#"{text:"a",hover_event:{}}"#, "needs `action`"),
            (r#"{text:"a",hover_event:"x"}"#, "must be a compound"),
            (
                r#"{text:"a",hover_event:{action:1}}"#,
                "`action` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:1}}"#,
                "`url` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"example.com"}}"#,
                "`http://`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"//a"}}"#,
                "`http://`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:""}}"#,
                "`http://`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"ftp://a"}}"#,
                "`http://`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"mailto:a@b"}}"#,
                "`http://`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https:"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https:#a"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a/[b"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a[b/c"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://[::1/a"}}"#,
                "not a valid link",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a b"}}"#,
                "can't hold ` `",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a/|b"}}"#,
                "can't hold `|`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a#b#c"}}"#,
                "more than one `#`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a/%2"}}"#,
                "`%`",
            ),
            (
                r#"{text:"a",click_event:{action:"open_url",url:"https://a/%zz"}}"#,
                "`%`",
            ),
            (
                r#"{text:"a",click_event:{action:"change_page",page:0}}"#,
                "above 0",
            ),
            (
                r#"{text:"a",click_event:{action:"change_page",page:-1}}"#,
                "above 0",
            ),
            (
                r#"{text:"a",click_event:{action:"change_page",page:0.5}}"#,
                "above 0",
            ),
            (
                r#"{text:"a",click_event:{action:"change_page",page:false}}"#,
                "above 0",
            ),
            (
                r#"{text:"a",click_event:{action:"change_page",page:[]}}"#,
                "`page` must be a number",
            ),
            (
                r#"{text:"a",click_event:{action:"run_command",command:1}}"#,
                "`command` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"copy_to_clipboard",value:1}}"#,
                "`value` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"custom",id:"Nope"}}"#,
                "`id` must be an id",
            ),
            (
                r#"{text:"a",click_event:{action:"custom",id:1}}"#,
                "`id` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"show_dialog",dialog:"A"}}"#,
                "`dialog` must be an id",
            ),
            (
                r#"{text:"a",click_event:{action:"show_dialog",dialog:1}}"#,
                "`dialog` must be a string",
            ),
            (
                r#"{text:"a",click_event:{action:"show_dialog",dialog:{}}}"#,
                "`type` is missing",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"nope"}}"#,
                "Unknown item `nope`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"minecraft:nope"}}"#,
                "Unknown item",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"Nope"}}"#,
                "`id` must be an id",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:1}}"#,
                "`id` must be a string",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"air"}}"#,
                "can't show",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"minecraft:air"}}"#,
                "can't show",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:0}}"#,
                "`count`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:0.5}}"#,
                "`count`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:100}}"#,
                "`count`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:false}}"#,
                "`count`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",count:"1"}}"#,
                "`count`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_item",id:"stone",components:1}}"#,
                "`components`",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"nope",uuid:"0-0-0-0-0"}}"#,
                "Unknown entity type",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"Nope",uuid:"0-0-0-0-0"}}"#,
                "`id` must be an id",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"x"}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:""}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:1}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:[I;1,2,3]}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:["a",2,3,4]}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"00000000000000000000000000000000"}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"8000000000000000-0-0-0-0"}}"#,
                "not a UUID",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_entity",id:"pig",uuid:"0-0-0-0-0",name:1}}"#,
                "a number or a boolean",
            ),
            (
                r#"{text:"a",hover_event:{action:"show_text",value:{text:"x",hover_event:{}}}}"#,
                "needs `action`",
            ),
            (
                r#"{text:"a",extra:[{text:"b",color:1}]}"#,
                "`color` must be a string",
            ),
            (r#"{"text":"x","color":"pink"}"#, "`pink` is not a color"),
            (r#"{"text":"x","color":"RED"}"#, "`RED` is not a color"),
            ("{text:\"snbt\",color:pink}", "`pink` is not a color"),
            (
                r#"{"text":"x","bold":"true"}"#,
                "`bold` must be true or false",
            ),
            (r#"{"score":{"name":"x"}}"#, "needs `objective`"),
            (r#"{"score":{"objective":"y"}}"#, "needs `name`"),
            (r#"{"nbt":"a"}"#, "needs `storage`, `entity`, or `block`"),
            (r#"{"selector":"@bad"}"#, "`@bad` is not a selector"),
            (r#"{selector:"@e[}"}"#, "`@e[}` is not a selector"),
            (r#"{"nbt":"a","entity":"@bad"}"#, "`@bad` is not a selector"),
            (r#"{translate:"a",with:"x"}"#, "`with` must be a list"),
            (r#"{selector:"@a",separator:1}"#, "a number or a boolean"),
            (
                r#"{"text":"x","extra":[]}"#,
                "`extra` can't be an empty list",
            ),
            (r#"{"text":"x","extra":"y"}"#, "`extra` must be a list"),
            (r#"{"text":"x","extra":[5]}"#, "`extra` items"),
            (
                r#"{"text":"x","click_event":{"action":"run_command","value":"/x"}}"#,
                "`value` of the click action `run_command` is now `command`",
            ),
            (
                r#"{"text":"x","click_event":{"action":"open_url","value":"u"}}"#,
                "is now `url`",
            ),
            (
                r#"{"text":"x","click_event":{"action":"bogus"}}"#,
                "`bogus` is not a click action",
            ),
            (
                r#"{"text":"x","click_event":{"action":"change_page","page":"2"}}"#,
                "`page` must be a number",
            ),
            (
                r#"{"text":"x","click_event":{"action":"custom"}}"#,
                "needs `id`",
            ),
            (
                r#"{"text":"x","hover_event":{"action":"show_text","contents":"t"}}"#,
                "`contents` of the hover action `show_text` is now `value`",
            ),
            (
                r#"{"text":"x","hover_event":{"action":"show_item"}}"#,
                "needs `id`",
            ),
            (
                r#"{"text":"x","hover_event":{"action":"show_entity","id":"minecraft:pig"}}"#,
                "needs `uuid`",
            ),
            (
                r#"{"text":"x","hover_event":{"action":"bogus"}}"#,
                "`bogus` is not a hover action",
            ),
        ] {
            let found = problem(component).unwrap_or_default();
            assert!(found.contains(expected), "{component}: {found}");
        }
    }
}
