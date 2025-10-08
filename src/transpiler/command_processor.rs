use crate::ast::Expression;
use std::collections::{HashMap, HashSet};

/// Process Minecraft command strings with variable substitution
pub struct CommandProcessor<'a> {
    pub current_params: &'a [String],
    pub scoreboard_variables: &'a HashSet<String>,
    pub variables: &'a HashMap<String, Expression>,
    pub variable_objectives: &'a HashMap<String, String>,
    pub selector_aliases: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
}

impl<'a> CommandProcessor<'a> {
    pub fn new(
        current_params: &'a [String],
        scoreboard_variables: &'a HashSet<String>,
        variables: &'a HashMap<String, Expression>,
        variable_objectives: &'a HashMap<String, String>,
        selector_aliases: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
    ) -> Self {
        Self {
            current_params,
            scoreboard_variables,
            variables,
            variable_objectives,
            selector_aliases,
            compile_time_constants,
        }
    }

    fn is_param(&self, name: &str) -> bool {
        self.current_params.contains(&name.to_string())
    }

    pub fn process_command_string(&self, cmd: &str) -> Result<String, String> {
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
                                        ));
                                    }

                                    // Check if we're already inside quotes by counting unescaped quotes before this position
                                    let before_var = &result[..i];
                                    let mut quote_count = 0;
                                    let mut prev_was_backslash = false;

                                    for ch in before_var.chars() {
                                        if ch == '"' && !prev_was_backslash {
                                            quote_count += 1;
                                        }
                                        prev_was_backslash = ch == '\\' && !prev_was_backslash;
                                    }

                                    let inside_quotes = quote_count % 2 == 1;

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
        for (start_idx, end_idx, replacement) in replacements.into_iter().rev() {
            result.replace_range(start_idx..end_idx, &replacement);
        }

        // Handle scoreboard variables - special processing for tellraw/title/say
        if !scoreboard_vars_found.is_empty() {
            result = self.handle_scoreboard_vars_in_command(&result, &scoreboard_vars_found)?;
        }

        // If the command has macro variables, prefix with $ for Minecraft 1.21.8+ macro system
        if has_macro_vars && !result.starts_with('$') {
            result = format!("${}", result);
        }

        // Replace selector aliases (@Name -> @a[...])
        // Only replace outside of string literals to avoid breaking JSON text
        for (alias_name, selector) in self.selector_aliases {
            let pattern = format!("@{}", alias_name);

            // Find all occurrences of the pattern
            let mut new_result = String::new();
            let mut last_end = 0;

            while let Some(pos) = result[last_end..].find(&pattern) {
                let abs_pos = last_end + pos;

                // Check if this occurrence is inside a string literal
                // Count quotes before this position to determine if we're in a string
                let before = &result[..abs_pos];
                let mut in_string = false;
                let mut escape_next = false;

                for ch in before.chars() {
                    if escape_next {
                        escape_next = false;
                        continue;
                    }
                    if ch == '\\' {
                        escape_next = true;
                        continue;
                    }
                    if ch == '"' {
                        in_string = !in_string;
                    }
                }

                // Only replace if not in a string
                if !in_string {
                    new_result.push_str(&result[last_end..abs_pos]);
                    new_result.push_str(selector);
                    last_end = abs_pos + pattern.len();
                } else {
                    // Keep the original @Name in strings
                    new_result.push_str(&result[last_end..abs_pos + pattern.len()]);
                    last_end = abs_pos + pattern.len();
                }
            }

            // Add remaining part
            new_result.push_str(&result[last_end..]);
            result = new_result;
        }

        Ok(result)
    }

    fn handle_scoreboard_vars_in_command(
        &self,
        cmd: &str,
        vars: &[(usize, usize, String)],
    ) -> Result<String, String> {
        let trimmed = cmd.trim();

        // Check if it's a tellraw or title command - we can auto-convert these
        if trimmed.starts_with("tellraw ") || trimmed.starts_with("title ") {
            return self.convert_to_tellraw_json(cmd, vars);
        }

        // Check if this command is using macro parameters (starts with $ or contains $(var))
        // Macro parameters are allowed in any command, including say
        let is_macro_command = trimmed.starts_with('$') || {
            // Check if all variables are macro parameters (not scoreboard variables)
            vars.iter().all(|(_, _, name)| {
                self.current_params.contains(name)
            })
        };

        if is_macro_command {
            // This is a macro command - allow it to pass through
            // The variables will be replaced with $(var) syntax
            return Ok(cmd.to_string());
        }

        // For other commands (say, etc.) with scoreboard variables, provide helpful error
        let var_names: Vec<_> = vars.iter().map(|(_, _, name)| name.as_str()).collect();
        Err(format!(
            "Cannot interpolate scoreboard variable{} {} in '{}' command.\n\
            Scoreboard variables cannot be displayed in simple text commands.\n\n\
            Solutions:\n\
            1. Use 'tellraw' instead of 'say' to display scores:\n\
               /tellraw @a [{{\"text\":\"Value: \"}},{{\"score\":{{\"name\":\"*\",\"objective\":\"temp\"}}}}]\n\
            2. Use a function parameter:\n\
               def show_value(val):\n\
                   /say Value: {{val}}\n\
               show_value(your_variable)",
            if var_names.len() > 1 { "s" } else { "" },
            var_names.join(", "),
            trimmed.split_whitespace().next().unwrap_or("unknown")
        ))
    }

    fn convert_to_tellraw_json(
        &self,
        cmd: &str,
        vars: &[(usize, usize, String)],
    ) -> Result<String, String> {
        // Parse the command to extract target selector and message
        // For title commands: "title <selector> <action> <message>"
        // For tellraw commands: "tellraw <selector> <message>"

        // First, split to get command type
        let first_parts: Vec<&str> = cmd.trim().splitn(2, ' ').collect();
        if first_parts.is_empty() {
            return Err("Empty command".to_string());
        }

        let command = first_parts[0];

        // Handle title and tellraw differently due to different number of arguments
        let (selector, action, message) = if command == "title" {
            // Title: "title <selector> <action> <message>"
            let parts: Vec<&str> = cmd.trim().splitn(4, ' ').collect();
            if parts.len() < 4 {
                return Err(format!(
                    "Title command requires action (title/subtitle/actionbar). Format: /title <selector> <action> <text>"
                ));
            }
            (parts[1], Some(parts[2]), parts[3])
        } else {
            // Tellraw: "tellraw <selector> <message>"
            let parts: Vec<&str> = cmd.trim().splitn(3, ' ').collect();
            if parts.len() < 3 {
                return Err("Invalid tellraw command format".to_string());
            }
            (parts[1], None, parts[2])
        };

        // Check if message is a JSON array
        let message_trimmed = message.trim();
        if message_trimmed.starts_with('[') {
            // Message is a JSON array
            if !vars.is_empty() {
                // Variables in JSON arrays are not supported
                return Err(format!(
                    "Cannot use variables inside JSON array text components.\n\
                     \n\
                     You wrote: {}\n\
                     \n\
                     Variables like {{{}}} cannot be automatically inserted into existing JSON arrays.\n\
                     \n\
                     Solution: Use Minecraft's score component syntax directly:\n\
                     /tellraw @a [{{\"text\":\"Score: \"}},{{\"score\":{{\"name\":\"{}\",\"objective\":\"temp\"}}}}]\n\
                     \n\
                     Or use a simple JSON object (not array) and let Cobble handle it:\n\
                     /tellraw @a {{\"text\":\"Score: {{{}}}\"}}\n\
                     \n\
                     This will automatically generate proper JSON with score components.",
                    cmd,
                    vars[0].2,
                    vars[0].2,
                    vars[0].2
                ));
            } else {
                // No variables - return JSON array as-is
                if let Some(action_token) = action {
                    return Ok(format!("{} {} {} {}", command, selector, action_token, message));
                } else {
                    return Ok(format!("{} {} {}", command, selector, message));
                }
            }
        }

        // If message is a JSON object, extract the text value
        // Example: {"text":"Hello {player}"} -> "Hello {player}"
        let mut message = message;
        if message_trimmed.starts_with('{') {
            // Try to extract text value from JSON
            if let Some(text_start) = message.find("\"text\":") {
                let after_text = &message[text_start + 7..].trim_start();
                // Find the string value after "text":
                if after_text.starts_with('"') {
                    // Find the closing quote (not escaped)
                    let mut end_pos = 1;
                    let chars: Vec<char> = after_text.chars().collect();
                    let mut prev_backslash = false;
                    while end_pos < chars.len() {
                        if chars[end_pos] == '"' && !prev_backslash {
                            break;
                        }
                        prev_backslash = chars[end_pos] == '\\' && !prev_backslash;
                        end_pos += 1;
                    }
                    if end_pos < chars.len() {
                        message = &after_text[1..end_pos];
                    }
                }
            }
        }

        // Build JSON array by replacing {var} with score components
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
