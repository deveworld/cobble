use super::{DataPack, GeneratedCommand, GeneratedCommandKind};
use crate::stdlib::EventType;
use crate::transpiler::vars_objective;

/// The load function that holds the runtime setup and module variable
/// initializers.
pub const INIT_FUNCTION: &str = "_cobble_init";

impl DataPack {
    /// Records an objective the init function must create. The source-level
    /// `temp` objective is recorded as this pack's `<namespace>.vars`.
    pub fn track_objective(&mut self, objective: &str) {
        let objective = if objective == vars_objective::SOURCE_OBJECTIVE {
            self.vars_objective()
        } else {
            objective.to_string()
        };
        self.used_objectives.insert(objective);
    }

    /// Records that the `#true_const`/`#false_const` holders are needed.
    pub fn track_boolean_constants(&mut self) {
        self.track_objective(vars_objective::SOURCE_OBJECTIVE);
        self.uses_boolean_constants = true;
    }

    /// Records that the pack has a runtime loop. A loop runs every iteration
    /// in one tick and can pass Minecraft's default limit of 65536 commands,
    /// but world settings change only when explicitly configured.
    pub fn track_runtime_loop(&mut self) {
        self.uses_runtime_loops = true;
    }

    fn runtime_setup_commands(&self) -> Vec<String> {
        let mut commands = Vec::new();
        if let Some(limit) = self.command_limit {
            commands.push(format!("gamerule max_command_sequence_length {limit}"));
        }

        let mut objectives: Vec<_> = self.used_objectives.iter().collect();
        objectives.sort();
        for objective in objectives {
            commands.push(format!("scoreboard objectives add {} dummy", objective));
        }

        if self.uses_boolean_constants {
            commands.extend(self.boolean_constant_commands());
        }

        commands
    }

    fn boolean_constant_commands(&self) -> [String; 2] {
        let objective = self.vars_objective();
        [
            format!("scoreboard players set #true_const {objective} 1"),
            format!("scoreboard players set #false_const {objective} 0"),
        ]
    }

    fn is_runtime_setup_command(&self, command: &str) -> bool {
        Self::is_runtime_setup_command_for(
            &self.used_objectives,
            &self.boolean_constant_commands(),
            self.command_limit,
            command,
        )
    }

    fn is_runtime_setup_command_for(
        used_objectives: &std::collections::HashSet<String>,
        boolean_constants: &[String; 2],
        command_limit: Option<u32>,
        command: &str,
    ) -> bool {
        // The gamerule only counts as setup when Cobble emits it, so the same
        // command written in a load handler stays.
        if command_limit
            .is_some_and(|limit| command == format!("gamerule max_command_sequence_length {limit}"))
            || boolean_constants.iter().any(|constant| constant == command)
        {
            return true;
        }

        let Some(rest) = command.strip_prefix("scoreboard objectives add ") else {
            return false;
        };
        let mut parts = rest.split_whitespace();
        let Some(objective) = parts.next() else {
            return false;
        };
        parts.next() == Some("dummy")
            && parts.next().is_none()
            && used_objectives.contains(objective)
    }

    pub fn runtime_setup_end_index(&self, commands: &[String]) -> usize {
        commands
            .iter()
            .position(|command| !self.is_runtime_setup_command(command))
            .unwrap_or(commands.len())
    }

    /// Puts the runtime setup (gamerule, objectives, boolean constants) at the
    /// start of `_cobble_init`, the first `minecraft:load` handler, so it runs
    /// before every other load function. Module variable initializers follow
    /// it in the same function. Packs without setup get no `_cobble_init`.
    pub fn ensure_init_function(&mut self) {
        let setup_commands = self.runtime_setup_commands();
        if setup_commands.is_empty() && !self.functions.contains_key(INIT_FUNCTION) {
            return;
        }

        let used_objectives = self.used_objectives.clone();
        let boolean_constants = self.boolean_constant_commands();
        let command_limit = self.command_limit;
        let original_commands = self
            .functions
            .get(INIT_FUNCTION)
            .cloned()
            .unwrap_or_default();
        let existing_metadata = self
            .command_metadata
            .remove(INIT_FUNCTION)
            .unwrap_or_default();

        // Drop the setup written by an earlier file of the build, then write
        // the current setup in front of the remaining commands.
        let mut updated_commands = setup_commands.clone();
        let mut kept_entries = Vec::new();
        for (old_index, command) in original_commands.into_iter().enumerate() {
            if Self::is_runtime_setup_command_for(
                &used_objectives,
                &boolean_constants,
                command_limit,
                &command,
            ) {
                continue;
            }
            kept_entries.push((old_index, command.clone()));
            updated_commands.push(command);
        }

        let mut updated_metadata =
            Self::metadata_for_commands(&setup_commands, GeneratedCommandKind::RuntimeSetup);
        let setup_len = setup_commands.len();
        for (new_offset, (old_index, command)) in kept_entries.into_iter().enumerate() {
            let generated = existing_metadata
                .get(&old_index)
                .cloned()
                .unwrap_or_else(|| {
                    GeneratedCommand::new(command, None, GeneratedCommandKind::ControlFlow)
                });
            updated_metadata.insert(setup_len + new_offset, generated);
        }
        updated_metadata = Self::complete_metadata(
            &updated_commands,
            updated_metadata,
            GeneratedCommandKind::ControlFlow,
        );

        self.command_metadata
            .insert(INIT_FUNCTION.to_string(), updated_metadata);
        self.functions
            .insert(INIT_FUNCTION.to_string(), updated_commands);
        self.stdlib
            .add_first_event_listener(EventType::Load, INIT_FUNCTION.to_string());
    }

    pub fn ensure_math_helper(&mut self, op: &str) {
        let func_name = format!("_cobble_math_{}", op);
        if self.functions.contains_key(&func_name) {
            return;
        }

        let mut commands = Vec::new();
        match op {
            "abs" => {
                commands.push(
                    "scoreboard players operation #math_result temp = #math_input temp".to_string(),
                );
                commands.push("scoreboard players set #math_temp temp -1".to_string());
                commands.push("execute if score #math_result temp matches ..-1 run scoreboard players operation #math_result temp *= #math_temp temp".to_string());
            }
            "min" => {
                commands.push(
                    "scoreboard players operation #math_result temp = #math_input temp".to_string(),
                );
                commands.push("execute if score #math_input2 temp < #math_result temp run scoreboard players operation #math_result temp = #math_input2 temp".to_string());
            }
            "max" => {
                commands.push(
                    "scoreboard players operation #math_result temp = #math_input temp".to_string(),
                );
                commands.push("execute if score #math_input2 temp > #math_result temp run scoreboard players operation #math_result temp = #math_input2 temp".to_string());
            }
            "sqrt" => {
                commands.push("scoreboard players set #math_result temp 0".to_string());
                commands.push(
                    "scoreboard players operation #sqrt_high temp = #math_input temp".to_string(),
                );
                commands.push("scoreboard players set #sqrt_low temp 0".to_string());
                commands.push("scoreboard players set #sqrt_limit temp 46340".to_string());
                commands.push("scoreboard players set #sqrt_two temp 2".to_string());
                commands.push(
                    "execute if score #sqrt_high temp matches ..-1 run scoreboard players set #sqrt_high temp 0"
                        .to_string(),
                );
                commands.push(
                    "execute if score #sqrt_high temp > #sqrt_limit temp run scoreboard players operation #sqrt_high temp = #sqrt_limit temp"
                        .to_string(),
                );

                for _ in 0..16 {
                    commands.push(
                        "scoreboard players operation #sqrt_mid temp = #sqrt_low temp".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid temp += #sqrt_high temp"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid temp /= #sqrt_two temp".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square temp = #sqrt_mid temp"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square temp *= #sqrt_mid temp"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square temp <= #math_input temp run scoreboard players operation #math_result temp = #sqrt_mid temp"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square temp <= #math_input temp run scoreboard players operation #sqrt_low temp = #sqrt_mid temp"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square temp <= #math_input temp run scoreboard players add #sqrt_low temp 1"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square temp > #math_input temp run scoreboard players operation #sqrt_high temp = #sqrt_mid temp"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square temp > #math_input temp run scoreboard players remove #sqrt_high temp 1"
                            .to_string(),
                    );
                }
            }
            _ => {}
        }

        self.add_function_with_kind(func_name, commands, GeneratedCommandKind::StdLib);
    }
}
