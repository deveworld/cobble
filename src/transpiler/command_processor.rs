use crate::ast::{CobbleType, Expression};
use crate::transpiler::placeholders::{self, Piece};
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

/// A text command whose hover strings need escaped macro arguments.
pub(in crate::transpiler) struct QuotedHoverMacro {
    pub command: String,
    /// The original parameter and its escaped argument's name.
    pub arguments: Vec<(String, String)>,
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
    /// Names of the Cobble variables found, in order.
    vars: Vec<String>,
    /// Whether the text uses macro arguments (`$(name)`).
    has_macro_vars: bool,
    /// Whether a `tellraw` or `title` message written as plain text needs
    /// a `{"text":...}` wrapper: it holds a parameter, or it starts with an
    /// escaped brace.
    plain_message: bool,
}

/// The error for a placeholder Cobble has no value for.
fn undefined_placeholder(raw: &str, name: &str) -> String {
    format!(
        "Cobble can't fill the placeholder `{raw}` here: `{name}` is not a parameter of this \
         function, a const, the variable of a loop Cobble expands, or a variable Cobble knows at \
         this point, and a variable shows only in `say`, `tellraw`, and `title` text. Write \
         `{{{{{name}}}}}` for literal braces."
    )
}

/// Where a Cobble variable shown in a text command sits in the text until it
/// becomes a text component. Private-use characters, so literal text such
/// as `{name}` written as `{{name}}` is never taken for it.
fn var_marker(name: &str) -> String {
    format!("\u{E000}{name}\u{E001}")
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

/// Hover entity names and item components are decoded when the command is
/// read, before Minecraft resolves text components. They therefore need
/// escaped macro text rather than an NBT text component.
pub(in crate::transpiler) fn quoted_hover_macro(command: &str) -> Option<QuotedHoverMacro> {
    let command = command.strip_prefix('$')?;
    let start = text_command_start(command)?;
    let (verb, target, action, message) = split_text_command(&command[start..]).ok()?;
    let normalized;
    let command = if let Some(json) = snbt_text::component_json(message.trim()) {
        normalized = match action {
            Some(action) => format!("{}{verb} {target} {action} {json}", &command[..start]),
            None => format!("{}{verb} {target} {json}", &command[..start]),
        };
        normalized.as_str()
    } else {
        command
    };
    let names = macro_argument_names(command);
    let mut arguments: Vec<(String, String)> = Vec::new();
    let mut result = String::new();
    let mut quotes = QuoteScan::default();
    let mut cursor = 0;
    for (at, _) in command.match_indices("$(") {
        quotes.advance(command, at);
        let Some(open) = quotes.open else { continue };
        // A quoted selector parameter in an outer `execute` precedes the
        // text command and has no component path to inspect.
        if open < start {
            continue;
        }
        let path = key_path(&command[start..open]);
        let Some(hover) = path.iter().rposition(|key| key == "hover_event") else {
            continue;
        };
        if !path
            .get(hover + 1)
            .is_some_and(|key| matches!(key.as_str(), "name" | "components"))
        {
            continue;
        }
        let end = at + 2 + command[at + 2..].find(')')?;
        let name = &command[at + 2..end];
        if !names.iter().any(|known| known == name) {
            continue;
        }
        let replacement = match arguments.iter().find(|(original, _)| original == name) {
            Some((_, escaped)) => escaped.clone(),
            None => {
                let mut index = arguments.len();
                let mut escaped = format!("_cobble_quoted_{index}");
                while names.contains(&escaped)
                    || arguments.iter().any(|(_, existing)| existing == &escaped)
                {
                    index += 1;
                    escaped = format!("_cobble_quoted_{index}");
                }
                arguments.push((name.to_string(), escaped.clone()));
                escaped
            }
        };
        result.push_str(&command[cursor..at]);
        result.push_str(&format!("$({replacement})"));
        cursor = end + 1;
    }
    if arguments.is_empty() {
        return None;
    }
    result.push_str(&command[cursor..]);
    Some(QuotedHoverMacro {
        command: format!("$return run {result}"),
        arguments,
    })
}

/// Parameters in whole integer arguments of raw commands. These uses must
/// count as number reads too: passing `"0.6"` to `scoreboard players set`
/// prevents the entire macro function from being instantiated.
pub(in crate::transpiler) fn integer_parameters(command: &str) -> Vec<String> {
    let command = command.trim_start_matches(['/', '$']);
    let words = split_command_words(command, usize::MAX);
    let mut first = 0;
    while words.get(first) == Some(&"execute") {
        let Some(run) = words[first + 1..].iter().position(|word| *word == "run") else {
            return Vec::new();
        };
        first += run + 2;
    }
    let candidates: &[&str] = match &words[first..] {
        ["scoreboard", "players", "set" | "add" | "remove", _, _, value, ..] => {
            std::slice::from_ref(value)
        }
        ["return", value] => std::slice::from_ref(value),
        ["trigger", _, "set" | "add", value] => std::slice::from_ref(value),
        ["give", _, _, value] | ["clear", _, _, value] => std::slice::from_ref(value),
        ["experience" | "xp", "add" | "set", _, value, ..] => std::slice::from_ref(value),
        _ => &[],
    };
    candidates
        .iter()
        .filter_map(|word| {
            word.strip_prefix('{')
                .and_then(|s| s.strip_suffix('}'))
                .or_else(|| word.strip_prefix("$(").and_then(|s| s.strip_suffix(')')))
        })
        .map(str::to_string)
        .collect()
}

/// Process Minecraft command strings with variable substitution
pub struct CommandProcessor<'a> {
    pub(in crate::transpiler) namespace: String,
    pub(in crate::transpiler) current_params: &'a [String],
    pub(in crate::transpiler) safe_plain_text: bool,
    pub(in crate::transpiler) symbols: &'a Symbols,
}

impl<'a> CommandProcessor<'a> {
    pub(in crate::transpiler) fn has_plain_parameter_message(
        command: &str,
        params: &[String],
    ) -> bool {
        if params.is_empty() || command.contains("$(") {
            return false;
        }
        let prepared = placeholders::command_template(command.strip_prefix('/').unwrap_or(command));
        let Ok(command) = prepared else { return false };
        let command = command.as_str();
        let Ok(pieces) = placeholders::checked_pieces(command) else {
            return false;
        };
        let Some(message) = text_message(command, &pieces, |name| {
            params.iter().any(|param| param == name)
        }) else {
            return false;
        };
        message.plain && pieces.iter().any(|piece| matches!(piece,
            Piece::Field { name, start, .. } if *start >= message.start && params.iter().any(|param| param == name)))
    }
    fn is_param(&self, name: &str) -> bool {
        self.current_params.contains(&name.to_string())
    }

    pub fn process_command_string(&self, cmd: &str) -> Result<String, TranspileError> {
        let prepared = placeholders::command_template(cmd)?;
        self.process_template_string(&prepared)
    }

    pub(in crate::transpiler) fn process_template_string(
        &self,
        cmd: &str,
    ) -> Result<String, TranspileError> {
        // Special handling for tellraw/title commands with scoreboard variables
        let Substituted {
            text: mut result,
            vars: scoreboard_vars_found,
            has_macro_vars,
            plain_message,
        } = self.substitute_placeholders(cmd, false)?;

        // Handle scoreboard variables - special processing for tellraw/title/say
        if !scoreboard_vars_found.is_empty() {
            result = self.handle_scoreboard_vars_in_command(&result, &scoreboard_vars_found)?;
        } else if plain_message {
            result = wrap_plain_message(&result);
        }
        if result.contains(['\u{E000}', '\u{E001}']) {
            return Err(UNREACHED_VARIABLE.into());
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
        if let Some(selector) = unknown_selector(expanded.trim_start_matches('$'))
            .or_else(|| unknown_selector_in_text_strings(&expanded))
        {
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
        let prepared = placeholders::command_template(cmd)?;
        self.template_say_macro(&prepared)
    }

    fn template_say_macro(&self, cmd: &str) -> Result<Option<SayMacro>, TranspileError> {
        if !cmd.trim().starts_with("say ") {
            return Ok(None);
        }
        let Substituted { text, vars, .. } = self.substitute_placeholders(cmd, true)?;
        if vars.is_empty() {
            return Ok(None);
        }

        let mut arguments: Vec<SayMacroArgument> = Vec::new();
        for name in vars {
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

        let text = crate::parser::tokenizer::trim_command_end(text.trim_start());
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
    /// and constant variables, reads `{{` and `}}` as literal braces (see
    /// [`placeholders`]), and collects the Cobble variables it finds. Each
    /// variable stays in the text as its [`var_marker`]. With
    /// `vars_as_macros`, a variable becomes `$(name)` instead.
    fn substitute_placeholders(
        &self,
        cmd: &str,
        vars_as_macros: bool,
    ) -> Result<Substituted, TranspileError> {
        let mut result = String::with_capacity(cmd.len());
        // A hand-written `$(name)` makes the line a macro line.
        let mut has_macro_vars = cmd.contains("$(");
        let mut scoreboard_vars_found = Vec::new();
        // The quote state before each placeholder, scanned once along the
        // line instead of from its start for every placeholder.
        let mut quotes = QuoteScan::default();
        let pieces = placeholders::checked_pieces(cmd)?;
        // The message of a `tellraw` or `title`, also after `execute ...
        // run`. A line with a hand-written `$(name)` is the author's macro
        // text and stays as written.
        let message = if vars_as_macros {
            None
        } else {
            text_message(cmd, &pieces, |name| self.is_param(name))
        };
        let mut plain_message = false;

        for piece in pieces {
            let (raw, var_name, start) = match piece {
                Piece::Text { text, .. } => {
                    result.push_str(&text);
                    continue;
                }
                Piece::Field { raw, name, start } => (raw, name, start),
            };
            quotes.advance(cmd, start);
            let in_message = message.as_ref().filter(|message| start >= message.start);
            if self.is_param(var_name)
                && in_message.is_some_and(|message| message.plain)
                && !cmd.contains("$(")
            {
                // A parameter in a message written as plain text, as
                // `/tellraw @a {msg}`, is macro text, so a direct call with
                // inline arguments shows it too; the message gets a
                // `{"text":...}` wrapper.
                if self.safe_plain_text {
                    scoreboard_vars_found.push(var_name.to_string());
                    result.push_str(&var_marker(var_name));
                } else {
                    plain_message = true;
                    result.push_str(&format!("$({})", var_name));
                    has_macro_vars = true;
                }
            } else if self.is_param(var_name)
                && !cmd.contains("$(")
                && in_message.is_some_and(|message| {
                    quotes
                        .open
                        .is_some_and(|open| message.opens_text_string(cmd, open))
                })
            {
                // Shown from `args` like a variable, because a macro value
                // with a `"` would break the JSON.
                scoreboard_vars_found.push(var_name.to_string());
                result.push_str(&var_marker(var_name));
            } else if self.is_param(var_name) {
                // Function parameter - convert to macro
                result.push_str(&format!("$({})", var_name));
                has_macro_vars = true;
            } else if let Some(const_value) = self.symbols.compile_time_constants.get(var_name) {
                result.push_str(&self.format_constant(*const_value));
            } else if self.symbols.variable_storage_paths.contains_key(var_name)
                || self.symbols.scoreboard_variables.contains(var_name)
            {
                // Storage or scoreboard variable found - collect for special handling
                scoreboard_vars_found.push(var_name.to_string());
                if vars_as_macros {
                    result.push_str(&format!("$({})", var_name));
                } else {
                    result.push_str(&var_marker(var_name));
                }
            } else if let Some(value) = self.symbols.variables.get(var_name) {
                // Constant variable - inline the value
                let replacement = match value {
                    Expression::Number(n) => n.to_string(),
                    Expression::String(s) => {
                        // Check if we're in a command that supports string literals
                        let cmd_trimmed = cmd.trim();

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

                        // Check if we're already inside quotes by counting
                        // unescaped quotes before this position.
                        let inside_quotes = quotes.open.is_some();

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
                    _ => return Err(undefined_placeholder(raw, var_name).into()),
                };
                result.push_str(&replacement);
            } else {
                // Printing `{name}` as text would be a silent wrong result.
                return Err(undefined_placeholder(raw, var_name).into());
            }
        }

        let escaped_message = message
            .as_ref()
            .is_some_and(|message| message.plain && cmd[message.start..].starts_with("{{"));
        Ok(Substituted {
            text: result,
            vars: scoreboard_vars_found,
            has_macro_vars,
            plain_message: plain_message || escaped_message,
        })
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
        let prepared = placeholders::command_template(cmd)?;
        self.template_copy_commands(&prepared)
    }

    pub(in crate::transpiler) fn template_copy_commands(
        &self,
        cmd: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let trimmed = cmd.trim();
        if text_command_start(trimmed).is_none() && !trimmed.starts_with("say ") {
            return Ok(Vec::new());
        }
        let Substituted { vars, .. } = self.substitute_placeholders(cmd, false)?;
        let storage = format!("{}:global", self.namespace);
        let mut copied: Vec<&str> = Vec::new();
        let mut commands = Vec::new();
        for name in &vars {
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
            .any(|name| text.contains(&var_marker(name)))
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
                let pattern = var_marker(var_name);
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

                let pattern = var_marker(&next_var_name);
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
        vars: &[String],
    ) -> Result<String, TranspileError> {
        let trimmed = cmd.trim();

        // A `tellraw` or `title`, also after `execute ... run`, shows the
        // variables as text components.
        if let Some(start) = text_command_start(cmd) {
            let text_command = &cmd[start..];
            let words = split_command_words(text_command.trim(), 4);
            let text_action = text_command.starts_with("tellraw ")
                || words.get(2).is_some_and(|action| {
                    matches!(*action, "title" | "subtitle" | "actionbar")
                        || is_placeholder_word(action)
                });
            if text_action {
                let converted = self.convert_to_tellraw_json(text_command, vars)?;
                return Ok(format!("{}{converted}", &cmd[..start]));
            }
        }

        if let Some(say_message) = trimmed.strip_prefix("say ") {
            // Fallback for callers that cannot add helper functions: the
            // transpiler runs `say` text with variables through
            // `say_macro` instead, which keeps `say`'s own output.
            let tellraw_cmd = format!("tellraw @a {}", serde_json::Value::from(say_message.trim()));
            return self.convert_to_tellraw_json(&tellraw_cmd, vars);
        }

        // For other commands with scoreboard variables, provide helpful error
        let var_names: Vec<_> = vars.iter().map(String::as_str).collect();
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
        vars: &[String],
    ) -> Result<String, TranspileError> {
        let (command, selector, action, message) = split_text_command(cmd)?;
        let assemble = |text: &str| match action {
            Some(action) => format!("{} {} {} {}", command, selector, action, text),
            None => format!("{} {} {}", command, selector, text),
        };
        let var_names: Vec<String> = vars.to_vec();
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
                .filter_map(|name| remaining.find(&var_marker(name)).map(|pos| (pos, name)))
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
            remaining = &remaining[pos + var_marker(var_name).len()..];
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
            in_selector_string = in_string
                && text[index + 1..].starts_with('@')
                && (follows_selector_key(&text[..index]) || is_score_holder_string(&text[..index]));
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
/// Whether `word` is one macro argument and nothing else, as `$(kind)`.
fn is_placeholder_word(word: &str) -> bool {
    word.strip_prefix("$(")
        .and_then(|rest| rest.strip_suffix(')'))
        .is_some_and(|name| !name.is_empty() && !name.contains(['(', ')']))
}

/// The byte offset of the `tellraw` or `title` command in `command`, at its
/// start or after `execute ... run`.
fn text_command_start(command: &str) -> Option<usize> {
    let mut start = command.len() - command.trim_start().len();
    loop {
        let rest = &command[start..];
        if rest.starts_with("tellraw ") || rest.starts_with("title ") {
            return Some(start);
        }
        if !rest.starts_with("execute ") {
            return None;
        }
        let words = split_command_words(rest, usize::MAX);
        let run = words.iter().position(|word| *word == "run")?;
        let next = words.get(run + 1)?;
        start = next.as_ptr() as usize - command.as_ptr() as usize;
    }
}

/// The message of a `tellraw` or `title` command.
struct TextMessage {
    /// Where the `tellraw` or `title` command starts.
    command_start: usize,
    /// Where its message starts.
    start: usize,
    /// Whether the message is plain text rather than a text component: it
    /// doesn't start with `[` or `"`, and a `{` there starts a placeholder
    /// or an escaped brace.
    plain: bool,
}

impl TextMessage {
    /// Whether the string opened by the quote at `open` is shown as text.
    fn opens_text_string(&self, command: &str, open: usize) -> bool {
        open == self.start
            || open > self.start && opens_text_string(&command[self.command_start..open])
    }
}

/// The message of the `tellraw` or `title` (`title`, `subtitle`,
/// `actionbar`, or a parameter standing for one) in `command`.
fn text_message(
    command: &str,
    pieces: &[Piece<'_>],
    is_param: impl Fn(&str) -> bool,
) -> Option<TextMessage> {
    let command_start = text_command_start(command)?;
    let text_command = &command[command_start..];
    let field_at = |offset: usize| {
        pieces.iter().find_map(|piece| match piece {
            Piece::Field { start, name, .. } if *start == offset => Some(*name),
            _ => None,
        })
    };
    let offset_of = |word: &str| word.as_ptr() as usize - command.as_ptr() as usize;
    let message = if text_command.starts_with("tellraw ") {
        let words = split_command_words(text_command, 3);
        (words.len() == 3).then(|| words[2])?
    } else {
        let words = split_command_words(text_command, 4);
        let action = *words.get(2)?;
        let text_action = matches!(action, "title" | "subtitle" | "actionbar")
            || field_at(offset_of(action)).is_some_and(&is_param)
                && action.starts_with('{')
                && action.ends_with('}');
        (words.len() == 4 && text_action).then(|| words[3])?
    };
    let start = offset_of(message);
    let plain = match message.as_bytes().first() {
        Some(b'[' | b'"' | b'\'') | None => false,
        Some(b'{') => message.starts_with("{{") || field_at(start).is_some(),
        Some(_) => true,
    };
    Some(TextMessage {
        command_start,
        start,
        plain,
    })
}

/// `command` with the plain-text message of its `tellraw` or `title` in a
/// `{"text":...}` wrapper, so a sentence with spaces works.
fn wrap_plain_message(command: &str) -> String {
    let Some(command_start) = text_command_start(command) else {
        return command.to_string();
    };
    let text_command = &command[command_start..];
    let limit = if text_command.starts_with("tellraw ") {
        3
    } else {
        4
    };
    let words = split_command_words(text_command, limit);
    if words.len() < limit {
        return command.to_string();
    }
    let message = words[limit - 1];
    let start = message.as_ptr() as usize - command.as_ptr() as usize;
    format!(
        "{}{}",
        &command[..start],
        serde_json::json!({ "text": message.trim_end() })
    )
}

/// Whether the text right after `chars` is inside a JSON string that is
/// shown as text: a `text` value or a string item of an array. Other strings,
/// such as a `selector`, a `color`, or a click event's `value`, take the
/// value as it is.
/// The double-quote state of a command up to a byte offset. A `"` after a
/// backslash doesn't count.
#[derive(Default)]
struct QuoteScan {
    scanned: usize,
    escaped: bool,
    /// The byte offset of the single or double quote opening the string.
    open: Option<usize>,
    delimiter: Option<char>,
}

impl QuoteScan {
    /// Scans on from the last offset to `to`, which must not be before it.
    fn advance(&mut self, text: &str, to: usize) {
        for (index, ch) in text[self.scanned..to].char_indices() {
            if self.escaped {
                self.escaped = false;
            } else if ch == '\\' && self.open.is_some() {
                self.escaped = true;
            } else if self.delimiter == Some(ch) {
                self.open = None;
                self.delimiter = None;
            } else if self.open.is_none() && matches!(ch, '\'' | '"') {
                self.open = Some(self.scanned + index);
                self.delimiter = Some(ch);
            }
        }
        self.scanned = to;
    }
}

/// Whether a string in a text component whose opening quote follows
/// `before` is shown as text: a `text` or show_text `value` field, or an
/// item of a text list. [`TextMessage::opens_text_string`] covers the whole
/// message.
fn opens_text_string(before: &str) -> bool {
    let before = before.trim();
    // Minecraft fills in text components only in the text itself and in
    // `show_text` hover text, so a `show_entity` name or a `show_item`
    // component takes the value as macro text.
    let path = key_path(before);
    if let Some(at) = path.iter().position(|key| key == "hover_event") {
        if path.get(at + 1).is_some_and(|key| key != "value") {
            return false;
        }
    }
    if before.ends_with('[') || before.ends_with(',') {
        return true;
    }
    let Some(key) = before.strip_suffix(':') else {
        return false;
    };
    let key = key.trim_end();
    if ends_with_key(key, "text") {
        return true;
    }
    // The hover text of `show_text`: `value` in a `hover_event` object.
    // Only `show_text` has a `value` there.
    ends_with_key(key, "value")
        && enclosing_object_start(key)
            .and_then(|start| before[..start].trim_end().strip_suffix(':'))
            .is_some_and(|outer| ends_with_key(outer.trim_end(), "hover_event"))
}

/// The first `@Name` that starts a `selector` string or a score
/// component's `name` in `text` and is not a vanilla selector. Minecraft
/// reads such a string as a selector, and an unknown name there shows
/// nothing.
fn unknown_selector_in_text_strings(text: &str) -> Option<String> {
    let mut in_string = false;
    let mut escape_next = false;
    for (index, ch) in text.char_indices() {
        if escape_next {
            escape_next = false;
        } else if ch == '\\' {
            escape_next = true;
        } else if ch == '"' {
            in_string = !in_string;
            let rest = &text[index + 1..];
            if in_string
                && rest.starts_with('@')
                && (follows_selector_key(&text[..index]) || is_score_holder_string(&text[..index]))
            {
                let name: String = rest[1..]
                    .chars()
                    .take_while(|c| c.is_ascii_alphanumeric() || *c == '_')
                    .collect();
                if !matches!(name.as_str(), "a" | "e" | "p" | "r" | "s" | "n") {
                    return Some(format!("@{name}"));
                }
            }
        }
    }
    None
}

/// Whether a string opened right after `before` is the `name` of a score
/// text component, `{"score":{"name":"@s",...}}`, which takes a selector.
fn is_score_holder_string(before: &str) -> bool {
    let path = key_path(before.trim_end());
    path.len() >= 2 && path[path.len() - 2] == "score" && path[path.len() - 1] == "name"
}

/// The keys of the JSON or SNBT objects that the end of `text` is inside,
/// outermost first, ending with the key whose value comes next, if any.
fn key_path(text: &str) -> Vec<String> {
    // The key of each open object or list, and the last key read in the
    // innermost one.
    let mut open: Vec<Option<String>> = Vec::new();
    let mut current: Option<String> = None;
    let mut word = String::new();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for ch in text.chars() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                word.clear();
            }
            ':' => current = Some(std::mem::take(&mut word).trim().to_string()),
            '{' | '[' => {
                open.push(current.take());
                word.clear();
            }
            '}' | ']' => {
                open.pop();
                current = None;
                word.clear();
            }
            ',' => {
                current = None;
                word.clear();
            }
            ch if ch.is_whitespace() => {}
            ch => word.push(ch),
        }
    }
    open.into_iter().flatten().chain(current).collect()
}

/// Whether `text` ends with the object key `key`, quoted or bare.
fn ends_with_key(text: &str, key: &str) -> bool {
    text.ends_with(&format!("\"{key}\""))
        || text.ends_with(&format!("'{key}'"))
        || text.strip_suffix(key).is_some_and(|rest| {
            !rest.ends_with(|c: char| c.is_alphanumeric() || c == '_' || c == '"' || c == '\'')
        })
}

/// The byte offset of the `{` that opens the object `text` ends inside.
fn enclosing_object_start(text: &str) -> Option<usize> {
    let mut depth = 0usize;
    let mut in_string = false;
    let bytes = text.as_bytes();
    for index in (0..bytes.len()).rev() {
        let byte = bytes[index];
        let escaped = index > 0 && bytes[index - 1] == b'\\';
        if byte == b'"' && !escaped {
            in_string = !in_string;
        } else if in_string {
        } else if byte == b'}' || byte == b']' {
            depth += 1;
        } else if byte == b'{' || byte == b'[' {
            if depth == 0 {
                return (byte == b'{').then_some(index);
            }
            depth -= 1;
        }
    }
    None
}

#[cfg(test)]
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
        let parts = split_command_words(cmd, 4);
        if parts.len() < 4 {
            return Err(
                "Title command requires action (title/subtitle/actionbar). Format: /title <selector> <action> <text>".into()
            );
        }
        Ok((command, parts[1], Some(parts[2]), parts[3]))
    } else {
        let parts = split_command_words(cmd, 3);
        if parts.len() < 3 {
            return Err("Invalid tellraw command format".into());
        }
        Ok((command, parts[1], None, parts[2]))
    }
}

/// `command` split at spaces into at most `limit` words, the last holding
/// the rest. A space inside a selector's `[...]`, braces, or quotes, as in
/// `@a[tag=x, limit=1]` or `@a[name="a b"]`, doesn't split.
pub(in crate::transpiler) fn split_command_words(command: &str, limit: usize) -> Vec<&str> {
    let mut words = Vec::new();
    let mut depth = 0usize;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut start = 0;
    for (index, ch) in command.char_indices() {
        if words.len() + 1 == limit {
            break;
        }
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
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth = depth.saturating_sub(1),
            ' ' if depth == 0 => {
                if index > start {
                    words.push(&command[start..index]);
                }
                start = index + 1;
            }
            _ => {}
        }
    }
    if start < command.len() {
        words.push(&command[start..]);
    }
    words
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
            safe_plain_text: false,
            symbols: &symbols,
        };

        let result = processor
            .process_command_string("tellraw @a {\"text\":f\"안녕하세요 {msg}\"}")
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
