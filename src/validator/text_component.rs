//! Checks for text components written as JSON. Minecraft parses a component
//! when it loads a function, so one it rejects stops the whole function from
//! loading, and every function that calls it. Each rule was confirmed against
//! a 26.3 server. Values with a `$(name)` placeholder are known only when the
//! function runs and are skipped.

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

/// What is wrong with the text component `source`. Text that isn't JSON,
/// such as SNBT or a plain word, gets no check.
pub fn problem(source: &str) -> Option<String> {
    let value: Value = serde_json::from_str(source).ok()?;
    check_component(&value)
}

fn check_component(value: &Value) -> Option<String> {
    match value {
        Value::Array(items) if items.is_empty() => {
            Some("an empty list is not a text component".to_string())
        }
        Value::Array(items) => items.iter().find_map(check_component),
        Value::Object(fields) => check_object(fields),
        _ => None,
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
    if !CONTENT_KEYS.iter().any(|key| fields.contains_key(*key)) {
        return Some("a text component needs content, such as `text`".to_string());
    }
    if fields.get("text").is_some_and(|text| !text.is_string()) {
        return Some("`text` must be a string".to_string());
    }
    if let Some(problem) = fields.get("color").and_then(check_color) {
        return Some(problem);
    }
    for flag in STYLE_FLAGS {
        if let Some(Value::String(text)) = fields.get(*flag) {
            if !text.contains("$(") {
                return Some(format!("`{flag}` must be true or false, not a string"));
            }
        }
    }
    if let Some(score) = fields.get("score") {
        for key in ["name", "objective"] {
            if score.get(key).is_none() {
                return Some(format!("a `score` component needs `{key}`"));
            }
        }
    }
    if fields.contains_key("nbt") && !NBT_SOURCES.iter().any(|key| fields.contains_key(*key)) {
        return Some("an `nbt` component needs `storage`, `entity`, or `block`".to_string());
    }
    for key in ["selector", "entity"] {
        if let Some(problem) = fields.get(key).and_then(check_selector) {
            return Some(problem);
        }
    }
    if let Some(extra) = fields.get("extra") {
        let Value::Array(items) = extra else {
            return Some("`extra` must be a list".to_string());
        };
        if items.is_empty() {
            return Some("`extra` can't be an empty list".to_string());
        }
        if items.iter().any(Value::is_number) {
            return Some("`extra` items must be text components".to_string());
        }
        if let Some(problem) = items.iter().find_map(check_component) {
            return Some(problem);
        }
    }
    if let Some(Value::Array(items)) = fields.get("with") {
        if let Some(problem) = items.iter().find_map(check_component) {
            return Some(problem);
        }
    }
    if let Some(problem) = fields.get("separator").and_then(check_component) {
        return Some(problem);
    }
    if let Some(event) = fields.get("click_event") {
        if let Some(problem) = check_click_event(event) {
            return Some(problem);
        }
    }
    fields.get("hover_event").and_then(check_hover_event)
}

fn check_color(color: &Value) -> Option<String> {
    let color = color.as_str()?;
    if color.starts_with('#') || color.contains("$(") || NAMED_COLORS.contains(&color) {
        return None;
    }
    Some(format!(
        "`{color}` is not a color: use a lowercase color name or #RRGGBB"
    ))
}

fn check_selector(selector: &Value) -> Option<String> {
    let selector = selector.as_str()?;
    let variable = selector.strip_prefix('@')?;
    if matches!(
        variable.chars().next(),
        Some('a' | 'e' | 'p' | 'r' | 's' | 'n')
    ) && !variable
        .chars()
        .nth(1)
        .is_some_and(|ch| ch.is_alphanumeric())
    {
        return None;
    }
    if selector.contains("$(") {
        return None;
    }
    Some(format!("`{selector}` is not a selector"))
}

/// The action's text, unless a placeholder fills it in.
fn event_action(event: &Value) -> Option<&str> {
    event
        .get("action")
        .and_then(Value::as_str)
        .filter(|action| !action.contains("$("))
}

fn check_click_event(event: &Value) -> Option<String> {
    let action = event_action(event)?;
    let required = match action {
        "open_url" => "url",
        "run_command" | "suggest_command" => "command",
        "change_page" => "page",
        "copy_to_clipboard" => "value",
        "show_dialog" => "dialog",
        "custom" => "id",
        _ => return Some(format!("`{action}` is not a click action")),
    };
    if event.get(required).is_none() {
        return Some(if event.get("value").is_some() {
            format!("`value` of the click action `{action}` is now `{required}`")
        } else {
            format!("the click action `{action}` needs `{required}`")
        });
    }
    match event.get("page") {
        Some(Value::String(page)) if action == "change_page" && !page.contains("$(") => {
            Some("`page` must be a number, not a string".to_string())
        }
        _ => None,
    }
}

fn check_hover_event(event: &Value) -> Option<String> {
    let action = event_action(event)?;
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
    if action == "show_text" {
        return event.get("value").and_then(check_component);
    }
    None
}

#[cfg(test)]
mod tests {
    use super::problem;

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
            "{text:\"snbt is not checked\",color:pink}",
        ] {
            assert_eq!(problem(component), None, "{component}");
        }
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
            (r#"{"text":"x","color":"pink"}"#, "`pink` is not a color"),
            (r#"{"text":"x","color":"RED"}"#, "`RED` is not a color"),
            (
                r#"{"text":"x","bold":"true"}"#,
                "`bold` must be true or false",
            ),
            (r#"{"score":{"name":"x"}}"#, "needs `objective`"),
            (r#"{"score":{"objective":"y"}}"#, "needs `name`"),
            (r#"{"nbt":"a"}"#, "needs `storage`, `entity`, or `block`"),
            (r#"{"selector":"@bad"}"#, "`@bad` is not a selector"),
            (r#"{"nbt":"a","entity":"@bad"}"#, "`@bad` is not a selector"),
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
