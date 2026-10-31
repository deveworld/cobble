//! SNBT text components as JSON.
//!
//! Minecraft reads `{text:"Score",color:gold}`, and that is how text is
//! usually written since 1.21.5. Placeholders are filled in on the JSON form of
//! a component, so a component with a placeholder is read here first.

use crate::validator::snbt;
use serde_json::Value;

/// The JSON form of `text`, when it is a component in SNBT. JSON itself and
/// anything that doesn't parse give `None`.
pub(in crate::transpiler) fn component_json(text: &str) -> Option<String> {
    if !text.starts_with(['{', '[']) || serde_json::from_str::<Value>(text).is_ok() {
        return None;
    }
    snbt::parse(text).ok().map(|value| value.to_string())
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
