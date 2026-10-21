pub mod arg_parsers;
pub mod command_tree;
pub mod string_reader;

use command_tree::CommandNode;
use std::path::{Path, PathBuf};
use string_reader::StringReader;
use walkdir::WalkDir;

#[derive(Debug)]
pub struct ValidationError {
    pub line_number: usize,
    pub command: String,
    pub message: String,
    pub position: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandValidationError {
    pub message: String,
    pub position: usize,
}

#[derive(Debug)]
pub struct ValidationReport {
    pub files_checked: usize,
    pub commands_checked: usize,
    pub macro_commands_checked: usize,
    pub commands_skipped: usize,
    pub errors: Vec<(PathBuf, ValidationError)>,
    pub source_map_errors: Vec<String>,
}

pub struct CommandValidator {
    root: CommandNode,
    /// Highest command permission level a validated command may use.
    max_permission_level: u8,
}

impl CommandValidator {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let root = CommandNode::from_file(path)?;
        Ok(Self::new(root))
    }

    pub fn from_json_str(content: &str) -> Result<Self, String> {
        let root = CommandNode::from_json_str(content)?;
        Ok(Self::new(root))
    }

    fn new(root: CommandNode) -> Self {
        Self {
            root,
            max_permission_level: command_tree::DEFAULT_FUNCTION_PERMISSION_LEVEL,
        }
    }

    /// Validate as if functions ran at `level` (a server whose
    /// `function-permission-level` was raised from the default 2).
    pub fn with_max_permission_level(mut self, level: u8) -> Self {
        self.max_permission_level = level;
        self
    }

    /// A command node above the function permission level is not available
    /// to data pack functions: Minecraft refuses to load a function using it.
    fn permission_error(
        &self,
        name: &str,
        node: &CommandNode,
        position: usize,
    ) -> Option<CommandValidationError> {
        let (level, level_name) = node.required_permission_level()?;
        if level <= self.max_permission_level {
            return None;
        }
        let level_label = if level_name.is_empty() {
            level.to_string()
        } else {
            format!("{} ({})", level, level_name)
        };
        Some(CommandValidationError {
            message: format!(
                "`{}` requires permission level {}; data pack functions run at level {} (`function-permission-level`), so Minecraft rejects this command. If the server raised `function-permission-level`, set COBBLE_FUNCTION_PERMISSION_LEVEL to match",
                name, level_label, self.max_permission_level
            ),
            position,
        })
    }

    /// Validate a single command string.
    /// Returns Ok(()) if the command is valid, Err with a message otherwise.
    pub fn validate_command(&self, command: &str) -> Result<(), String> {
        self.validate_command_detailed(command)
            .map_err(|error| error.message)
    }

    /// Validate a single command string and retain the best-known error cursor.
    pub fn validate_command_detailed(&self, command: &str) -> Result<(), CommandValidationError> {
        let trimmed = command.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Ok(()); // comment or empty
        }

        let macro_offset = usize::from(trimmed.starts_with('$'));
        if macro_offset == 1 {
            Self::validate_macro_placeholders(trimmed)?;
        }
        let command = trimmed.strip_prefix('$').unwrap_or(trimmed);
        let mut reader = StringReader::new(command);
        self.walk_node(&self.root, &mut reader, 0)
            .map_err(|mut error| {
                error.position += macro_offset;
                error
            })
    }

    /// Validate an entire .mcfunction file.
    /// Returns a list of errors found.
    pub fn validate_mcfunction(&self, content: &str) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        for (i, line) in content.lines().enumerate() {
            let line_num = i + 1;
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Err(error) = self.validate_command_detailed(trimmed) {
                errors.push(ValidationError {
                    line_number: line_num,
                    command: trimmed.to_string(),
                    message: error.message,
                    position: error.position,
                });
            }
        }
        errors
    }

    /// Validate all .mcfunction files in a datapack directory.
    pub fn validate_datapack(&self, dir: &Path) -> ValidationReport {
        let mut report = ValidationReport {
            files_checked: 0,
            commands_checked: 0,
            macro_commands_checked: 0,
            commands_skipped: 0,
            errors: Vec::new(),
            source_map_errors: Vec::new(),
        };

        for entry in WalkDir::new(dir)
            .follow_links(false)
            .into_iter()
            .filter_map(|e| e.ok())
        {
            let path = entry.path();
            if entry.file_type().is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "mcfunction" {
                        report.files_checked += 1;
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for (i, line) in content.lines().enumerate() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() || trimmed.starts_with('#') {
                                    report.commands_skipped += 1;
                                    continue;
                                }
                                report.commands_checked += 1;
                                if trimmed.starts_with('$') {
                                    report.macro_commands_checked += 1;
                                }
                                if let Err(error) = self.validate_command_detailed(trimmed) {
                                    report.errors.push((
                                        path.to_path_buf(),
                                        ValidationError {
                                            line_number: i + 1,
                                            command: trimmed.to_string(),
                                            message: error.message,
                                            position: error.position,
                                        },
                                    ));
                                }
                            }
                        }
                    }
                }
            }
        }

        report
    }

    /// Tree-walking validation against the command tree.
    /// Tries to match the input against the node's children using backtracking.
    fn walk_node(
        &self,
        node: &CommandNode,
        reader: &mut StringReader,
        depth: usize,
    ) -> Result<(), CommandValidationError> {
        reader.skip_whitespace();

        if !reader.can_read() {
            if node.executable {
                return Ok(());
            }
            return Err(CommandValidationError {
                message: "Incomplete command".to_string(),
                position: reader.cursor(),
            });
        }

        // Safety: prevent infinite recursion from redirect loops
        if depth > 100 {
            return Err(CommandValidationError {
                message: "Command too deeply nested (possible redirect loop)".to_string(),
                position: reader.cursor(),
            });
        }

        let mut best_error: Option<CommandValidationError> = None;
        let mut best_error_pos: usize = 0;

        // Try literal children first (they have priority over arguments)
        for (name, child) in &node.children {
            if child.node_type == "literal" {
                let saved = reader.cursor();
                if reader.try_read_literal(name) {
                    if let Some(error) = self.permission_error(name, child, reader.cursor()) {
                        if best_error.is_none() || error.position > best_error_pos {
                            best_error_pos = error.position;
                            best_error = Some(error);
                        }
                        reader.set_cursor(saved);
                        continue;
                    }
                    if child.executable && Self::is_at_end(reader) {
                        return Ok(());
                    }

                    let target = if let Some(ref redirect_path) = child.redirect {
                        child
                            .resolve_redirect(&self.root, redirect_path)
                            .unwrap_or(&self.root)
                    } else {
                        child
                    };

                    match self.walk_node(target, reader, depth + 1) {
                        Ok(()) => return Ok(()),
                        Err(error) => {
                            let pos = error.position;
                            if pos > best_error_pos {
                                best_error_pos = pos;
                                best_error = Some(error);
                            }
                            reader.set_cursor(saved);
                        }
                    }
                }
            }
        }

        // Then try argument children
        for (name, child) in &node.children {
            if child.node_type == "argument" {
                if let Some(ref parser_type) = child.parser {
                    let saved = reader.cursor();
                    if arg_parsers::parse_argument(reader, parser_type, child.properties.as_ref()) {
                        if let Some(error) = self.permission_error(name, child, reader.cursor()) {
                            if best_error.is_none() || error.position > best_error_pos {
                                best_error_pos = error.position;
                                best_error = Some(error);
                            }
                            reader.set_cursor(saved);
                            continue;
                        }
                        if child.executable && Self::is_at_end(reader) {
                            return Ok(());
                        }

                        let target = if let Some(ref redirect_path) = child.redirect {
                            child
                                .resolve_redirect(&self.root, redirect_path)
                                .unwrap_or(&self.root)
                        } else {
                            child
                        };

                        match self.walk_node(target, reader, depth + 1) {
                            Ok(()) => return Ok(()),
                            Err(error) => {
                                let pos = error.position;
                                if pos > best_error_pos {
                                    best_error_pos = pos;
                                    best_error = Some(error);
                                }
                                reader.set_cursor(saved);
                            }
                        }
                    }
                }
            }
        }

        Err(best_error.unwrap_or_else(|| {
            let remaining = reader.remaining();
            let preview = remaining.chars().take(40).collect::<String>();
            CommandValidationError {
                message: format!(
                    "Unknown or invalid argument at position {}: '{}'",
                    reader.cursor(),
                    preview
                ),
                position: reader.cursor(),
            }
        }))
    }

    fn is_at_end(reader: &StringReader) -> bool {
        let mut reader = reader.clone();
        reader.skip_whitespace();
        !reader.can_read()
    }

    /// Minecraft's macro line rules (`StringTemplate`): every `$(...)` must
    /// be closed and name a valid variable (letters, digits, `_`), and a
    /// macro line must contain at least one variable ("No variables in
    /// macro"). Any violation stops the whole function from loading.
    fn validate_macro_placeholders(command: &str) -> Result<(), CommandValidationError> {
        let chars = command.chars().collect::<Vec<_>>();
        if !command.contains("$(") {
            return Err(CommandValidationError {
                message: "Macro line has no `$(...)` variables; Minecraft rejects macro lines without variables".to_string(),
                position: 0,
            });
        }
        let mut index = 0;
        while index + 1 < chars.len() {
            if chars[index] == '$' && chars[index + 1] == '(' {
                let start = index;
                index += 2;
                let name_start = index;
                while index < chars.len() && chars[index] != ')' {
                    index += 1;
                }
                if index >= chars.len() {
                    return Err(CommandValidationError {
                        message: format!("Unclosed macro placeholder at position {}", start),
                        position: start,
                    });
                }
                if index == name_start {
                    return Err(CommandValidationError {
                        message: format!("Empty macro placeholder at position {}", start),
                        position: start,
                    });
                }
                let name = chars[name_start..index].iter().collect::<String>();
                // `Character.isLetterOrDigit` or `_`, like `StringTemplate`.
                if !name.chars().all(|ch| ch.is_alphanumeric() || ch == '_') {
                    return Err(CommandValidationError {
                        message: format!(
                            "Invalid macro variable name `{}` at position {}",
                            name, start
                        ),
                        position: start,
                    });
                }
            }
            index += 1;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn get_validator() -> Option<CommandValidator> {
        let commands_json = Path::new("data/commands.json");
        if !commands_json.exists() {
            eprintln!("Skipping validator tests: data/commands.json not found");
            return None;
        }
        Some(CommandValidator::from_file(commands_json).unwrap())
    }

    fn fixture_validator() -> CommandValidator {
        CommandValidator::from_json_str(
            r#"{
                "type": "root",
                "children": {
                    "say": {
                        "type": "literal",
                        "children": {
                            "message": {
                                "type": "argument",
                                "parser": "minecraft:message",
                                "executable": true
                            }
                        }
                    },
                    "tellraw": {
                        "type": "literal",
                        "children": {
                            "targets": {
                                "type": "argument",
                                "parser": "minecraft:entity",
                                "children": {
                                    "message": {
                                        "type": "argument",
                                        "parser": "minecraft:component",
                                        "executable": true
                                    }
                                }
                            }
                        }
                    },
                    "execute": {
                        "type": "literal",
                        "children": {
                            "run": {
                                "type": "literal",
                                "redirect": []
                            }
                        }
                    },
                    "return": {
                        "type": "literal",
                        "children": {
                            "run": {
                                "type": "literal",
                                "redirect": []
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap()
    }

    /// A small tree mirroring the real report's shapes for permissions,
    /// float ranges inside selectors, particles, and greedy messages.
    fn permission_fixture_validator() -> CommandValidator {
        let require = |level: &str| {
            format!(
                r#""permissions": {{"type": "minecraft:require", "permission": {{"type": "minecraft:command_level", "level": "{level}"}}}}"#
            )
        };
        CommandValidator::from_json_str(&format!(
            r#"{{
                "type": "root",
                "children": {{
                    "stop": {{"type": "literal", "executable": true, {owners}}},
                    "tick": {{
                        "type": "literal",
                        {admins},
                        "children": {{"freeze": {{"type": "literal", "executable": true}}}}
                    }},
                    "say": {{
                        "type": "literal",
                        {gamemasters},
                        "children": {{
                            "message": {{"type": "argument", "parser": "minecraft:message", "executable": true}}
                        }}
                    }},
                    "kill": {{
                        "type": "literal",
                        "children": {{
                            "targets": {{"type": "argument", "parser": "minecraft:entity", "executable": true, "properties": {{"type": "entities", "amount": "multiple"}}}}
                        }}
                    }},
                    "particle": {{
                        "type": "literal",
                        "children": {{
                            "name": {{"type": "argument", "parser": "minecraft:particle", "executable": true}}
                        }}
                    }},
                    "execute": {{
                        "type": "literal",
                        "children": {{"run": {{"type": "literal", "redirect": []}}}}
                    }}
                }}
            }}"#,
            owners = require("owners"),
            admins = require("admins"),
            gamemasters = require("gamemasters"),
        ))
        .unwrap()
    }

    #[test]
    fn commands_above_function_permission_level_are_rejected() {
        let validator = permission_fixture_validator();

        let error = validator.validate_command_detailed("stop").unwrap_err();
        assert!(
            error
                .message
                .contains("`stop` requires permission level 4 (owners)"),
            "{error:?}"
        );
        let error = validator
            .validate_command_detailed("tick freeze")
            .unwrap_err();
        assert!(error.message.contains("level 3 (admins)"), "{error:?}");
        let error = validator
            .validate_command_detailed("execute run stop")
            .unwrap_err();
        assert!(error.message.contains("`stop`"), "{error:?}");

        // Level-2 commands are fine, and a server with a raised
        // function-permission-level accepts the rest.
        assert!(validator.validate_command("say hi").is_ok());
        let raised = permission_fixture_validator().with_max_permission_level(4);
        assert!(raised.validate_command("stop").is_ok());
        assert!(raised.validate_command("tick freeze").is_ok());
    }

    #[test]
    fn selector_float_ranges_with_lower_bound_are_valid() {
        let validator = permission_fixture_validator();
        for command in [
            "kill @e[distance=1..5]",
            "kill @e[distance=1..]",
            "kill @e[distance=..5]",
            "kill @e[distance=1.5..2.5]",
            "kill @e[x_rotation=-90..0]",
        ] {
            assert!(
                validator.validate_command(command).is_ok(),
                "{command}: {:?}",
                validator.validate_command(command)
            );
        }
    }

    #[test]
    fn particle_options_compound_is_valid() {
        let validator = permission_fixture_validator();
        assert!(validator
            .validate_command("particle dust{color:[1.0,0.0,0.0],scale:1.0}")
            .is_ok());
        assert!(validator
            .validate_command("particle minecraft:flame")
            .is_ok());
        assert!(validator
            .validate_command("particle dust{color:[1.0,0.0,0.0]")
            .is_err());
    }

    /// Minecraft 26.3 argument types: `slot_source`, `team_color`,
    /// `swing_animation`, and the `ResourceOrIdArgument` family.
    fn argument_types_26_3_fixture() -> CommandValidator {
        let argument = |parser: &str, rest: &str| {
            format!(r#"{{"type": "argument", "parser": "{parser}"{rest}}}"#)
        };
        let executable = r#", "executable": true"#;
        CommandValidator::from_json_str(&format!(
            r#"{{
                "type": "root",
                "children": {{
                    "item": {{"type": "literal", "children": {{
                        "modify": {{"type": "literal", "children": {{
                            "entity": {{"type": "literal", "children": {{
                                "targets": {{"type": "argument", "parser": "minecraft:entity", "properties": {{"type": "entities", "amount": "multiple"}}, "children": {{
                                    "slot": {{"type": "argument", "parser": "minecraft:slot_source", "children": {{
                                        "modifier": {modifier}
                                    }}}}
                                }}}}
                            }}}}
                        }}}}
                    }}}},
                    "compute": {{"type": "literal", "children": {{
                        "default": {{"type": "literal", "children": {{
                            "integer": {{"type": "literal", "children": {{"provider": {int_provider}}}}},
                            "float": {{"type": "literal", "children": {{"provider": {float_provider}}}}}
                        }}}}
                    }}}},
                    "place": {{"type": "literal", "children": {{
                        "feature": {{"type": "literal", "children": {{"feature": {feature}}}}}
                    }}}},
                    "swing": {{"type": "literal", "children": {{"animation": {swing}}}}},
                    "team": {{"type": "literal", "children": {{
                        "color": {{"type": "literal", "children": {{
                            "reset": {{"type": "literal", "executable": true}},
                            "value": {team_color}
                        }}}}
                    }}}}
                }}
            }}"#,
            modifier = argument("minecraft:loot_modifier", executable),
            int_provider = argument("minecraft:context_int_provider", executable),
            float_provider = argument("minecraft:context_float_provider", executable),
            feature = argument("minecraft:feature", executable),
            swing = argument("minecraft:swing_animation", executable),
            team_color = argument("minecraft:team_color", executable),
        ))
        .unwrap()
    }

    #[test]
    fn minecraft_26_3_argument_types() {
        let validator = argument_types_26_3_fixture();
        for command in [
            "item modify entity @s container.0 ns:double",
            "item modify entity @s container.* ns:double",
            "item modify entity @s weapon ns:double",
            "item modify entity @s ns:hands ns:double",
            r#"item modify entity @s {type:"minecraft:slot_range",slots:"hotbar.4"} {type:"minecraft:set_count",count:2}"#,
            "compute default integer 5",
            "compute default integer -3",
            r#"compute default integer {type:"minecraft:uniform",min_inclusive:1,max_inclusive:6}"#,
            "compute default integer ns:dice",
            "compute default float 2.5",
            "place feature minecraft:oak",
            r#"place feature {type:"minecraft:no_op"}"#,
            "swing stab",
            "swing whack",
            "team color dark_purple",
            "team color reset",
        ] {
            assert!(
                validator.validate_command(command).is_ok(),
                "{command}: {:?}",
                validator.validate_command(command)
            );
        }
        for command in [
            "item modify entity @s Armor.Head ns:double",
            "item modify entity @s {unclosed ns:double",
            // Bare lowercase words are provider/feature IDs, so only
            // malformed IDs and unclosed SNBT are syntax errors.
            "compute default integer Five",
            "compute default integer {type:\"minecraft:uniform\"",
            "place feature Oak",
            "swing slap",
            "team color DarkPurple",
            "team color darkpurple",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    #[test]
    fn macro_lines_follow_minecraft_template_rules() {
        let validator = permission_fixture_validator();
        let error = validator
            .validate_command_detailed("$say plain")
            .unwrap_err();
        assert!(error.message.contains("no `$(...)` variables"), "{error:?}");
        let error = validator
            .validate_command_detailed("$say $(bad-name)")
            .unwrap_err();
        assert!(
            error.message.contains("Invalid macro variable name"),
            "{error:?}"
        );
        assert!(validator.validate_command("$say $(força)").is_ok());
    }

    #[test]
    fn greedy_message_after_macro_placeholder_is_valid() {
        let validator = permission_fixture_validator();
        assert!(validator.validate_command("$say $(x) raw text").is_ok());
        assert!(validator.validate_command("$say hello $(x)").is_ok());
    }

    #[test]
    fn fixture_validator_reports_error_positions() {
        let validator = fixture_validator();

        let error = validator
            .validate_command_detailed("titel @a actionbar bad")
            .unwrap_err();

        assert_eq!(error.position, 0);
        assert!(error.message.contains("Unknown or invalid argument"));
    }

    #[test]
    fn fixture_validator_handles_redirects_and_macro_prefixes() {
        let validator = fixture_validator();

        assert!(validator.validate_command("execute run say ok").is_ok());
        assert!(validator.validate_command("return run say ok").is_ok());
        assert!(validator
            .validate_command(r#"$tellraw $(player) {"text":"ok"}"#)
            .is_ok());
    }

    #[test]
    fn fixture_validator_rejects_malformed_macro_placeholders() {
        let validator = fixture_validator();

        for command in [
            "$say $(message",
            "$say $()",
            r#"$tellraw $(player) {"text":"$(bad"}"#,
        ] {
            let error = validator.validate_command_detailed(command).unwrap_err();
            assert!(
                error.message.contains("macro placeholder"),
                "unexpected error for {command}: {:?}",
                error
            );
        }
    }

    #[test]
    fn fixture_validator_allows_literal_macro_syntax_in_non_macro_commands() {
        let validator = fixture_validator();

        assert!(validator
            .validate_command(r#"tellraw @a {"text":"$(price"}"#)
            .is_ok());
    }

    #[test]
    fn datapack_validation_counts_checked_macro_commands() {
        let validator = fixture_validator();
        let temp_dir = tempfile::TempDir::new().unwrap();
        let function_dir = temp_dir.path().join("data/test/function");
        fs::create_dir_all(&function_dir).unwrap();
        fs::write(
            function_dir.join("macro.mcfunction"),
            "$tellraw $(player) {\"text\":\"ok\"}\n# comment\nsay done\n",
        )
        .unwrap();

        let report = validator.validate_datapack(temp_dir.path());

        assert_eq!(report.files_checked, 1);
        assert_eq!(report.commands_checked, 2);
        assert_eq!(report.macro_commands_checked, 1);
        assert_eq!(report.commands_skipped, 1);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_validate_function_accepts_function_tags() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("function demo:hooks/start").is_ok());
        assert!(v.validate_command("function #demo:hooks/start").is_ok());
        assert!(v
            .validate_command("execute if function #demo:checks run say ok")
            .is_ok());
        assert!(v.validate_command("function #").is_err());
    }

    #[test]
    fn test_validate_say() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("say hello world").is_ok());
        assert!(v.validate_command("say").is_err()); // missing message
    }

    #[test]
    fn test_validate_tellraw() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v
            .validate_command("tellraw @a {\"text\":\"Hello\",\"color\":\"green\"}")
            .is_ok());
        assert!(v
            .validate_command("tellraw @a [{\"text\":\"Hello\"}]")
            .is_ok());
    }

    #[test]
    fn test_validate_scoreboard() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v
            .validate_command("scoreboard objectives add test dummy")
            .is_ok());
        assert!(v
            .validate_command("scoreboard players set x temp 10")
            .is_ok());
        assert!(v
            .validate_command("scoreboard players operation x temp += y temp")
            .is_ok());
    }

    #[test]
    fn test_validate_execute() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("execute as @a run say hello").is_ok());
        assert!(v
            .validate_command("execute as @a at @s run particle flame ~ ~1 ~")
            .is_ok());
        assert!(v
            .validate_command("execute if score x temp matches 1..5 run say match")
            .is_ok());
        assert!(v.validate_command("execute if entity @s").is_ok());
        assert!(v
            .validate_command("execute if block ~ ~ ~ minecraft:stone")
            .is_ok());
    }

    #[test]
    fn test_validate_function() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("function cobble:test").is_ok());
    }

    #[test]
    fn test_validate_comments_and_macros() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("# comment").is_ok());
        assert!(v.validate_command("").is_ok());
        assert!(v.validate_command("$say $(message)").is_ok());
        assert!(v
            .validate_command("$give $(player) minecraft:$(item) $(count)")
            .is_ok());
        assert!(v.validate_command("$tp $(player) $(x) $(y) $(z)").is_ok());
        assert!(v
            .validate_command(
                "$execute if entity $(player)[nbt={Inventory:[{id:\"minecraft:diamond\"}]}] run say found"
            )
            .is_ok());
        assert!(v
            .validate_command(
                "$execute store result score selected temp run data get storage cobble:global vars.items[$(idx)]"
            )
            .is_ok());
        assert!(v
            .validate_command(
                "$execute store result score selected temp run data get storage cobble:global vars.config.$(key)"
            )
            .is_ok());
        assert!(v
            .validate_command(
                "$summon minecraft:armor_stand $(x) $(y) $(z) {Invisible:1b,CustomName:'{\"text\":\"Checkpoint_$(id)\"}'}"
            )
            .is_ok());
        assert!(v
            .validate_command("$particle minecraft:end_rod $(x) $(y) $(z) 0.5 1 0.5 0.01 20")
            .is_ok());
        assert!(v
            .validate_command("$title $(player) actionbar {\"text\":\"Kit count: $(count)\"}")
            .is_ok());
        assert!(v.validate_command("$titel $(player) actionbar hi").is_err());
        assert!(v.validate_command("$swing $(player) bogus").is_err());
    }

    #[test]
    fn test_validate_particle() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("particle flame ~ ~1 ~").is_ok());
    }

    #[test]
    fn test_validate_data_commands() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("data get entity @s Pos").is_ok());
    }

    #[test]
    fn test_validate_setblock() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("setblock ~ ~ ~ minecraft:stone").is_ok());
        assert!(v
            .validate_command("setblock 0 64 0 minecraft:oak_stairs[facing=north]")
            .is_ok());
    }

    #[test]
    fn test_validate_kill() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("kill @e[type=zombie]").is_ok());
        assert!(v.validate_command("kill").is_ok()); // kill is executable without args
    }

    #[test]
    fn test_validate_gamemode() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("gamemode creative @s").is_ok());
        assert!(v.validate_command("gamemode survival").is_ok());
        assert!(v.validate_command("gamemode flying @s").is_err());
        assert!(v
            .validate_command("gamemode creative @e[type=zombie]")
            .is_err());
    }

    #[test]
    fn test_validate_tp() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("tp @s 0 64 0").is_ok());
        assert!(v.validate_command("tp @s @p").is_ok());
    }

    #[test]
    fn test_validate_give() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("give @s minecraft:diamond 64").is_ok());
        assert!(v.validate_command("give @s diamond").is_ok());
        assert!(v.validate_command("give @s minecraft:diamond -1").is_err());
    }

    #[test]
    fn test_validate_schedule() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v
            .validate_command("schedule function cobble:tick 1t")
            .is_ok());
        assert!(v
            .validate_command("schedule function cobble:tick -1t")
            .is_err());
    }

    #[test]
    fn test_validate_scoreboard_operation_rejects_invalid_ops() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v
            .validate_command("scoreboard players operation x temp >= y temp")
            .is_err());
        assert!(v
            .validate_command("scoreboard players operation x temp <= y temp")
            .is_err());
    }

    #[test]
    fn test_validate_rejects_invalid_selector_shapes_and_enums() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("data get entity @a Pos").is_err());
        assert!(v.validate_command("dialog clear @e[type=zombie]").is_err());
        assert!(v.validate_command("kill @x").is_err());
        assert!(v.validate_command("kill @e[foo=bar]").is_err());
        assert!(v
            .validate_command("team modify matrix color ultraviolet")
            .is_err());
        assert!(v
            .validate_command("waypoint modify @s color hex nope")
            .is_err());
        assert!(v
            .validate_command("execute anchored nose run say hi")
            .is_err());
        assert!(v
            .validate_command("scoreboard objectives setdisplay sideways points")
            .is_err());
        assert!(v.validate_command("execute align xx run say hi").is_err());
        // Since 26.3 a slot is a `slot_source`: an unknown lowercase name such
        // as `armor.tail` is a syntactically valid slot source ID (resolved
        // against the registry at load), but uppercase is never valid.
        assert!(v
            .validate_command("item replace entity @s Armor.Head with minecraft:stone")
            .is_err());
        assert!(v
            .validate_command("give @s minecraft:diamond{Enchantments:[]}")
            .is_err());
    }

    #[test]
    fn test_validate_rejects_unbalanced_syntax_and_empty_ranges() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v.validate_command("tellraw @a {\"text\":\"hi\"").is_err());
        assert!(v
            .validate_command("data merge entity @s {Glowing:1b")
            .is_err());
        assert!(v.validate_command("kill @e[type=zombie").is_err());
        assert!(v
            .validate_command("execute if score score temp matches .. run say hi")
            .is_err());
        assert!(v.validate_command("random roll ..").is_err());
    }

    #[test]
    fn test_validate_mcfunction_file() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        let content = "\
# This is a comment
say hello
scoreboard objectives add test dummy
scoreboard players set x temp 10
execute as @a run say hi

# Another comment
kill @e[type=zombie]
";
        let errors = v.validate_mcfunction(content);
        assert!(
            errors.is_empty(),
            "Unexpected validation errors: {:?}",
            errors
                .iter()
                .map(|e| format!("L{}: {} ({})", e.line_number, e.message, e.command))
                .collect::<Vec<_>>()
        );
    }
}
