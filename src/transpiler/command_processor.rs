use crate::ast::{CobbleType, Expression};
use crate::transpiler::snbt_text;
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{escape_quoted, TranspileError};
use std::collections::HashMap;

/// A `say` command that shows Cobble variables, run as a macro in a helper
/// function. See [`CommandProcessor::say_macro`].
#[derive(Debug, Clone, PartialEq)]
pub struct SayMacro {
    /// The helper's macro line: `$say ...` with `$(name)` for each argument.
    pub command: String,
    /// The values the caller copies into the macro storage, in order.
    pub arguments: Vec<SayMacroArgument>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SayMacroArgument {
    pub name: String,
    pub source: SayMacroSource,
}

#[derive(Debug, Clone, PartialEq)]
pub enum SayMacroSource {
    /// A score on `objective` (`temp` is this pack's variable objective).
    Score { objective: String },
    /// A storage-backed variable at `path` in `<namespace>:global`.
    Storage { path: String },
    /// A parameter of the current function, read from `args`.
    Parameter,
}

/// Storage paths in `<namespace>:global` where a text command finds the
/// text of a value of unknown type, and a list or a map that has none.
const TEXT_COPY: &str = "_text";
const SNBT_COPY: &str = "_text_snbt";

/// A command after placeholder substitution.
struct Substituted {
    text: String,
    /// Cobble variables found, as (start, end, name) char positions in the
    /// original command.
    vars: Vec<(usize, usize, String)>,
    /// Whether the text uses macro arguments (`$(name)`).
    has_macro_vars: bool,
}

/// Names of the `$(name)` macro arguments in `text`, in order, without
/// duplicates.
fn macro_argument_names(text: &str) -> Vec<String> {
    let mut names: Vec<String> = Vec::new();
    let mut rest = text;
    while let Some(start) = rest.find("$(") {
        let after = &rest[start + 2..];
        let Some(end) = after.find(')') else {
            break;
        };
        let name = &after[..end];
        if !name.is_empty()
            && name.chars().all(|ch| ch.is_alphanumeric() || ch == '_')
            && !names.iter().any(|existing| existing == name)
        {
            names.push(name.to_string());
        }
        rest = &after[end + 1..];
    }
    names
}

/// Process Minecraft command strings with variable substitution
pub struct CommandProcessor<'a> {
    pub(in crate::transpiler) namespace: String,
    pub(in crate::transpiler) current_params: &'a [String],
    pub(in crate::transpiler) symbols: &'a Symbols,
}

impl<'a> CommandProcessor<'a> {
    fn is_param(&self, name: &str) -> bool {
        self.current_params.contains(&name.to_string())
    }

    pub fn process_command_string(&self, cmd: &str) -> Result<String, TranspileError> {
        // Special handling for tellraw/title commands with scoreboard variables
        let Substituted {
            text: mut result,
            vars: scoreboard_vars_found,
            has_macro_vars,
        } = self.substitute_placeholders(cmd, false)?;

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

        if let Some(alias) = refined_alias(&result, &self.symbols.selector_aliases) {
            return Err(format!(
                "Selector alias `@{alias}` can't take more arguments in `[...]`. Write the whole selector, or define another alias with them."
            )
            .into());
        }
        let expanded = expand_selector_aliases(&result, &self.symbols.selector_aliases);
        if let Some(selector) = unknown_selector(expanded.trim_start_matches('$')) {
            let name = &selector[1..];
            let hint = if self.symbols.entity_templates.contains_key(name) {
                format!(
                    " `{selector}` is an entity template, which only works with `create`; \
                     add a selector alias `@Name = @e[...]` to target its entities."
                )
            } else {
                " Define it with `@Name = @selector[...]` or use a vanilla selector \
                 (@a, @e, @p, @r, @s, @n)."
                    .to_string()
            };
            return Err(format!("Unknown selector `{selector}`.{hint}").into());
        }
        Ok(expanded)
    }

    /// Plans a `say` command whose text shows Cobble variables. `say` adds
    /// its `[name]` prefix and shows selectors as names only when it runs as
    /// `say`, so every variable becomes a macro argument (`$(name)`) and the
    /// caller copies the values into storage before running the command in a
    /// helper. Returns `None` for other commands and for `say` text without
    /// variables.
    pub fn say_macro(&self, cmd: &str) -> Result<Option<SayMacro>, TranspileError> {
        if !cmd.trim().starts_with("say ") {
            return Ok(None);
        }
        let Substituted { text, vars, .. } = self.substitute_placeholders(cmd, true)?;
        if vars.is_empty() {
            return Ok(None);
        }

        let mut arguments: Vec<SayMacroArgument> = Vec::new();
        for (_, _, name) in vars {
            if arguments.iter().any(|argument| argument.name == name) {
                continue;
            }
            let source = match self.symbols.variable_storage_paths.get(&name) {
                Some(path) => SayMacroSource::Storage { path: path.clone() },
                None => SayMacroSource::Score {
                    objective: self
                        .symbols
                        .variable_objectives
                        .get(&name)
                        .cloned()
                        .unwrap_or_else(|| "temp".to_string()),
                },
            };
            arguments.push(SayMacroArgument { name, source });
        }
        // Parameters already read `$(name)`; the caller copies them too,
        // since the helper only sees the storage it is called with. A
        // hand-written `$(name)` that is not a parameter comes from whoever
        // calls this function with macro arguments, which a copy from `args`
        // would not see, so such text keeps the `tellraw` form.
        for name in macro_argument_names(&text) {
            if arguments.iter().any(|argument| argument.name == name) {
                continue;
            }
            if !self.is_param(&name) {
                return Ok(None);
            }
            arguments.push(SayMacroArgument {
                name,
                source: SayMacroSource::Parameter,
            });
        }

        let text = text.trim();
        let text = text.strip_prefix('$').unwrap_or(text);
        Ok(Some(SayMacro {
            command: format!(
                "${}",
                expand_selector_aliases(text, &self.symbols.selector_aliases)
            ),
            arguments,
        }))
    }

    /// Replaces `{name}` placeholders for parameters (`$(name)`), constants,
    /// and constant variables, and collects the Cobble variables it finds.
    /// With `vars_as_macros`, each variable placeholder also becomes
    /// `$(name)`.
    fn substitute_placeholders(
        &self,
        cmd: &str,
        vars_as_macros: bool,
    ) -> Result<Substituted, TranspileError> {
        // Handle both variable substitution and parameter substitution for Minecraft 1.21.8+ macros
        // Properly handles nested braces by tracking nesting levels
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
                        if self.is_param(var_name)
                            && !vars_as_macros
                            && is_text_command(&result)
                            && in_text_string(&chars[..i])
                        {
                            // Shown from `args` like a variable, because a
                            // macro value with a `"` would break the JSON.
                            scoreboard_vars_found.push((i, j, var_name.to_string()));
                            i = j;
                        } else if self.is_param(var_name) {
                            // Function parameter - convert to macro
                            let replacement = format!("$({})", var_name);
                            replacements.push((i, j, replacement));
                            has_macro_vars = true;
                            i = j; // Skip past this replacement
                        } else if let Some(const_value) =
                            self.symbols.compile_time_constants.get(var_name)
                        {
                            let replacement = self.format_constant(*const_value);
                            replacements.push((i, j, replacement));
                            i = j;
                        } else if self.symbols.variable_storage_paths.contains_key(var_name)
                            || self.symbols.scoreboard_variables.contains(var_name)
                        {
                            // Storage or scoreboard variable found - collect for special handling
                            scoreboard_vars_found.push((i, j, var_name.to_string()));
                            if vars_as_macros {
                                replacements.push((i, j, format!("$({})", var_name)));
                            }
                            i = j; // Skip past this variable
                        } else if let Some(value) = self.symbols.variables.get(var_name) {
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
                                    let escaped = escape_quoted(s);

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

        Ok(Substituted {
            text: result,
            vars: scoreboard_vars_found,
            has_macro_vars,
        })
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
        if self.is_param(var_name) {
            self.storage_text_component(var_name, &format!("args.{var_name}"))
        } else if let Some(storage_path) = self.symbols.variable_storage_paths.get(var_name) {
            self.storage_text_component(var_name, storage_path)
        } else {
            let objective = self
                .symbols
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

    /// The text component that shows a storage-backed variable. An `nbt`
    /// component quotes a string unless it interprets the value, and an
    /// interpreted number or list shows nothing, so a string is interpreted
    /// and a list or a map shows as SNBT. A value of unknown type shows the
    /// copies [`Self::text_copy_commands`] makes before the command.
    fn storage_text_component(&self, var_name: &str, storage_path: &str) -> serde_json::Value {
        let storage = format!("{}:global", self.namespace);
        let known_type = if self.is_param(var_name) {
            None
        } else {
            self.symbols.variable_types.get(var_name)
        };
        match known_type {
            Some(CobbleType::String) => serde_json::json!({
                "nbt": storage_path,
                "storage": storage,
                "interpret": true
            }),
            Some(CobbleType::List | CobbleType::Map) => serde_json::json!({
                "nbt": storage_path,
                "storage": storage
            }),
            _ => serde_json::json!({
                "text": "",
                "extra": [
                    {
                        "nbt": format!("{TEXT_COPY}.{var_name}"),
                        "storage": storage,
                        "interpret": true
                    },
                    {"nbt": format!("{SNBT_COPY}.{var_name}"), "storage": storage}
                ]
            }),
        }
    }

    /// Commands that run before a text command showing a storage-backed
    /// variable of unknown type: a number or a string gets its text in
    /// `_text.<name>`, and a list or a map, which has no text, gets a copy
    /// in `_text_snbt.<name>` (see [`Self::storage_text_component`]).
    pub fn text_copy_commands(&self, cmd: &str) -> Result<Vec<String>, TranspileError> {
        let trimmed = cmd.trim();
        if !["tellraw ", "title ", "say "]
            .iter()
            .any(|command| trimmed.starts_with(command))
        {
            return Ok(Vec::new());
        }
        let Substituted { vars, .. } = self.substitute_placeholders(cmd, false)?;
        let storage = format!("{}:global", self.namespace);
        let mut copied: Vec<&str> = Vec::new();
        let mut commands = Vec::new();
        for (_, _, name) in &vars {
            let path = if self.is_param(name) {
                format!("args.{name}")
            } else {
                let Some(path) = self.symbols.variable_storage_paths.get(name) else {
                    continue;
                };
                if matches!(
                    self.symbols.variable_types.get(name),
                    Some(CobbleType::String | CobbleType::List | CobbleType::Map)
                ) {
                    continue;
                }
                path.clone()
            };
            if copied.contains(&name.as_str()) {
                continue;
            }
            copied.push(name);
            commands.extend([
                format!("data remove storage {storage} {TEXT_COPY}.{name}"),
                format!("data remove storage {storage} {SNBT_COPY}.{name}"),
                format!("data modify storage {storage} {TEXT_COPY}.{name} set string storage {storage} {path}"),
                format!("execute unless data storage {storage} {TEXT_COPY}.{name} run data modify storage {storage} {SNBT_COPY}.{name} set from storage {storage} {path}"),
            ]);
        }
        Ok(commands)
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
            // Fallback for callers that cannot add helper functions: the
            // transpiler runs `say` text with variables through
            // `say_macro` instead, which keeps `say`'s own output.
            let tellraw_cmd = format!("tellraw @a {}", serde_json::Value::from(say_message.trim()));
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
        // Explicit `\n` plus spaces: a `\` line continuation would strip the
        // example's indentation.
        Err(format!(
            "Cannot interpolate scoreboard variable{} {} in '{}' command.\n\
            Only say, tellraw, and title text can show a variable's value.\n\
            To use it in another command, pass it to a function parameter:\n    \
            def show_value(val):\n        \
            /command {{val}}\n    \
            show_value({})",
            if var_names.len() > 1 { "s" } else { "" },
            var_names.join(", "),
            trimmed.split_whitespace().next().unwrap_or("unknown"),
            var_names.first().copied().unwrap_or("your_variable")
        )
        .into())
    }

    fn convert_to_tellraw_json(
        &self,
        cmd: &str,
        vars: &[(usize, usize, String)],
    ) -> Result<String, TranspileError> {
        let (command, selector, action, message) = split_text_command(cmd)?;
        let assemble = |text: &str| match action {
            Some(action) => format!("{} {} {} {}", command, selector, action, text),
            None => format!("{} {} {}", command, selector, text),
        };
        let var_names: Vec<String> = vars.iter().map(|(_, _, name)| name.clone()).collect();
        let snbt_as_json = snbt_text::component_json(message.trim());
        let message = snbt_as_json.as_deref().unwrap_or(message);
        let trimmed = message.trim();

        if trimmed.starts_with('[') {
            if var_names.is_empty() {
                return Ok(assemble(message));
            }
            return Ok(assemble(&self.json_array_with_vars(trimmed, &var_names)?));
        }

        // A JSON object keeps its styling, such as
        // {"text":"Hello {player}","color":"gold","bold":true}.
        let mut message = message;
        if let Some(text) = quoted_message(trimmed) {
            let components = self.plain_text_components(&text, &var_names);
            return Ok(assemble(&format!("[{}]", components.join(","))));
        }
        if trimmed.starts_with('{') {
            if let Ok(json_obj) = serde_json::from_str::<serde_json::Value>(trimmed) {
                let component = self.object_with_vars(&json_obj, &var_names);
                let text = serde_json::to_string(&component).unwrap_or_else(|_| "{}".to_string());
                if Self::text_contains_any_var(&text, &var_names) {
                    return Err(UNREACHED_VARIABLE.into());
                }
                return Ok(assemble(&text));
            }

            // If JSON parsing failed or there is no text field, fall back to
            // the quoted string after `"text":`.
            if let Some(text_start) = message.find("\"text\":") {
                let after_text = message[text_start + 7..].trim_start();
                if let Some(contents) = quoted_text_contents(after_text) {
                    message = contents;
                }
            }
        }

        let components = self.plain_text_components(message, &var_names);
        Ok(assemble(&format!("[{}]", components.join(","))))
    }

    /// A JSON array text component with `{var}` placeholders in its string
    /// items and object `text` fields turned into score or storage
    /// components.
    fn json_array_with_vars(
        &self,
        message: &str,
        var_names: &[String],
    ) -> Result<String, TranspileError> {
        let parsed_array = serde_json::from_str::<serde_json::Value>(message).map_err(|error| {
            format!("Cannot use variables inside malformed JSON array text component: {error}")
        })?;
        let serde_json::Value::Array(items) = parsed_array else {
            return Err("Expected a JSON array text component.".into());
        };

        let converted_components = items
            .iter()
            .flat_map(|item| self.item_with_vars(item, var_names))
            .collect();
        let json_string = serde_json::to_string(&serde_json::Value::Array(converted_components))
            .unwrap_or_else(|_| "[]".to_string());
        if Self::text_contains_any_var(&json_string, var_names) {
            return Err(UNREACHED_VARIABLE.into());
        }
        Ok(json_string)
    }

    /// One item of a component list, which a placeholder can split in parts.
    fn item_with_vars(
        &self,
        item: &serde_json::Value,
        var_names: &[String],
    ) -> Vec<serde_json::Value> {
        match item {
            serde_json::Value::String(text) if Self::text_contains_any_var(text, var_names) => {
                self.split_text_component_with_vars(text, var_names)
            }
            serde_json::Value::Object(_) => vec![self.object_with_vars(item, var_names)],
            _ => vec![item.clone()],
        }
    }

    /// `object` with its placeholders filled in: its `text`, its `extra`
    /// parts, the `with` arguments of a translation, and hover text.
    fn object_with_vars(
        &self,
        object: &serde_json::Value,
        var_names: &[String],
    ) -> serde_json::Value {
        let mut result = match object.get("text").and_then(|value| value.as_str()) {
            Some(text) if Self::text_contains_any_var(text, var_names) => {
                self.styled_component_with_vars(object, text, var_names)
            }
            _ => object.clone(),
        };
        if let Some(serde_json::Value::Array(extra)) = result.get("extra").cloned() {
            result["extra"] = extra
                .iter()
                .flat_map(|item| self.item_with_vars(item, var_names))
                .collect();
        }
        if let Some(serde_json::Value::Array(arguments)) = result.get("with").cloned() {
            // One placeholder of the translation takes one argument.
            result["with"] = arguments
                .iter()
                .map(|item| {
                    let mut parts = self.item_with_vars(item, var_names);
                    match parts.len() {
                        1 => parts.remove(0),
                        _ => serde_json::Value::Array(parts),
                    }
                })
                .collect();
        }
        let hover_text = result
            .get("hover_event")
            .filter(|hover| {
                hover.get("action").and_then(|value| value.as_str()) == Some("show_text")
            })
            .and_then(|hover| hover.get("value"))
            .cloned();
        if let Some(value) = hover_text {
            let parts: Vec<_> = match &value {
                serde_json::Value::Array(items) => items
                    .iter()
                    .flat_map(|item| self.item_with_vars(item, var_names))
                    .collect(),
                value => self.item_with_vars(value, var_names),
            };
            result["hover_event"]["value"] = serde_json::Value::Array(parts);
        }
        result
    }

    /// Plain text with `{var}` placeholders as text components: each
    /// variable becomes a score component, or an `nbt` component for a
    /// storage-backed variable.
    fn plain_text_components(&self, message: &str, var_names: &[String]) -> Vec<String> {
        let mut json_components = Vec::new();
        let mut remaining = message;
        while !remaining.is_empty() {
            let next_var = var_names
                .iter()
                .filter_map(|name| {
                    remaining
                        .find(&format!("{{{}}}", name))
                        .map(|pos| (pos, name))
                })
                .min_by_key(|(pos, _)| *pos);
            let Some((pos, var_name)) = next_var else {
                json_components.push(format!("{{\"text\":\"{}\"}}", escape_quoted(remaining)));
                break;
            };

            if pos > 0 {
                json_components.push(format!(
                    "{{\"text\":\"{}\"}}",
                    escape_quoted(&remaining[..pos])
                ));
            }
            if self.is_param(var_name) || self.symbols.variable_storage_paths.contains_key(var_name)
            {
                json_components.push(self.variable_text_component(var_name).to_string());
            } else {
                // The variable's name is its fake player.
                let objective = self
                    .symbols
                    .variable_objectives
                    .get(var_name)
                    .map(|s| s.as_str())
                    .unwrap_or("temp");
                json_components.push(format!(
                    "{{\"score\":{{\"name\":\"{}\",\"objective\":\"{}\"}}}}",
                    var_name, objective
                ));
            }
            remaining = &remaining[pos + var_name.len() + 2..];
        }

        if json_components.is_empty() {
            json_components.push("{\"text\":\"\"}".to_string());
        }
        json_components
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
    // Inside the string value of a `selector` text component, where an alias
    // names entities as it does outside strings.
    let mut in_selector_string = false;
    let mut escape_next = false;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if escape_next {
            escape_next = false;
        } else if ch == '\\' {
            escape_next = true;
        } else if ch == '"' {
            in_string = !in_string;
            in_selector_string = in_string && follows_selector_key(&text[..index]);
        } else if ch == '@' && (!in_string || in_selector_string) {
            let name_end = text[index + 1..]
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .map_or(text.len(), |offset| index + 1 + offset);
            if let Some(selector) = aliases.get(&text[index + 1..name_end]) {
                if in_string {
                    result.push_str(&escape_quoted(selector));
                } else {
                    result.push_str(selector);
                }
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

/// The first alias in `text` outside strings that is followed by `[`, as
/// in `@Players[distance=..5]`. Expanding it would give `@a[...][...]`,
/// which Minecraft can't parse.
fn refined_alias(text: &str, aliases: &HashMap<String, String>) -> Option<String> {
    let mut in_string = false;
    let mut escape_next = false;
    for (index, ch) in text.char_indices() {
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
            let name = &text[index + 1..name_end];
            if aliases.contains_key(name) && text[name_end..].starts_with('[') {
                return Some(name.to_string());
            }
        }
    }
    None
}

/// Whether `before` ends with a `selector` key, as in `{"selector":` or
/// the SNBT `{selector:`.
fn follows_selector_key(before: &str) -> bool {
    let Some(key) = before.trim_end().strip_suffix(':') else {
        return false;
    };
    let key = key.trim_end();
    key.strip_suffix("\"selector\"")
        .or_else(|| key.strip_suffix("selector"))
        .is_some_and(|rest| matches!(rest.trim_end().chars().last(), Some('{' | ',')))
}

const UNREACHED_VARIABLE: &str = "A variable in a text component works in `text`, `extra`, `with`, and hover text. Other fields take no variables.";

/// Commands whose text after the command name is a chat message, where
/// `@Name` is plain text. `msg`, `tell`, and `w` take a target first.
const MESSAGE_COMMANDS: &[&str] = &["say", "me", "teammsg", "tm"];
const PRIVATE_MESSAGE_COMMANDS: &[&str] = &["msg", "tell", "w"];

/// The first `@Name` in `command` outside strings and chat messages that
/// is not a vanilla selector (`@a`, `@e`, `@p`, `@r`, `@s`, `@n`).
/// Minecraft can't parse such a command, so the whole function would fail
/// to load.
pub(in crate::transpiler) fn unknown_selector(command: &str) -> Option<String> {
    let mut quote: Option<char> = None;
    let mut escape_next = false;
    let mut word_start = 0;
    let mut previous_word = "";
    let mut skip_words = 0usize;
    let chars: Vec<(usize, char)> = command.char_indices().collect();
    let mut position = 0;
    while position < chars.len() {
        let (index, ch) = chars[position];
        position += 1;
        if escape_next {
            escape_next = false;
            continue;
        }
        if let Some(open) = quote {
            if ch == '\\' {
                escape_next = true;
            } else if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            ' ' => {
                let word = &command[word_start..index];
                if !word.is_empty() {
                    let starts_command = word_start == 0 || previous_word == "run";
                    if starts_command && MESSAGE_COMMANDS.contains(&word) {
                        return None;
                    }
                    if starts_command && PRIVATE_MESSAGE_COMMANDS.contains(&word) {
                        // The target is checked; the message after it isn't.
                        skip_words = 1;
                    } else if skip_words > 0 {
                        skip_words -= 1;
                        if skip_words == 0 {
                            return None;
                        }
                    }
                    previous_word = word;
                }
                word_start = index + 1;
            }
            '@' if index == word_start => {
                let name: String = chars[position..]
                    .iter()
                    .map(|(_, c)| *c)
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !matches!(name.as_str(), "a" | "e" | "p" | "r" | "s" | "n") {
                    return Some(format!("@{name}"));
                }
            }
            _ => {}
        }
    }
    None
}

/// Whether the position right after `chars` falls inside an unterminated
/// double-quoted string (backslash-escaped quotes do not count).
///
/// Operates on a char slice so callers holding a char index never have to
/// byte-slice the underlying `String` (which panics mid-character on
/// non-ASCII text).
/// Whether `command` is a `tellraw` or `title` command, whose JSON text can
/// show values as text components.
fn is_text_command(command: &str) -> bool {
    let command = command.trim_start();
    command.starts_with("tellraw ") || command.starts_with("title ")
}

/// Whether the text right after `chars` is inside a JSON string that is
/// shown as text: a `text` value or a string item of an array. Other strings,
/// such as a `selector`, a `color`, or a click event's `value`, take the
/// value as it is.
fn in_text_string(chars: &[char]) -> bool {
    let mut open: Option<usize> = None;
    let mut escaped = false;
    for (index, &ch) in chars.iter().enumerate() {
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == '"' {
            open = match open {
                Some(_) => None,
                None => Some(index),
            };
        }
    }
    let Some(open) = open else {
        return false;
    };
    let before: String = chars[..open].iter().collect();
    let before = before.trim();
    if before.ends_with('[') || before.ends_with(',') {
        return true;
    }
    // The whole message: `tellraw @a "text"`, `title @a title "text"`.
    let words = before.split(' ').count();
    if before.starts_with("tellraw ") && words == 2 || before.starts_with("title ") && words == 3 {
        return true;
    }
    let Some(key) = before.strip_suffix(':') else {
        return false;
    };
    let key = key.trim_end();
    key.ends_with("\"text\"")
        || key.ends_with("'text'")
        || key.ends_with("text")
            && !key
                .trim_end_matches("text")
                .ends_with(|c: char| c.is_alphanumeric() || c == '_')
}

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

/// The text of a message that is one quoted string, which Minecraft reads as
/// a text component and shows without its quotes.
fn quoted_message(message: &str) -> Option<String> {
    if let Some(text) = message
        .strip_prefix('\'')
        .and_then(|rest| rest.strip_suffix('\''))
    {
        return Some(text.to_string());
    }
    match serde_json::from_str(message) {
        Ok(serde_json::Value::String(text)) => Some(text),
        _ => None,
    }
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

/// Splits `title <selector> <action> <message>` or
/// `tellraw <selector> <message>` into its parts.
fn split_text_command(cmd: &str) -> Result<(&str, &str, Option<&str>, &str), TranspileError> {
    let cmd = cmd.trim();
    let command = cmd.split(' ').next().unwrap_or("");
    if command == "title" {
        let parts: Vec<&str> = cmd.splitn(4, ' ').collect();
        if parts.len() < 4 {
            return Err(
                "Title command requires action (title/subtitle/actionbar). Format: /title <selector> <action> <text>".into()
            );
        }
        Ok((command, parts[1], Some(parts[2]), parts[3]))
    } else {
        let parts: Vec<&str> = cmd.splitn(3, ' ').collect();
        if parts.len() < 3 {
            return Err("Invalid tellraw command format".into());
        }
        Ok((command, parts[1], None, parts[2]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_string_variable_after_non_ascii_text_is_char_safe() {
        // Regression: `&result[..i]` used a char index on a byte String and
        // panicked mid-character when non-ASCII text preceded the variable.
        let mut symbols = Symbols::default();
        symbols
            .variables
            .insert("msg".to_string(), Expression::String("hello".to_string()));
        let processor = CommandProcessor {
            namespace: "test".to_string(),
            current_params: &[],
            symbols: &symbols,
        };

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
