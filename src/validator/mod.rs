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

#[derive(Debug)]
pub struct ValidationReport {
    pub files_checked: usize,
    pub commands_checked: usize,
    pub commands_skipped: usize,
    pub errors: Vec<(PathBuf, ValidationError)>,
    pub source_map_errors: Vec<String>,
}

pub struct CommandValidator {
    root: CommandNode,
}

impl CommandValidator {
    pub fn from_file(path: &Path) -> Result<Self, String> {
        let root = CommandNode::from_file(path)?;
        Ok(Self { root })
    }

    /// Validate a single command string.
    /// Returns Ok(()) if the command is valid, Err with a message otherwise.
    pub fn validate_command(&self, command: &str) -> Result<(), String> {
        let trimmed = command.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            return Ok(()); // comment or empty
        }

        let command = trimmed.strip_prefix('$').unwrap_or(trimmed);
        let mut reader = StringReader::new(command);
        self.walk_node(&self.root, &mut reader, 0)
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
            if let Err(msg) = self.validate_command(trimmed) {
                errors.push(ValidationError {
                    line_number: line_num,
                    command: trimmed.to_string(),
                    message: msg,
                    position: 0,
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
            if path.is_file() {
                if let Some(ext) = path.extension() {
                    if ext == "mcfunction" {
                        report.files_checked += 1;
                        if let Ok(content) = std::fs::read_to_string(path) {
                            for (i, line) in content.lines().enumerate() {
                                let trimmed = line.trim();
                                if trimmed.is_empty() || trimmed.starts_with('#') {
                                    continue;
                                }
                                report.commands_checked += 1;
                                if let Err(msg) = self.validate_command(trimmed) {
                                    report.errors.push((
                                        path.to_path_buf(),
                                        ValidationError {
                                            line_number: i + 1,
                                            command: trimmed.to_string(),
                                            message: msg,
                                            position: 0,
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
    ) -> Result<(), String> {
        reader.skip_whitespace();

        if !reader.can_read() {
            if node.executable {
                return Ok(());
            }
            return Err("Incomplete command".to_string());
        }

        // Safety: prevent infinite recursion from redirect loops
        if depth > 100 {
            return Err("Command too deeply nested (possible redirect loop)".to_string());
        }

        let mut best_error: Option<String> = None;
        let mut best_error_pos: usize = 0;

        // Try literal children first (they have priority over arguments)
        for (name, child) in &node.children {
            if child.node_type == "literal" {
                let saved = reader.cursor();
                if reader.try_read_literal(name) {
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
                        Err(e) => {
                            let pos = reader.cursor();
                            if pos > best_error_pos {
                                best_error_pos = pos;
                                best_error = Some(e);
                            }
                            reader.set_cursor(saved);
                        }
                    }
                }
            }
        }

        // Then try argument children
        for child in node.children.values() {
            if child.node_type == "argument" {
                if let Some(ref parser_type) = child.parser {
                    let saved = reader.cursor();
                    if arg_parsers::parse_argument(reader, parser_type, child.properties.as_ref()) {
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
                            Err(e) => {
                                let pos = reader.cursor();
                                if pos > best_error_pos {
                                    best_error_pos = pos;
                                    best_error = Some(e);
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
            let preview_len = remaining.len().min(40);
            format!(
                "Unknown or invalid argument at position {}: '{}'",
                reader.cursor(),
                &remaining[..preview_len]
            )
        }))
    }

    fn is_at_end(reader: &StringReader) -> bool {
        let mut reader = reader.clone();
        reader.skip_whitespace();
        !reader.can_read()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn get_validator() -> Option<CommandValidator> {
        let commands_json = Path::new("data/commands.json");
        if !commands_json.exists() {
            eprintln!("Skipping validator tests: data/commands.json not found");
            return None;
        }
        Some(CommandValidator::from_file(commands_json).unwrap())
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
