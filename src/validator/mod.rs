pub mod arg_parsers;
pub mod command_tree;
pub mod registries;
pub mod snbt;
pub mod string_reader;
pub mod text_component;
pub mod values;

use command_tree::CommandNode;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
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

/// How many command nodes a command is checked through. Each one takes a
/// stack frame, and a debug build overflows a 2 MB thread at about 1,300.
const MAX_DEPTH: usize = 800;

/// How the error for an argument no parser accepts starts.
const GENERIC_ARGUMENT_ERROR: &str = "Unknown or invalid argument";

pub struct CommandValidator {
    root: CommandNode,
    /// Highest command permission level a validated command may use.
    max_permission_level: u8,
    /// The pack `validate_datapack` is checking, for references to its
    /// resources.
    pack_resources: RefCell<registries::PackResources>,
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
            pack_resources: RefCell::new(registries::PackResources::none()),
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
        self.validate_trimmed_command(java_trim(command))
    }

    /// A command as Minecraft reads it after trimming each line of the file.
    /// One continued by `\` and an empty line still ends in a space.
    fn validate_trimmed_command(&self, command: &str) -> Result<(), CommandValidationError> {
        if command.is_empty() || command.starts_with('#') {
            return Ok(()); // comment or empty
        }

        if !command.starts_with('$') {
            return self.walk_node(&self.root, &mut StringReader::new(command), 0, None);
        }
        // The values of a macro line can end in an argument that takes a
        // space at the end, as `say` does.
        let trimmed = java_trim(command);
        let macro_line = &trimmed[1..];
        Self::validate_macro_placeholders(trimmed)?;
        let mut reader = StringReader::for_macro_line(macro_line);
        self.walk_node(&self.root, &mut reader, 0, None)
            .or_else(|mut error| {
                // A placeholder can be empty or hold spaces, so spaces next to
                // one don't say where the arguments of the final command are.
                let mut loose = StringReader::for_macro_line(macro_line).with_loose_spaces();
                if self.walk_node(&self.root, &mut loose, 0, None).is_ok() {
                    return Ok(());
                }
                // A placeholder can also stand for several arguments, as in
                // `$fill $(from) $(to) stone` or `$data modify storage $(to)
                // set value 1`, so what follows it can be any argument after it.
                let spans = RefCell::new(PlaceholderSpans::default());
                let mut reader = StringReader::for_macro_line(macro_line);
                if self
                    .walk_node(&self.root, &mut reader, 0, Some(&spans))
                    .is_ok()
                {
                    return Ok(());
                }
                error.position += 1;
                Err(error)
            })
    }

    /// Validate an entire .mcfunction file.
    /// Returns a list of errors found.
    pub fn validate_mcfunction(&self, content: &str) -> Vec<ValidationError> {
        let mut errors = Vec::new();
        for (line_number, command) in command_lines(content) {
            if holds_no_command(&command) {
                continue;
            }
            if let Err(error) = self.validate_command_line(&command) {
                errors.push(ValidationError {
                    line_number,
                    command,
                    message: error.message,
                    position: error.position,
                });
            }
        }
        errors
    }

    /// One command of a function file. A file that ends in the middle of a
    /// continued line doesn't load.
    fn validate_command_line(&self, command: &str) -> Result<(), CommandValidationError> {
        if command.ends_with('\\') {
            return Err(CommandValidationError {
                message: "The file ends after a `\\`, which continues a line".to_string(),
                position: command.len(),
            });
        }
        self.validate_trimmed_command(command)
    }

    /// Validate all .mcfunction files in a datapack directory.
    pub fn validate_datapack(&self, dir: &Path) -> ValidationReport {
        self.pack_resources
            .replace(registries::PackResources::new(dir));
        let report = self.validate_function_files(dir);
        self.pack_resources
            .replace(registries::PackResources::none());
        report
    }

    fn validate_function_files(&self, dir: &Path) -> ValidationReport {
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
                            for (line_number, command) in command_lines(&content) {
                                if holds_no_command(&command) {
                                    report.commands_skipped += 1;
                                    continue;
                                }
                                report.commands_checked += 1;
                                if command.starts_with('$') {
                                    report.macro_commands_checked += 1;
                                }
                                if let Err(error) = self.validate_command_line(&command) {
                                    report.errors.push((
                                        path.to_path_buf(),
                                        ValidationError {
                                            line_number,
                                            command,
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
    fn walk_node<'a>(
        &'a self,
        node: &'a CommandNode,
        reader: &mut StringReader,
        depth: usize,
        spans: Option<&RefCell<PlaceholderSpans<'a>>>,
    ) -> Result<(), CommandValidationError> {
        let Some(spans) = spans else {
            return self.walk_children(node, reader, depth, None);
        };
        // The same node at the same place reads the same way, and with
        // placeholders standing for any number of arguments it is reached
        // along many paths.
        let key = (node as *const CommandNode, reader.cursor());
        if spans.borrow().failed.contains(&key) {
            return Err(CommandValidationError {
                message: GENERIC_ARGUMENT_ERROR.to_string(),
                position: reader.cursor(),
            });
        }
        let result = self.walk_children(node, reader, depth, Some(spans));
        if result.is_err() {
            spans.borrow_mut().failed.insert(key);
        }
        result
    }

    /// Reads the rest of the command as one of `node`'s children.
    fn walk_children<'a>(
        &'a self,
        node: &'a CommandNode,
        reader: &mut StringReader,
        depth: usize,
        spans: Option<&RefCell<PlaceholderSpans<'a>>>,
    ) -> Result<(), CommandValidationError> {
        let separated = reader.skip_separator();

        if !reader.can_read() {
            // Brigadier reads past a space only when something follows it.
            if separated && !reader.is_macro_line() {
                return Err(CommandValidationError {
                    message: format!(
                        "Incorrect argument for command at position {}: nothing follows the space at the end",
                        reader.cursor() - 1
                    ),
                    position: reader.cursor() - 1,
                });
            }
            // The value of a placeholder can hold the rest of the command.
            if node.executable || reader.ends_in_macro() {
                return Ok(());
            }
            return Err(CommandValidationError {
                message: "Incomplete command".to_string(),
                position: reader.cursor(),
            });
        }

        // Each step reads some of the command, so this can't loop forever. A
        // very long `execute` chain is valid, and Minecraft has no fixed limit:
        // the rest of it goes unchecked instead of overflowing the stack.
        if depth > MAX_DEPTH {
            return Ok(());
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
                    if child.executable && !reader.can_read() {
                        return Ok(());
                    }

                    match self.walk_node(self.next_node(child), reader, depth + 1, spans) {
                        Ok(()) => return Ok(()),
                        Err(error) => {
                            Self::keep_better_error(&mut best_error, &mut best_error_pos, error);
                            reader.set_cursor(saved);
                        }
                    }
                }
            }
        }

        // Then try argument children
        let mut argument_read = false;
        for (name, child) in &node.children {
            if child.node_type == "argument" {
                if let Some(ref parser_type) = child.parser {
                    let saved = reader.cursor();
                    if arg_parsers::parse_argument(reader, parser_type, child.properties.as_ref()) {
                        argument_read = true;
                        let token = reader.slice(saved, reader.cursor());
                        if let Some(problem) =
                            self.argument_problem(parser_type, child.properties.as_ref(), &token)
                        {
                            Self::keep_better_error(
                                &mut best_error,
                                &mut best_error_pos,
                                CommandValidationError {
                                    message: problem,
                                    position: saved,
                                },
                            );
                            reader.set_cursor(saved);
                            continue;
                        }
                        if let Some(error) = self.permission_error(name, child, reader.cursor()) {
                            if best_error.is_none() || error.position > best_error_pos {
                                best_error_pos = error.position;
                                best_error = Some(error);
                            }
                            reader.set_cursor(saved);
                            continue;
                        }
                        if child.executable && !reader.can_read() {
                            return Ok(());
                        }

                        match self.walk_node(self.next_node(child), reader, depth + 1, spans) {
                            Ok(()) => return Ok(()),
                            Err(error) => {
                                Self::keep_better_error(
                                    &mut best_error,
                                    &mut best_error_pos,
                                    error,
                                );
                                reader.set_cursor(saved);
                            }
                        }
                    }
                }
            }
        }

        // A placeholder can stand for a word of the command itself. Where no
        // argument fits, as in `$$(command)` and `$execute run $(command)`,
        // what follows it is known only when the function runs. Where one
        // does, what follows can still fit a literal instead, as in
        // `$return $(how) function a:b`. The retry that lets a placeholder
        // stand for several arguments checks what follows it either way.
        let saved = reader.cursor();
        if reader.is_macro_line() && reader.skip_token_with_macro() {
            if !argument_read && spans.is_none() {
                return Ok(());
            }
            let after_word = reader.cursor();
            for child in node.children.values() {
                if child.node_type == "literal"
                    && self
                        .walk_node(self.next_node(child), reader, depth + 1, spans)
                        .is_ok()
                {
                    return Ok(());
                }
                reader.set_cursor(after_word);
            }
            // The word can stand for any number of arguments, so what follows
            // it can start at any node below this one.
            if let Some(spans) = spans {
                let reach = self.reach(node, spans);
                for &next in reach.iter() {
                    if self.walk_node(next, reader, depth + 1, Some(spans)).is_ok() {
                        return Ok(());
                    }
                    reader.set_cursor(after_word);
                }
            }
            reader.set_cursor(saved);
        }

        Err(best_error.unwrap_or_else(|| {
            let remaining = reader.remaining();
            let preview = remaining.chars().take(40).collect::<String>();
            CommandValidationError {
                message: format!(
                    "{GENERIC_ARGUMENT_ERROR} at position {}: '{}'",
                    reader.cursor(),
                    preview
                ),
                position: reader.cursor(),
            }
        }))
    }

    /// The nodes the walk can be at after reading one or more arguments
    /// from `node`. An empty placeholder leaves two spaces in a row, which
    /// the retry with loose spaces covers.
    fn reach<'a>(
        &'a self,
        node: &'a CommandNode,
        spans: &RefCell<PlaceholderSpans<'a>>,
    ) -> Rc<Vec<&'a CommandNode>> {
        let key = node as *const CommandNode;
        if let Some(reach) = spans.borrow().reach.get(&key) {
            return Rc::clone(reach);
        }
        let mut seen = HashSet::new();
        let mut reach = Vec::new();
        let mut index = 0;
        let mut current = Some(node);
        while let Some(from) = current {
            for child in from.children.values() {
                let next = self.next_node(child);
                if seen.insert(next as *const CommandNode) {
                    reach.push(next);
                }
            }
            current = reach.get(index).copied();
            index += 1;
        }
        let reach = Rc::new(reach);
        spans.borrow_mut().reach.insert(key, Rc::clone(&reach));
        reach
    }

    /// The node that goes on after `child`.
    fn next_node<'a>(&'a self, child: &'a CommandNode) -> &'a CommandNode {
        match &child.redirect {
            Some(path) => child
                .resolve_redirect(&self.root, path)
                .unwrap_or(&self.root),
            None => child,
        }
    }

    /// Keeps the error from the branch that read furthest, and at the same
    /// position the one that says more than "invalid argument".
    fn keep_better_error(
        best_error: &mut Option<CommandValidationError>,
        best_error_pos: &mut usize,
        error: CommandValidationError,
    ) {
        let replaces_generic = error.position == *best_error_pos
            && !error.message.starts_with(GENERIC_ARGUMENT_ERROR)
            && best_error
                .as_ref()
                .is_none_or(|best| best.message.starts_with(GENERIC_ARGUMENT_ERROR));
        if best_error.is_none() || error.position > *best_error_pos || replaces_generic {
            *best_error_pos = error.position;
            *best_error = Some(error);
        }
    }

    /// What Minecraft would reject in an argument whose shape is right.
    fn argument_problem(
        &self,
        parser_type: &str,
        properties: Option<&serde_json::Value>,
        token: &str,
    ) -> Option<String> {
        let shape_problem = if parser_type == "minecraft:component" {
            text_component::problem(token)
                .map(|problem| format!("Invalid text component: {problem}"))
        } else {
            values::problem(parser_type, token)
                .or_else(|| values::target_problem(parser_type, properties, token))
                .or_else(|| registries::problem(parser_type, properties, token))
        };
        shape_problem.or_else(|| {
            self.pack_resources
                .borrow()
                .problem(parser_type, properties, token)
        })
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

/// What the check of a macro line in which a placeholder can stand for any
/// number of arguments has worked out so far.
#[derive(Default)]
struct PlaceholderSpans<'a> {
    /// The nodes below each node, by [`CommandValidator::reach`].
    reach: HashMap<*const CommandNode, Rc<Vec<&'a CommandNode>>>,
    /// The nodes that didn't read the rest of the line from a cursor.
    failed: HashSet<(*const CommandNode, usize)>,
}

/// Java's `String.trim`, which strips what is at most a space. Rust's `trim`
/// strips other spaces too, such as U+00A0, and Minecraft keeps those.
fn java_trim(text: &str) -> &str {
    text.trim_matches(|ch| ch <= ' ')
}

/// A blank line or a comment. A comment that ends the file in the middle of a
/// continued line stops the file from loading all the same.
fn holds_no_command(command: &str) -> bool {
    (command.is_empty() || command.starts_with('#')) && !command.ends_with('\\')
}

/// The commands of a function file with their first line numbers. A line that
/// ends in `\` goes on in the next one, whose indent doesn't count. Java ends
/// a line at a lone `\r` as well.
fn command_lines(content: &str) -> Vec<(usize, String)> {
    let content = content.replace("\r\n", "\n").replace('\r', "\n");
    let mut commands = Vec::new();
    let mut lines = content.lines().enumerate();
    while let Some((index, line)) = lines.next() {
        let mut command = java_trim(line).to_string();
        while command.ends_with('\\') {
            let Some((_, next)) = lines.next() else {
                break;
            };
            command.pop();
            command.push_str(java_trim(next));
        }
        commands.push((index + 1, command));
    }
    commands
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
            "compute default integer +1",
            "compute default float +1.5",
            r#"compute default integer {type:"minecraft:uniform",min_inclusive:1,max_inclusive:6}"#,
            "compute default integer ns:dice",
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
            // Minecraft reads a plain number as an id, such as `minecraft:5`.
            "compute default integer 5",
            "compute default integer -3",
            "compute default float 2.5",
            "compute default integer +a",
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

    /// `scoreboard`, `tp`, and a repeating `execute`, shaped like the real
    /// tree: each `execute` step redirects back to `execute`.
    fn execute_chain_validator() -> CommandValidator {
        CommandValidator::from_json_str(
            r#"{
                "type": "root",
                "children": {
                    "say": {"type": "literal", "children": {
                        "message": {"type": "argument", "parser": "minecraft:message", "executable": true}
                    }},
                    "scoreboard": {"type": "literal", "children": {"players": {"type": "literal", "children": {
                        "set": {"type": "literal", "children": {
                            "targets": {"type": "argument", "parser": "minecraft:score_holder", "children": {
                                "objective": {"type": "argument", "parser": "minecraft:objective", "children": {
                                    "score": {"type": "argument", "parser": "brigadier:integer", "executable": true}
                                }}
                            }}
                        }}
                    }}}},
                    "tp": {"type": "literal", "children": {
                        "targets": {"type": "argument", "parser": "minecraft:entity", "children": {
                            "location": {"type": "argument", "parser": "minecraft:vec3", "executable": true}
                        }}
                    }},
                    "execute": {"type": "literal", "children": {
                        "as": {"type": "literal", "children": {
                            "targets": {"type": "argument", "parser": "minecraft:entity", "redirect": ["execute"]}
                        }},
                        "run": {"type": "literal", "redirect": []}
                    }}
                }
            }"#,
        )
        .unwrap()
    }

    /// One command per kind of argument, to check what follows a space.
    fn spaces_validator() -> CommandValidator {
        let word = r#"{"type": "word"}"#;
        CommandValidator::from_json_str(&format!(
            r#"{{
                "type": "root",
                "children": {{
                    "say": {{"type": "literal", "children": {{
                        "message": {{"type": "argument", "parser": "minecraft:message", "executable": true}}
                    }}}},
                    "tag": {{"type": "literal", "children": {{
                        "name": {{"type": "argument", "parser": "brigadier:string", "properties": {word}, "children": {{
                            "last": {{"type": "literal", "executable": true}}
                        }}}}
                    }}}},
                    "tellraw": {{"type": "literal", "children": {{
                        "targets": {{"type": "argument", "parser": "minecraft:entity", "children": {{
                            "message": {{"type": "argument", "parser": "minecraft:component", "executable": true}}
                        }}}}
                    }}}},
                    "give": {{"type": "literal", "children": {{
                        "targets": {{"type": "argument", "parser": "minecraft:entity", "children": {{
                            "item": {{"type": "argument", "parser": "minecraft:item_stack", "executable": true, "children": {{
                                "count": {{"type": "argument", "parser": "brigadier:integer", "executable": true}}
                            }}}}
                        }}}}
                    }}}},
                    "clear": {{"type": "literal", "children": {{
                        "targets": {{"type": "argument", "parser": "minecraft:entity", "children": {{
                            "item": {{"type": "argument", "parser": "minecraft:item_predicate", "executable": true, "children": {{
                                "count": {{"type": "argument", "parser": "brigadier:integer", "executable": true}}
                            }}}}
                        }}}}
                    }}}},
                    "kill": {{"type": "literal", "executable": true, "children": {{
                        "targets": {{"type": "argument", "parser": "minecraft:entity", "executable": true}}
                    }}}},
                    "tp": {{"type": "literal", "children": {{
                        "location": {{"type": "argument", "parser": "minecraft:vec3", "executable": true}},
                        "targets": {{"type": "argument", "parser": "minecraft:entity", "children": {{
                            "location": {{"type": "argument", "parser": "minecraft:vec3", "executable": true}}
                        }}}}
                    }}}},
                    "setblock": {{"type": "literal", "children": {{
                        "pos": {{"type": "argument", "parser": "minecraft:block_pos", "children": {{
                            "block": {{"type": "argument", "parser": "minecraft:block_state", "executable": true}}
                        }}}}
                    }}}},
                    "function": {{"type": "literal", "children": {{
                        "name": {{"type": "argument", "parser": "minecraft:function", "children": {{
                            "with": {{"type": "literal", "children": {{
                                "storage": {{"type": "literal", "children": {{
                                    "source": {{"type": "argument", "parser": "minecraft:resource_location", "executable": true}}
                                }}}}
                            }}}}
                        }}}}
                    }}}},
                    "scoreboard": {{"type": "literal", "children": {{
                        "holder": {{"type": "argument", "parser": "minecraft:score_holder", "children": {{
                            "objective": {{"type": "argument", "parser": "minecraft:objective", "children": {{
                                "score": {{"type": "argument", "parser": "brigadier:integer", "executable": true}}
                            }}}}
                        }}}}
                    }}}},
                    "data": {{"type": "literal", "children": {{
                        "get": {{"type": "literal", "children": {{
                            "source": {{"type": "argument", "parser": "minecraft:resource_location", "children": {{
                                "path": {{"type": "argument", "parser": "minecraft:nbt_path", "children": {{
                                    "scale": {{"type": "argument", "parser": "brigadier:double", "executable": true}}
                                }}}}
                            }}}}
                        }}}},
                        "set": {{"type": "literal", "children": {{
                            "value": {{"type": "argument", "parser": "minecraft:nbt_tag", "executable": true}},
                            "compound": {{"type": "literal", "children": {{
                                "value": {{"type": "argument", "parser": "minecraft:nbt_compound_tag", "executable": true}}
                            }}}}
                        }}}}
                    }}}},
                    "loot": {{"type": "literal", "children": {{
                        "table": {{"type": "argument", "parser": "minecraft:loot_table", "executable": true}}
                    }}}},
                    "compute": {{"type": "literal", "children": {{
                        "provider": {{"type": "argument", "parser": "minecraft:context_int_provider", "executable": true}}
                    }}}},
                    "slots": {{"type": "literal", "children": {{
                        "slot": {{"type": "argument", "parser": "minecraft:slot_source", "executable": true}}
                    }}}},
                    "execute": {{"type": "literal", "children": {{
                        "as": {{"type": "literal", "children": {{
                            "targets": {{"type": "argument", "parser": "minecraft:entity", "redirect": ["execute"]}}
                        }}}},
                        "in": {{"type": "literal", "children": {{
                            "dimension": {{"type": "argument", "parser": "minecraft:dimension", "redirect": ["execute"]}}
                        }}}},
                        "run": {{"type": "literal", "redirect": []}}
                    }}}}
                }}
            }}"#
        ))
        .unwrap()
    }

    #[test]
    fn one_space_separates_arguments() {
        let validator = spaces_validator();
        for command in [
            "execute as @s  run say hi",
            "execute  as @s run say hi",
            "execute as  @s run say hi",
            "give @s stone  1",
            "kill  @s",
            "tp @s  1 2 3",
            "tp @s 1  2 3",
            "scoreboard @s  obj 1",
            "scoreboard @s obj  1",
            "data get a:b   1",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    #[test]
    fn some_arguments_can_be_empty() {
        let validator = spaces_validator();
        for command in [
            "tag  last",
            "scoreboard  obj 1",
            "scoreboard @s  1",
            "scoreboard   1",
            "function  with storage a:b",
            "execute in  run say hi",
            "data get  a 1",
            "data get a:b  1",
            // An empty coordinate counts as one of them.
            "setblock 1 2  stone",
            "setblock  2 3 stone",
            "setblock 1  3 stone",
            "tp 1 2  3",
            "tp @s  1 2",
            "tp @s ~  ~",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        for command in [
            "setblock 1  2 3 stone",
            "setblock 1 2 3  stone",
            // `^` can't go without a neighbour of its own.
            "tp ^  ^",
            "tp  ^ ^",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    #[test]
    fn some_arguments_keep_leading_spaces() {
        let validator = spaces_validator();
        for command in [
            "say  hi",
            "say   hi  there",
            "tellraw @a  \"x\"",
            "tellraw @a   {text:\"x\"}",
            "data set  1",
            "data set   {a:1}",
            "data set compound  {a:1}",
            "loot  {pools:[]}",
            "loot  chests/simple_dungeon",
            "compute   {type:\"minecraft:constant\",value:1}",
            "clear @s  stone",
            "clear @s   * 1",
            "clear @s  #minecraft:logs",
            // Spaces can stand before the tests of an item predicate too.
            "clear @s stone [damage=1]",
            "clear @s #minecraft:logs  [damage=1] 1",
            "slots  a:b",
            "slots  {type:\"minecraft:empty\"}",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        for command in [
            "give @s  stone",
            "kill  @s",
            "slots  weapon.*",
            "slots  weapon",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    /// Arguments with the `type` and `amount` of the real tree, and the
    /// resources a pack can define.
    fn targets_validator() -> CommandValidator {
        let target = |kind: &str, amount: &str, name: &str| {
            format!(
                r#""{name}": {{"type": "argument", "parser": "minecraft:entity", "executable": true, "properties": {{"type": "{kind}", "amount": "{amount}"}}}}"#
            )
        };
        let literal = |name: &str, child: &str| {
            format!(r#""{name}": {{"type": "literal", "children": {{{child}}}}}"#)
        };
        let argument = |name: &str, parser: &str| {
            format!(r#""{name}": {{"type": "argument", "parser": "{parser}", "executable": true}}"#)
        };
        let holder = r#""holder": {"type": "argument", "parser": "minecraft:score_holder", "executable": true, "properties": {"amount": "single"}}"#;
        let commands = [
            literal("tell", &target("players", "multiple", "targets")),
            literal("spectate", &target("players", "single", "target")),
            literal("look", &target("entities", "single", "target")),
            literal("kill", &target("entities", "multiple", "targets")),
            literal("score", holder),
            literal("say", &argument("text", "minecraft:component")),
            literal("style", &argument("style", "minecraft:style")),
            literal("loot", &argument("table", "minecraft:loot_table")),
            literal("feature", &argument("feature", "minecraft:feature")),
            literal("give", &argument("item", "minecraft:item_stack")),
        ];
        CommandValidator::from_json_str(&format!(
            r#"{{"type": "root", "children": {{{}}}}}"#,
            commands.join(",")
        ))
        .unwrap()
    }

    #[test]
    fn targets_have_to_fit_their_argument() {
        let validator = targets_validator();
        for command in [
            "tell @a",
            "tell @e[type=player]",
            "tell @n[type=minecraft:player]",
            "tell Steve",
            "spectate @p",
            "spectate @a[limit=1]",
            "spectate @e[type=player,limit=1]",
            "look @e[limit=01]",
            "kill @e",
            "kill 0-0-0-0-0",
            "score *",
            "score a:b",
            "score @e[limit=1]",
            "$tell @e[tag=$(tag)]",
            "$look @e[limit=$(limit)]",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        for (command, expected) in [
            ("tell @e", "`type=player`"),
            ("tell @n", "`type=player`"),
            ("tell @e[type=!player]", "`type=player`"),
            ("tell 0-0-0-0-0", "is a UUID"),
            ("spectate @a", "`limit=1`"),
            ("spectate @p[limit=2]", "`limit=1`"),
            ("look @e", "`limit=1`"),
            ("look @n[limit=2]", "`limit=1`"),
            ("score @a", "`limit=1`"),
            ("score @e[limit=2]", "`limit=1`"),
            ("kill @e[tag=a:b]", "can't hold `:`"),
            ("kill @e[limit=+1]", "`limit` takes"),
            ("kill @e[x=1e3]", "can't hold `e`"),
            ("kill @x", "`@x` is not a selector"),
        ] {
            let found = validator.validate_command(command).unwrap_err();
            assert!(found.contains(expected), "{command}: {found}");
        }
    }

    /// One command per kind of value, with the properties of the real tree.
    fn values_validator() -> CommandValidator {
        let argument = |name: &str, parser: &str, properties: &str| {
            format!(
                r#""{name}": {{"type": "argument", "parser": "{parser}", "executable": true, "properties": {properties}}}"#
            )
        };
        let literal = |name: &str, child: &str| {
            format!(r#""{name}": {{"type": "literal", "children": {{{child}}}}}"#)
        };
        let commands = [
            literal("say", &argument("text", "minecraft:message", "{}")),
            literal(
                "kill",
                &argument(
                    "targets",
                    "minecraft:entity",
                    r#"{"type": "entities", "amount": "multiple"}"#,
                ),
            ),
            literal("flag", &argument("value", "brigadier:bool", "{}")),
            literal("wait", &argument("time", "minecraft:time", r#"{"min": 0}"#)),
            literal("rain", &argument("time", "minecraft:time", r#"{"min": 1}"#)),
            literal(
                "hurt",
                &argument("amount", "brigadier:float", r#"{"min": 0.0}"#),
            ),
            literal("id", &argument("id", "minecraft:uuid", "{}")),
            literal("slots", &argument("slot", "minecraft:slot_source", "{}")),
            literal(
                "test",
                &argument("tests", "minecraft:resource_selector", "{}"),
            ),
            literal("function", &argument("name", "minecraft:function", "{}")),
            literal(
                "locate",
                &argument("name", "minecraft:resource_or_tag_key", "{}"),
            ),
            literal("give", &argument("item", "minecraft:item_stack", "{}")),
            literal("clear", &argument("item", "minecraft:item_predicate", "{}")),
            literal(
                "if_block",
                &argument("block", "minecraft:block_predicate", "{}"),
            ),
            literal(
                "setblock",
                &argument("block", "minecraft:block_state", "{}"),
            ),
            literal("data", &argument("path", "minecraft:nbt_path", "{}")),
            literal("pos", &argument("pos", "minecraft:block_pos", "{}")),
            literal(
                "add",
                &argument("criterion", "minecraft:objective_criteria", "{}"),
            ),
            literal(
                "enable",
                &argument("name", "brigadier:string", r#"{"type": "phrase"}"#),
            ),
        ];
        CommandValidator::from_json_str(&format!(
            r#"{{"type": "root", "children": {{{}}}}}"#,
            commands.join(",")
        ))
        .unwrap()
    }

    #[test]
    fn a_command_is_valid_when_minecraft_says_so() {
        let validator = values_validator();
        for command in [
            "say hi @a[tag=a]",
            "kill abcdefghijklmnop",
            "kill \"0-0-0-0-0\"",
            "flag \"true\"",
            "wait -0.5",
            "rain 0.5t",
            "rain 0.025s",
            "hurt 1",
            "id 0-0-0-0-0",
            "slots hotbar.8",
            "test *",
            "test minecraft:always_*",
            "function #",
            "function #a",
            "locate #",
            "give stone[damage=1,]",
            "give stone[enchantments={sharpness:255}]",
            "clear stone[! damage]",
            "if_block #minecraft:logs[a=]",
            "setblock oak_stairs[facing=\"north\"]",
            "setblock snow[layers=01]",
            "data $x",
            "data a$b",
            "pos 2147483647 0 0",
            "add minecraft.killed:zombie",
            "add killed:zombie",
            "enable 'a\\'b'",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        for command in [
            "say hi @a[",
            "say hi @e[limit=0] there",
            "kill abcdefghijklmnopq",
            "kill \"\"",
            "kill 00000000000000000000000000000000",
            "kill \"a\\nb\"",
            "flag \"True\"",
            "wait -0.51",
            "rain 0.4t",
            "hurt 340282356779733661637539395458142568448",
            "id 00000000000000000000000000000000",
            "slots hotbar.08",
            "test #a",
            "give stone[minecraft::damage=1]",
            "give stone[! damage]",
            "give stone[ ]",
            "give stone[custom_data=\"a\"]",
            "give stone[enchantments={sharpness:0}]",
            "give stone[dyed_color=[1,2]]",
            "give stone[item_model=\"a b\"]",
            "give stone[food={}]",
            "clear stone[damage=1,]",
            "clear stone[damage|]",
            "clear stone[!!damage]",
            "if_block #minecraft:logs[a=b:c]",
            "data \"\"",
            "data \"\\n\"",
            "pos 2147483648 0 0",
            "add minecraft.killed:zombi",
            "add dummy:x",
            "enable \"a\\nb\"",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    #[test]
    fn tokens_with_placeholders_are_known_when_the_function_runs() {
        let validator = values_validator();
        for command in [
            "$say @e[limit=$(n)]",
            "$say hi @a[ $(rest)",
            "$say @e[limit=0] $(rest)",
            "$kill $(name)abcdefghijklmnopqrstuvwxyz",
            "$kill \"$(name)\"",
            "$kill \"\\n$(name)\"",
            "$kill @e[distance=-$(x)]",
            "$kill @e[name=\"$(name)\"]",
            "$flag \"$(value)\"",
            "$flag tr$(ue)",
            "$wait 1$(unit)",
            "$wait $(time)x",
            "$rain 0$(fraction)",
            "$hurt 3$(digits)",
            "$id 0-$(rest)",
            "$slots hotbar.0$(index)",
            "$test a$(rest)",
            "$function #$(tag)",
            "$locate #$(tag)",
            "$give stone[$(components)]",
            "$give stone[custom_data=\"$(data)\"]",
            "$give stone[enchantments={sharpness:$(level)}]",
            "$give stone[dyed_color=[$(r),$(g)]]",
            "$give stone[item_model=\"$(model)\"]",
            "$give stone[food={nutrition:$(n)}]",
            "$clear stone[damage=$(x),]",
            "$clear stone[$(test)|]",
            "$if_block #minecraft:logs[a=b:$(c)]",
            "$setblock oak_stairs[facing=$(facing)]",
            "$data \"$(key)\"",
            "$data a.$(key)",
            "$pos $(x)2147483648 0 0",
            "$add minecraft.killed:$(mob)",
            "$add $(stat):zombie",
            "$enable \"$(name)\\n\"",
            // The value can close what the word before it opened.
            "$give stone[a=1, b=$(rest)",
            "$kill @e[tag=a, limit=$(rest)",
            "$kill \"a $(rest)",
            "$data a[0 $(rest)",
            "$data a[$(rest)",
            "$data a{b:1, c:$(rest)",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        for command in ["$flag maybe $(rest)", "$wait 1x $(rest)"] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
    }

    #[test]
    fn spaces_around_a_command_are_the_ones_java_trims() {
        let validator = values_validator();
        for command in [
            "\tkill @s",
            "kill @s\t",
            "kill @s\u{1c}",
            "\u{1c}kill @s",
            "kill @s\u{0}",
            "kill @e[\ttag=a]",
            "kill @e[tag=a\u{3000}]",
            "give stone[\u{3000}damage=1]",
            "clear \tstone",
            "say hi\u{a0}",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command:?}");
        }
        for command in [
            "\u{a0}say hi",
            "\u{3000}say hi",
            "\u{2003}kill @s",
            "kill @s\u{a0}",
            "kill @s\u{3000}",
            "kill @s\u{2007}",
            "kill @e[\u{a0}tag=a]",
            "give stone[\u{a0}damage=1]",
            "pos 1 2 3\u{3000}",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command:?}");
        }
    }

    #[test]
    fn references_are_looked_up_in_the_pack_being_validated() {
        let validator = targets_validator();
        let temp_dir = tempfile::TempDir::new().unwrap();
        for file in [
            "data/test/loot_table/here.json",
            "data/test/dialog/here.json",
            "data/test/enchantment/here.json",
            "data/test/worldgen/feature/here.json",
            "data/minecraft/loot_table/here.json",
            "data/minecraft/dialog/here.json",
            "data/minecraft/enchantment/here.json",
        ] {
            let path = temp_dir.path().join(file);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, "{}").unwrap();
        }
        let dialog = |id: &str| {
            format!(r#"{{text:"a",click_event:{{action:"show_dialog",dialog:"{id}"}}}}"#)
        };
        let lines = [
            "loot chests/simple_dungeon".to_string(),
            "loot here".to_string(),
            "loot minecraft:here".to_string(),
            "loot test:here".to_string(),
            "loot other:gone".to_string(),
            "feature oak".to_string(),
            "feature test:here".to_string(),
            "give stone[enchantments={here:1}]".to_string(),
            format!("say {}", dialog("here")),
            format!("say {}", dialog("test:here")),
            format!("style {}", dialog("server_links")),
        ];
        let bad_lines = [
            "loot gone".to_string(),
            "loot test:gone".to_string(),
            "feature test:gone".to_string(),
            "give stone[enchantments={\"test:gone\":1}]".to_string(),
            format!("say {}", dialog("gone")),
            format!("style {}", dialog("test:gone")),
        ];
        let function_dir = temp_dir.path().join("data/test/function");
        fs::create_dir_all(&function_dir).unwrap();
        fs::write(
            function_dir.join("main.mcfunction"),
            format!("{}\n{}\n", lines.join("\n"), bad_lines.join("\n")),
        )
        .unwrap();

        let report = validator.validate_datapack(temp_dir.path());
        let found: Vec<&String> = report
            .errors
            .iter()
            .map(|(_, error)| &error.command)
            .collect();
        assert_eq!(found, bad_lines.iter().collect::<Vec<_>>(), "{report:?}");

        // Outside `validate_datapack`, only the game's own ids count.
        assert_eq!(
            validator.validate_command("loot chests/simple_dungeon"),
            Ok(())
        );
        assert!(validator.validate_command("loot here").is_err());
    }

    #[test]
    fn spaces_near_a_placeholder_are_known_when_the_function_runs() {
        let validator = spaces_validator();
        for command in [
            "$tp @s  $(x) 1 2",
            "$tp @s $(x)  1 2",
            "$tp @s $(x) $(y)  $(z)",
            "$setblock 1 2  stone[$(state)]",
            "$scoreboard  $(objective) 1",
            "$function  with storage $(source)",
            "$execute as $(who)  run say hi",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        assert!(validator.validate_command("$give @s  $(item) x").is_err());
    }

    #[test]
    fn a_placeholder_can_stand_for_a_word_of_the_command() {
        let validator = execute_chain_validator();
        for command in [
            "$$(command)",
            "$$(a) $(b) $(c)",
            "$execute run $(command)",
            "$execute $(step) run say hi",
            "$execute as @s $(rest)",
            "$scoreboard players $(action) a b 1",
            "$scoreboard $(group) set a b 1",
            "$tp @s $(x)",
            "$tp @s $(x) $(y) $(z)",
            "$tp @s ~-$(x) ~ ~",
            "$tp @s ~$(x)1 ~ ~",
            "$tp @s ^$(a) ^ ^$(b)",
            "$tp $(who) ~ ~$(y) ~",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        // The word stands where no argument fits. Past that, or where an
        // argument did fit, the line is still checked.
        for command in [
            "$nope $(x)",
            "$scoreboard players nope a $(x) 1",
            "$scoreboard players set a b $(x) extra",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
        // A placeholder can stand for several arguments, so what follows it
        // can be any argument after it. Past that, the line is still checked.
        assert_eq!(
            validator.validate_command("$scoreboard players set $(who) 1"),
            Ok(())
        );
        for command in [
            "$scoreboard players set $(who) x",
            "$scoreboard players set $(who) 1 2 3 4",
            "$scoreboard players nope $(who) 1",
        ] {
            assert!(validator.validate_command(command).is_err(), "{command}");
        }
        // A placeholder that an argument took can stand for a literal, or
        // for the rest of the command.
        let validator = spaces_validator();
        for command in [
            "$function a:b $(how) storage a:b",
            "$function a:b wi$(th) storage a:b",
            "$function $(all)",
            "$scoreboard @s $(rest)",
            "$setblock 1 2 $(rest)",
            "$data set compound $(rest)",
        ] {
            assert_eq!(validator.validate_command(command), Ok(()), "{command}");
        }
        assert!(validator
            .validate_command("$scoreboard $(who) obj")
            .is_err());
        // Without the `$`, a placeholder is plain text.
        let validator = execute_chain_validator();
        assert!(validator.validate_command("$(command)").is_err());
        assert!(validator.validate_command("tp @s ~-$(x) ~ ~").is_err());
    }

    /// `execute` with `steps` copies of `as @s`, then `run <rest>`.
    fn execute_chain(steps: usize, rest: &str) -> String {
        format!("execute{} run {rest}", " as @s".repeat(steps))
    }

    #[test]
    fn deep_execute_chains_stop_being_checked_at_the_limit() {
        let run = || {
            let validator = execute_chain_validator();
            // Each step goes through two nodes, and `run` through one more.
            let last_checked = (MAX_DEPTH - 2) / 2;
            for steps in [1, 100, last_checked] {
                let good = execute_chain(steps, "say hi");
                assert_eq!(validator.validate_command(&good), Ok(()), "{steps} steps");
                let bad = execute_chain(steps, "nope");
                assert!(validator.validate_command(&bad).is_err(), "{steps} steps");
            }
            for steps in [last_checked + 1, MAX_DEPTH, 20 * MAX_DEPTH] {
                let good = execute_chain(steps, "say hi");
                assert_eq!(validator.validate_command(&good), Ok(()), "{steps} steps");
                let unchecked = execute_chain(steps, "nope");
                assert_eq!(
                    validator.validate_command(&unchecked),
                    Ok(()),
                    "{steps} steps"
                );
            }
        };
        // The size of a test thread, in a debug build's larger stack frames.
        std::thread::Builder::new()
            .stack_size(2 * 1024 * 1024)
            .spawn(run)
            .unwrap()
            .join()
            .unwrap();
    }

    #[test]
    fn command_lines_read_a_file_the_way_minecraft_does() {
        let lines = command_lines;
        let line = |number: usize, command: &str| (number, command.to_string());

        assert_eq!(
            lines("  say a  \n\n# note\nsay b"),
            [
                line(1, "say a"),
                line(2, ""),
                line(3, "# note"),
                line(4, "say b")
            ]
        );
        // The indent of the next line doesn't count, and neither do spaces
        // after the `\`.
        assert_eq!(lines("say a \\\n   b\n"), [line(1, "say a b")]);
        assert_eq!(lines("give @s\\\n stone\n"), [line(1, "give @sstone")]);
        assert_eq!(lines("say a \\   \nb\n"), [line(1, "say a b")]);
        assert_eq!(lines("say a \\\r\nb\r\n"), [line(1, "say a b")]);
        // A lone `\r` ends a line too, and only spaces at most U+0020 go.
        assert_eq!(
            lines("say a\rsay b\r\nsay c\n"),
            [line(1, "say a"), line(2, "say b"), line(3, "say c")]
        );
        assert_eq!(lines("say a\r"), [line(1, "say a")]);
        assert_eq!(
            lines("\u{a0}say a\u{3000}\n\tsay b\u{1f}\n"),
            [line(1, "\u{a0}say a\u{3000}"), line(2, "say b")]
        );
        assert_eq!(
            lines("say a\nsay b \\\nc \\\nd\nsay e\n"),
            [line(1, "say a"), line(2, "say b c d"), line(5, "say e")]
        );
        // A blank line ends the command, and a comment can continue too.
        assert_eq!(
            lines("say a \\\n\nsay b\n"),
            [line(1, "say a "), line(3, "say b")]
        );
        assert_eq!(lines("# a \\\nsay b\n"), [line(1, "# a say b")]);
        assert_eq!(lines("say a \\\n# b\n"), [line(1, "say a # b")]);
        // Only one `\` goes at each step, so a second one continues again.
        assert_eq!(lines("say a\\\\\nb\n"), [line(1, "say a\\b")]);
        assert_eq!(lines("say a\\\\\n\nsay b\n"), [line(1, "say asay b")]);
        assert_eq!(lines("\\\nsay a\n"), [line(1, "say a")]);
        // At the end of the file the `\` stays.
        assert_eq!(lines("say a \\"), [line(1, "say a \\")]);
        assert_eq!(lines("say a \\\n"), [line(1, "say a \\")]);
    }

    #[test]
    fn a_function_file_is_read_across_continued_lines() {
        let validator = fixture_validator();
        let file = "say a \\\n    b\n\nnope\n\ntellraw @a \\\n  \"x\"\nnope \\\n  c\n";
        let errors = validator.validate_mcfunction(file);
        let found: Vec<(usize, &str)> = errors
            .iter()
            .map(|error| (error.line_number, error.command.as_str()))
            .collect();
        assert_eq!(found, [(4, "nope"), (8, "nope c")]);
        // Without a space before the `\`, the words run together.
        assert!(!validator
            .validate_mcfunction("tellraw @a\\\n \"x\"\n")
            .is_empty());
        // Macro lines can continue like any other.
        assert!(validator
            .validate_mcfunction("$say $(x) \\\n  b\n$say \\\n  $(x)\n")
            .is_empty());
    }

    #[test]
    fn a_continued_line_can_end_in_a_space() {
        // Minecraft doesn't trim a command joined from a line ending in ` \`
        // and an empty line, and Brigadier reads past a space only when
        // something follows it: a 26.3 server refuses `kill @s ` with
        // "Incorrect argument for command at position 7".
        let validator = targets_validator();
        let errors = validator.validate_mcfunction("kill @s \\\n\n");
        let [error] = errors.as_slice() else {
            panic!("{errors:?}");
        };
        assert_eq!((error.command.as_str(), error.position), ("kill @s ", 7));
        assert!(error.message.starts_with("Incorrect argument for command"));
        assert_eq!(validator.validate_mcfunction("kill \\\n\n").len(), 1);
        // A message takes the space, and a lone command is trimmed.
        let validator = fixture_validator();
        assert!(validator.validate_mcfunction("say a \\\n\n").is_empty());
        assert_eq!(validator.validate_command("say a "), Ok(()));
        assert_eq!(targets_validator().validate_command("kill @s "), Ok(()));
    }

    #[test]
    fn a_file_can_not_end_in_a_continued_line() {
        let validator = fixture_validator();
        for file in [
            "say a \\",
            "say a \\\n",
            "say a \\\r\n",
            "say a\n\\",
            "say a \\\nb \\\n",
            "say a\\\\\n",
            "nope \\\n",
            "# only a comment \\",
            "# only a comment \\\n",
        ] {
            let errors = validator.validate_mcfunction(file);
            let [error] = errors.as_slice() else {
                panic!("{file:?}: {errors:?}");
            };
            assert_eq!(
                error.message, "The file ends after a `\\`, which continues a line",
                "{file:?}"
            );
        }
        for file in ["# a comment\n", "# a \\\nnote\n", "# a \\\n\n"] {
            let errors = validator.validate_mcfunction(file);
            assert!(errors.is_empty(), "{file:?}: {errors:?}");
        }

        let temp_dir = tempfile::TempDir::new().unwrap();
        let function_dir = temp_dir.path().join("data/test/function");
        fs::create_dir_all(&function_dir).unwrap();
        fs::write(function_dir.join("open.mcfunction"), "say a\n# b \\\n").unwrap();
        let report = validator.validate_datapack(temp_dir.path());
        let (_, error) = report.errors.first().expect("an error for the open line");
        assert_eq!((error.line_number, report.errors.len()), (2, 1));
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
        assert_eq!(v.validate_command("function #"), Ok(()));
    }

    #[test]
    fn test_validate_item_predicates_and_macro_offsets() {
        let v = match get_validator() {
            Some(v) => v,
            None => return,
        };
        assert!(v
            .validate_command(
                r#"execute if items entity @s weapon.mainhand *[minecraft:custom_data~{id:"blink"}] run say ok"#
            )
            .is_ok());
        assert!(v.validate_command("clear @s *").is_ok());
        assert!(v
            .validate_command("clear @s minecraft:stick[minecraft:damage=0]")
            .is_ok());
        assert!(v.validate_command("give @s *").is_err());
        assert!(v
            .validate_command("$execute rotated ~ 0 run tp @s ^ ^ ^$(distance)")
            .is_ok());
        assert!(v.validate_command("$tp @s ~$(dx) ~ ~$(dz)").is_ok());
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
