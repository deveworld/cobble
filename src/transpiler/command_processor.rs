use crate::ast::Expression;
use crate::transpiler::TranspileError;
use std::collections::{HashMap, HashSet};

/// Process Minecraft command strings with variable substitution
pub struct CommandProcessor<'a> {
    pub namespace: String,
    pub current_params: &'a [String],
    pub scoreboard_variables: &'a HashSet<String>,
    pub variables: &'a HashMap<String, Expression>,
    pub variable_objectives: &'a HashMap<String, String>,
    pub variable_storage_paths: &'a HashMap<String, String>,
    pub selector_aliases: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
}

impl<'a> CommandProcessor<'a> {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        namespace: String,
        current_params: &'a [String],
        scoreboard_variables: &'a HashSet<String>,
        variables: &'a HashMap<String, Expression>,
        variable_objectives: &'a HashMap<String, String>,
        variable_storage_paths: &'a HashMap<String, String>,
        selector_aliases: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
    ) -> Self {
        Self {
            namespace,
            current_params,
            scoreboard_variables,
            variables,
            variable_objectives,
            variable_storage_paths,
            selector_aliases,
            compile_time_constants,
        }
    }

    fn is_param(&self, name: &str) -> bool {
        self.current_params.contains(&name.to_string())
    }

    pub fn process_command_string(&self, cmd: &str) -> Result<String, TranspileError> {
        // Handle both variable substitution and parameter substitution for Minecraft 1.21.8+ macros
        // Properly handles nested braces by tracking nesting levels
        // Special handling for tellraw/title commands with scoreboard variables

        let mut result = cmd.to_string();
        let mut has_macro_vars = false;
        let mut replacements = Vec::new();
        let mut scoreboard_vars_found = Vec::new();

        // First pass: detect if command already contains $() macro syntax
        if result.contains("$(") {
            has_macro_vars = true;
        }

        let chars: Vec<char> = result.chars().collect();
        let mut i = 0;

        while i < chars.len() {
            if chars[i] == '{' {
                // Check for double brace escape sequence {{var}}
                if i + 1 < chars.len() && chars[i + 1] == '{' {
                    // This is an escaped brace sequence {{...}}
                    // Find the matching closing double braces
                    let mut j = i + 2;
                    let mut content = String::new();

                    while j < chars.len() {
                        if j + 1 < chars.len() && chars[j] == '}' && chars[j + 1] == '}' {
                            // Found closing double braces
                            replacements.push((i, j + 2, format!("{{{}}}", content)));
                            i = j + 2;
                            break;
                        }
                        content.push(chars[j]);
                        j += 1;
                    }

                    // If no closing double braces found, treat as regular brace
                    if j >= chars.len() {
                        i += 1;
                    }
                    continue;
                }

                // Regular single brace - find the matching closing brace by tracking nesting level
                let mut depth = 1;
                let mut j = i + 1;
                let mut var_content = String::new();

                while j < chars.len() && depth > 0 {
                    if chars[j] == '{' {
                        depth += 1;
                        var_content.push(chars[j]);
                    } else if chars[j] == '}' {
                        depth -= 1;
                        if depth > 0 {
                            var_content.push(chars[j]);
                        }
                    } else {
                        var_content.push(chars[j]);
                    }
                    j += 1;
                }

                if depth == 0 {
                    // Found matching brace
                    let var_name = var_content.trim();

                    // Check if it's a simple variable (no special characters)
                    let is_simple_var = !var_name.contains(':')
                        && !var_name.contains(',')
                        && !var_name.contains('{');

                    if is_simple_var {
                        // Try to convert simple variables
                        if self.is_param(var_name) {
                            // Function parameter - convert to macro
                            let replacement = format!("$({})", var_name);
                            replacements.push((i, j, replacement));
                            has_macro_vars = true;
                            i = j; // Skip past this replacement
                        } else if let Some(const_value) = self.compile_time_constants.get(var_name)
                        {
                            let replacement = self.format_constant(*const_value);
                            replacements.push((i, j, replacement));
                            i = j;
                        } else if self.variable_storage_paths.contains_key(var_name) {
                            // Storage variable found - collect for special handling
                            scoreboard_vars_found.push((i, j, var_name.to_string()));
                            i = j; // Skip past this variable
                        } else if self.scoreboard_variables.contains(var_name) {
                            // Scoreboard variable found - collect for special handling
                            scoreboard_vars_found.push((i, j, var_name.to_string()));
                            i = j; // Skip past this variable
                        } else if let Some(value) = self.variables.get(var_name) {
                            // Constant variable - inline the value
                            let replacement = match value {
                                Expression::Number(n) => n.to_string(),
                                Expression::String(s) => {
                                    // Check if we're in a command that supports string literals
                                    let cmd_trimmed = result.trim();

                                    // Check if we're in a JSON context (look for "text": pattern)
                                    let is_json_context = cmd_trimmed.contains("\"text\":")
                                        || cmd_trimmed.contains("'text':")
                                        || cmd_trimmed.contains("\"value\":")
                                        || cmd_trimmed.contains("'value':");

                                    let is_safe_for_strings = (cmd_trimmed.starts_with("tellraw ")
                                        || cmd_trimmed.starts_with("title "))
                                        && is_json_context
                                        || cmd_trimmed.starts_with("data ");

                                    if !is_safe_for_strings {
                                        return Err(format!(
                                            "Cannot use string variable '{}' in this command.\n\
                                            Most Minecraft commands don't accept string literals.\n\n\
                                            Solutions:\n\
                                            1. If you want to display text, use the text directly:\n\
                                               /say Hello  (not /say {{message}})\n\
                                            2. For dynamic text with parameters, use tellraw with JSON:\n\
                                               /tellraw @a {{\"text\":\"...\"}}\n\
                                            3. For NBT data operations, use data commands:\n\
                                               /data modify block ~ ~ ~ CustomName set value '{{\"text\":\"...\"}}'",
                                            var_name
                                        ).into());
                                    }

                                    // Check if we're already inside quotes by
                                    // counting unescaped quotes before this
                                    // position. `i` is a char index into
                                    // `chars`, so scan the collected chars
                                    // instead of byte-slicing `result`, which
                                    // would panic mid-character on non-ASCII
                                    // text.
                                    let inside_quotes = is_inside_quotes(&chars[..i]);

                                    // Escape quotes, backslashes, and special characters
                                    let escaped = s
                                        .replace('\\', "\\\\")
                                        .replace('"', "\\\"")
                                        .replace('\n', "\\n")
                                        .replace('\r', "\\r")
                                        .replace('\t', "\\t");

                                    // Only wrap in quotes if we're NOT already inside quotes
                                    if inside_quotes {
                                        escaped
                                    } else {
                                        format!("\"{}\"", escaped)
                                    }
                                }
                                Expression::Boolean(b) => b.to_string(),
                                _ => format!("{{{}}}", var_name),
                            };
                            replacements.push((i, j, replacement));
                            i = j; // Skip past this replacement
                        } else {
                            i += 1; // Not found, continue searching inside
                        }
                    } else {
                        // Complex structure (NBT), continue searching inside
                        i += 1;
                    }
                } else {
                    // No matching brace found, skip this character
                    i += 1;
                }
            } else {
                i += 1;
            }
        }

        // Apply replacements in reverse order to maintain indices
        // Convert char indices to byte indices for UTF-8 safety
        for (char_start, char_end, replacement) in replacements.into_iter().rev() {
            // Convert character indices to byte indices
            let byte_start = result
                .char_indices()
                .nth(char_start)
                .map(|(i, _)| i)
                .unwrap_or(0);
            let byte_end = if char_end < chars.len() {
                result
                    .char_indices()
                    .nth(char_end)
                    .map(|(i, _)| i)
                    .unwrap_or(result.len())
            } else {
                result.len()
            };
            result.replace_range(byte_start..byte_end, &replacement);
        }

        // Handle scoreboard variables - special processing for tellraw/title/say
        if !scoreboard_vars_found.is_empty() {
            result = self.handle_scoreboard_vars_in_command(&result, &scoreboard_vars_found)?;
        }

        if has_macro_vars && scoreboard_vars_found.is_empty() {
            result = self.normalize_macro_text_component_command(&result)?;
        }

        // If the command has macro variables, prefix with $ for Minecraft 1.21.8+ macro system
        if has_macro_vars && !result.starts_with('$') {
            result = format!("${}", result);
        }

        Ok(expand_selector_aliases(&result, self.selector_aliases))
    }

    fn normalize_macro_text_component_command(&self, cmd: &str) -> Result<String, TranspileError> {
        let trimmed = cmd.trim();
        if trimmed.starts_with("title ") {
            let parts: Vec<&str> = trimmed.splitn(4, ' ').collect();
            if parts.len() < 4 {
                return Ok(cmd.to_string());
            }
            let message = parts[3].trim();
            if Self::looks_like_text_component(message) {
                return Ok(cmd.to_string());
            }
            return Ok(format!(
                "{} {} {} {}",
                parts[0],
                parts[1],
                parts[2],
                Self::plain_text_component(message)
            ));
        }

        if trimmed.starts_with("tellraw ") {
            let parts: Vec<&str> = trimmed.splitn(3, ' ').collect();
            if parts.len() < 3 {
                return Ok(cmd.to_string());
            }
            let message = parts[2].trim();
            if Self::looks_like_text_component(message) {
                return Ok(cmd.to_string());
            }
            return Ok(format!(
                "{} {} {}",
                parts[0],
                parts[1],
                Self::plain_text_component(message)
            ));
        }

        Ok(cmd.to_string())
    }

    fn looks_like_text_component(message: &str) -> bool {
        message.starts_with('{') || message.starts_with('[') || message.starts_with('"')
    }

    fn plain_text_component(message: &str) -> String {
        serde_json::json!({ "text": message }).to_string()
    }

    fn variable_text_component(&self, var_name: &str) -> serde_json::Value {
        if let Some(storage_path) = self.variable_storage_paths.get(var_name) {
            serde_json::json!({
                "nbt": storage_path,
                "storage": format!("{}:global", self.namespace)
            })
        } else {
            let objective = self
                .variable_objectives
                .get(var_name)
                .map(|s| s.as_str())
                .unwrap_or("temp");
            serde_json::json!({
                "score": {
                    "name": var_name,
                    "objective": objective
                }
            })
        }
    }

    fn text_contains_any_var(text: &str, var_names: &[String]) -> bool {
        var_names
            .iter()
            .any(|name| text.contains(&format!("{{{}}}", name)))
    }

    fn split_text_component_with_vars(
        &self,
        text: &str,
        var_names: &[String],
    ) -> Vec<serde_json::Value> {
        let mut components = Vec::new();
        let mut remaining = text;

        while !remaining.is_empty() {
            let mut next_var_pos = None;
            let mut next_var_name = String::new();

            for var_name in var_names {
                let pattern = format!("{{{}}}", var_name);
                if let Some(pos) = remaining.find(&pattern) {
                    if next_var_pos.is_none() || pos < next_var_pos.unwrap() {
                        next_var_pos = Some(pos);
                        next_var_name = var_name.clone();
                    }
                }
            }

            if let Some(pos) = next_var_pos {
                if pos > 0 {
                    components.push(serde_json::json!({ "text": &remaining[..pos] }));
                }

                components.push(self.variable_text_component(&next_var_name));

                let pattern = format!("{{{}}}", next_var_name);
                remaining = &remaining[pos + pattern.len()..];
            } else {
                if !remaining.is_empty() {
                    components.push(serde_json::json!({ "text": remaining }));
                }
                break;
            }
        }

        components
    }

    /// Splits a styled object's `text` around `{var}` placeholders into one
    /// component: the text before the first variable keeps the object's
    /// styling and the rest goes into `extra`, so variable values inherit the
    /// same color and formatting.
    fn styled_component_with_vars(
        &self,
        base: &serde_json::Value,
        text: &str,
        var_names: &[String],
    ) -> serde_json::Value {
        let mut parts = self.split_text_component_with_vars(text, var_names);
        let mut component = base.clone();
        let leading_text = match parts.first() {
            Some(serde_json::Value::Object(first))
                if first.len() == 1 && first.get("text").is_some_and(|text| text.is_string()) =>
            {
                parts.remove(0)["text"].clone()
            }
            _ => serde_json::Value::String(String::new()),
        };
        component["text"] = leading_text;
        if let Some(serde_json::Value::Array(existing)) = base.get("extra") {
            parts.extend(existing.iter().cloned());
        }
        component["extra"] = serde_json::Value::Array(parts);
        component
    }

    fn handle_scoreboard_vars_in_command(
        &self,
        cmd: &str,
        vars: &[(usize, usize, String)],
    ) -> Result<String, TranspileError> {
        let trimmed = cmd.trim();

        // Check if it's a tellraw, title, or say command - we can auto-convert these
        if trimmed.starts_with("tellraw ") || trimmed.starts_with("title ") {
            return self.convert_to_tellraw_json(cmd, vars);
        }

        if let Some(say_message) = trimmed.strip_prefix("say ") {
            // Auto-convert /say to /tellraw @a
            let tellraw_cmd = format!("tellraw @a {}", say_message.trim());
            return self.convert_to_tellraw_json(&tellraw_cmd, vars);
        }

        // Check if this command is using macro parameters (starts with $ or contains $(var))
        // Macro parameters are allowed in any command, including say
        let is_macro_command = trimmed.starts_with('$') || {
            // Check if all variables are macro parameters (not scoreboard variables)
            vars.iter()
                .all(|(_, _, name)| self.current_params.contains(name))
        };

        if is_macro_command {
            // This is a macro command - allow it to pass through
            // The variables will be replaced with $(var) syntax
            return Ok(cmd.to_string());
        }

        // For other commands with scoreboard variables, provide helpful error
        let var_names: Vec<_> = vars.iter().map(|(_, _, name)| name.as_str()).collect();
        Err(format!(
            "Cannot interpolate scoreboard variable{} {} in '{}' command.\n\
            Scoreboard variables cannot be displayed in this command.\n\n\
            Note: /say and /tellraw commands are automatically converted to display scores.\n\
            For other commands, use function parameters:\n\
               def show_value(val):\n\
                   /command {{val}}\n\
               show_value(your_variable)",
            if var_names.len() > 1 { "s" } else { "" },
            var_names.join(", "),
            trimmed.split_whitespace().next().unwrap_or("unknown")
        )
        .into())
    }

    fn convert_to_tellraw_json(
        &self,
        cmd: &str,
        vars: &[(usize, usize, String)],
    ) -> Result<String, TranspileError> {
        // Parse the command to extract target selector and message
        // For title commands: "title <selector> <action> <message>"
        // For tellraw commands: "tellraw <selector> <message>"

        // First, split to get command type
        let first_parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
        if first_parts.is_empty() {
            return Err("Empty command".into());
        }

        let command = first_parts[0];

        // Handle title and tellraw differently due to different number of arguments
        let (selector, action, mut message) = if command == "title" {
            // Title: "title <selector> <action> <message>"
            let parts: Vec<&str> = cmd.trim().splitn(4, ' ').collect();
            if parts.len() < 4 {
                return Err(
                    "Title command requires action (title/subtitle/actionbar). Format: /title <selector> <action> <text>".into()
                );
            }
            (parts[1], Some(parts[2]), parts[3])
        } else {
            // Tellraw: "tellraw <selector> <message>"
            let parts: Vec<&str> = cmd.trim().splitn(3, ' ').collect();
            if parts.len() < 3 {
                return Err("Invalid tellraw command format".into());
            }
            (parts[1], None, parts[2])
        };

        // Check if message is a JSON array
        let message_trimmed = message.trim();
        if message_trimmed.starts_with('[') {
            // Message is a JSON array
            if vars.is_empty() {
                if let Some(action_token) = action {
                    return Ok(format!(
                        "{} {} {} {}",
                        command, selector, action_token, message
                    ));
                } else {
                    return Ok(format!("{} {} {}", command, selector, message));
                }
            }

            let parsed_array =
                serde_json::from_str::<serde_json::Value>(message_trimmed).map_err(|error| {
                    format!(
                        "Cannot use variables inside malformed JSON array text component: {error}"
                    )
                })?;
            let serde_json::Value::Array(items) = parsed_array else {
                return Err("Expected a JSON array text component.".into());
            };

            let var_names: Vec<String> = vars.iter().map(|(_, _, name)| name.clone()).collect();
            let mut converted_components = Vec::new();

            for item in items {
                match &item {
                    serde_json::Value::Object(object) => {
                        if let Some(text) = object.get("text").and_then(|value| value.as_str()) {
                            if Self::text_contains_any_var(text, &var_names) {
                                converted_components
                                    .push(self.styled_component_with_vars(&item, text, &var_names));
                            } else {
                                converted_components.push(item);
                            }
                        } else {
                            converted_components.push(item);
                        }
                    }
                    serde_json::Value::String(text) => {
                        if Self::text_contains_any_var(text, &var_names) {
                            converted_components
                                .extend(self.split_text_component_with_vars(text, &var_names));
                        } else {
                            converted_components.push(item);
                        }
                    }
                    _ => converted_components.push(item),
                }
            }

            let json_array = serde_json::Value::Array(converted_components);
            let json_string =
                serde_json::to_string(&json_array).unwrap_or_else(|_| "[]".to_string());

            if var_names
                .iter()
                .any(|name| json_string.contains(&format!("{{{}}}", name)))
            {
                return Err(
                    "Variables in JSON array text components are supported only in string components and object text fields."
                        .into(),
                );
            }

            if let Some(action_token) = action {
                return Ok(format!(
                    "{} {} {} {}",
                    command, selector, action_token, json_string
                ));
            } else {
                return Ok(format!("{} {} {}", command, selector, json_string));
            }
        }

        // If message is a JSON object, we need to handle it specially to preserve styling
        // Example: {"text":"Hello {player}","color":"gold","bold":true}
        if message_trimmed.starts_with('{') {
            // Try to parse as JSON to preserve styling
            if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(message_trimmed) {
                if let Some(text_value) = json_obj.get("text").and_then(|v| v.as_str()) {
                    // Check if text contains variables
                    let has_vars = vars
                        .iter()
                        .any(|(_, _, name)| text_value.contains(&format!("{{{}}}", name)));

                    if has_vars {
                        let var_names: Vec<String> =
                            vars.iter().map(|(_, _, name)| name.clone()).collect();
                        let component =
                            self.styled_component_with_vars(&json_obj, text_value, &var_names);
                        let json_string =
                            serde_json::to_string(&component).unwrap_or_else(|_| "{}".to_string());

                        if let Some(action_token) = action {
                            return Ok(format!(
                                "{} {} {} {}",
                                command, selector, action_token, json_string
                            ));
                        } else {
                            return Ok(format!("{} {} {}", command, selector, json_string));
                        }
                    }
                    // If no variables in text, return as-is
                    if let Some(action_token) = action {
                        return Ok(format!(
                            "{} {} {} {}",
                            command, selector, action_token, message
                        ));
                    } else {
                        return Ok(format!("{} {} {}", command, selector, message));
                    }
                }
            }

            // If JSON parsing failed or no text field, extract text the old way as fallback
            if let Some(text_start) = message.find("\"text\":") {
                let after_text = message[text_start + 7..].trim_start();
                if let Some(contents) = quoted_text_contents(after_text) {
                    message = contents;
                }
            }
        }

        // Build JSON array by replacing {var} with score components (for non-JSON messages)
        let mut json_components = Vec::new();
        let mut remaining = message;

        // Get unique variable names
        let var_names: Vec<String> = vars.iter().map(|(_, _, name)| name.clone()).collect();

        while !remaining.is_empty() {
            // Find the next variable placeholder
            let mut next_var_pos = None;
            let mut next_var_name = String::new();

            for var_name in &var_names {
                let pattern = format!("{{{}}}", var_name);
                if let Some(pos) = remaining.find(&pattern) {
                    if next_var_pos.is_none() || pos < next_var_pos.unwrap() {
                        next_var_pos = Some(pos);
                        next_var_name = var_name.clone();
                    }
                }
            }

            if let Some(pos) = next_var_pos {
                // Add text before variable
                if pos > 0 {
                    let text_before = &remaining[..pos];
                    json_components.push(format!(
                        "{{\"text\":\"{}\"}}",
                        text_before
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"")
                            .replace('\n', "\\n")
                            .replace('\r', "\\r")
                            .replace('\t', "\\t")
                    ));
                }

                // Check if it's a storage variable
                if let Some(storage_path) = self.variable_storage_paths.get(&next_var_name) {
                    json_components.push(format!(
                        "{{\"nbt\":\"{}\",\"storage\":\"{}:global\"}}",
                        storage_path, self.namespace
                    ));
                } else {
                    // Add score component - use variable name as the score holder (fake player)
                    let objective = self
                        .variable_objectives
                        .get(&next_var_name)
                        .map(|s| s.as_str())
                        .unwrap_or("temp");
                    json_components.push(format!(
                        "{{\"score\":{{\"name\":\"{}\",\"objective\":\"{}\"}}}}",
                        next_var_name, objective
                    ));
                }

                // Move past this variable
                let pattern = format!("{{{}}}", next_var_name);
                remaining = &remaining[pos + pattern.len()..];
            } else {
                // No more variables, add remaining text
                if !remaining.is_empty() {
                    json_components.push(format!(
                        "{{\"text\":\"{}\"}}",
                        remaining
                            .replace('\\', "\\\\")
                            .replace('"', "\\\"")
                            .replace('\n', "\\n")
                            .replace('\r', "\\r")
                            .replace('\t', "\\t")
                    ));
                }
                break;
            }
        }

        // Construct final command
        if json_components.is_empty() {
            json_components.push("{\"text\":\"\"}".to_string());
        }

        // Include action token for title commands
        if let Some(action_token) = action {
            Ok(format!(
                "{} {} {} [{}]",
                command,
                selector,
                action_token,
                json_components.join(",")
            ))
        } else {
            Ok(format!(
                "{} {} [{}]",
                command,
                selector,
                json_components.join(",")
            ))
        }
    }

    fn format_constant(&self, value: f64) -> String {
        if (value - value.trunc()).abs() < f64::EPSILON {
            format!("{}", value as i64)
        } else {
            value.to_string()
        }
    }
}

/// Replaces selector aliases (`@Name` -> `@a[...]`) outside of string
/// literals, so JSON text such as `"@Name"` keeps the literal. An alias only
/// matches a whole name: `@Spawn` leaves `@SpawnPoint` alone.
pub(crate) fn expand_selector_aliases(text: &str, aliases: &HashMap<String, String>) -> String {
    if aliases.is_empty() || !text.contains('@') {
        return text.to_string();
    }

    let mut result = String::with_capacity(text.len());
    let mut in_string = false;
    let mut escape_next = false;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if escape_next {
            escape_next = false;
        } else if ch == '\\' {
            escape_next = true;
        } else if ch == '"' {
            in_string = !in_string;
        } else if ch == '@' && !in_string {
            let name_end = text[index + 1..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(text.len(), |offset| index + 1 + offset);
            if let Some(selector) = aliases.get(&text[index + 1..name_end]) {
                result.push_str(selector);
                while chars.peek().is_some_and(|(next, _)| *next < name_end) {
                    chars.next();
                }
                continue;
            }
        }
        result.push(ch);
    }
    result
}

/// Whether the position right after `chars` falls inside an unterminated
/// double-quoted string (backslash-escaped quotes do not count).
///
/// Operates on a char slice so callers holding a char index never have to
/// byte-slice the underlying `String` (which panics mid-character on
/// non-ASCII text).
fn is_inside_quotes(chars: &[char]) -> bool {
    let mut quote_count = 0;
    let mut prev_was_backslash = false;

    for &ch in chars {
        if ch == '"' && !prev_was_backslash {
            quote_count += 1;
        }
        prev_was_backslash = ch == '\\' && !prev_was_backslash;
    }

    quote_count % 2 == 1
}

/// Extract the contents of the double-quoted string that `after_text` starts
/// with (`"abc" rest` -> `abc`), honoring backslash escapes. Returns `None`
/// when `after_text` does not start with a quote or the quote is
/// unterminated. Uses byte offsets from `char_indices`, so multi-byte
/// characters inside the string are sliced safely.
fn quoted_text_contents(after_text: &str) -> Option<&str> {
    if !after_text.starts_with('"') {
        return None;
    }

    let mut prev_backslash = false;
    for (byte_pos, ch) in after_text.char_indices().skip(1) {
        if ch == '"' && !prev_backslash {
            return Some(&after_text[1..byte_pos]);
        }
        prev_backslash = ch == '\\' && !prev_backslash;
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;

    type ProcessorFixture = (
        Vec<String>,
        HashSet<String>,
        HashMap<String, Expression>,
        HashMap<String, String>,
        HashMap<String, String>,
        HashMap<String, String>,
        HashMap<String, f64>,
    );

    fn empty_processor_fixture() -> ProcessorFixture {
        (
            Vec::new(),
            HashSet::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
            HashMap::new(),
        )
    }

    #[test]
    fn inline_string_variable_after_non_ascii_text_is_char_safe() {
        // Regression: `&result[..i]` used a char index on a byte String and
        // panicked mid-character when non-ASCII text preceded the variable.
        let (params, scoreboard, mut variables, objectives, storage, aliases, constants) =
            empty_processor_fixture();
        variables.insert("msg".to_string(), Expression::String("hello".to_string()));
        let processor = CommandProcessor::new(
            "test".to_string(),
            &params,
            &scoreboard,
            &variables,
            &objectives,
            &storage,
            &aliases,
            &constants,
        );

        let result = processor
            .process_command_string("tellraw @a {\"text\":\"안녕하세요 {msg}\"}")
            .unwrap();
        assert!(
            result.contains("안녕하세요 hello"),
            "inlined string should stay inside the quoted text: {result}"
        );
    }

    #[test]
    fn is_inside_quotes_counts_unescaped_quotes() {
        let inside: Vec<char> = "tellraw @a {\"text\":\"안녕 ".chars().collect();
        assert!(is_inside_quotes(&inside));

        let outside: Vec<char> = "tellraw @a {\"text\":\"안녕\" ".chars().collect();
        assert!(!is_inside_quotes(&outside));

        let escaped: Vec<char> = "say \\\" still outside".chars().collect();
        assert!(!is_inside_quotes(&escaped));
    }

    #[test]
    fn quoted_text_contents_is_char_safe() {
        // Regression: the closing-quote position was found as a char index
        // but applied as a byte index, mis-slicing (or panicking on)
        // non-ASCII text.
        assert_eq!(
            quoted_text_contents("\"안녕 world\" trailing"),
            Some("안녕 world")
        );
        assert_eq!(quoted_text_contents("\"a\\\"b\" rest"), Some("a\\\"b"));
        assert_eq!(quoted_text_contents("\"unterminated"), None);
        assert_eq!(quoted_text_contents("no quote"), None);
    }
}
