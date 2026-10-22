use super::{DataPack, GeneratedCommand, GeneratedCommandKind};
use crate::stdlib::EventType;
use crate::transpiler::vars_objective;

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

    fn runtime_setup_commands(&self) -> Vec<String> {
        let mut commands = vec!["gamerule max_command_sequence_length 1000000000".to_string()];

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
            command,
        )
    }

    fn is_runtime_setup_command_for(
        used_objectives: &std::collections::HashSet<String>,
        boolean_constants: &[String; 2],
        command: &str,
    ) -> bool {
        if command == "gamerule max_command_sequence_length 1000000000"
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

    pub fn ensure_init_function(&mut self) {
        // Get or create the init function from load event handlers
        let load_handlers = self.stdlib.get_event_handlers(&EventType::Load);

        if let Some(init_func_name) = load_handlers.first().cloned() {
            let setup_commands = self.runtime_setup_commands();
            let used_objectives = self.used_objectives.clone();
            let boolean_constants = self.boolean_constant_commands();
            // Add objectives to the beginning of the load function
            if let Some(commands) = self.functions.get_mut(&init_func_name) {
                let original_commands = commands.clone();
                let existing_metadata = self
                    .command_metadata
                    .remove(&init_func_name)
                    .unwrap_or_default();

                let mut updated_commands = setup_commands.clone();
                let mut kept_entries = Vec::new();
                for (old_index, command) in original_commands.into_iter().enumerate() {
                    if Self::is_runtime_setup_command_for(
                        &used_objectives,
                        &boolean_constants,
                        &command,
                    ) {
                        continue;
                    }
                    kept_entries.push((old_index, command.clone()));
                    updated_commands.push(command);
                }

                let mut updated_metadata = Self::metadata_for_commands(
                    &setup_commands,
                    GeneratedCommandKind::RuntimeSetup,
                );
                let setup_len = setup_commands.len();
                for (new_offset, (old_index, command)) in kept_entries.into_iter().enumerate() {
                    let new_index = setup_len + new_offset;
                    let generated =
                        existing_metadata
                            .get(&old_index)
                            .cloned()
                            .unwrap_or_else(|| {
                                GeneratedCommand::new(
                                    command,
                                    None,
                                    GeneratedCommandKind::ControlFlow,
                                )
                            });
                    updated_metadata.insert(new_index, generated);
                }

                updated_metadata = Self::complete_metadata(
                    &updated_commands,
                    updated_metadata,
                    GeneratedCommandKind::ControlFlow,
                );

                self.command_metadata
                    .insert(init_func_name.clone(), updated_metadata);
                *commands = updated_commands;
            }
        } else if !self.used_objectives.is_empty() {
            // No load handler exists, create a default init function
            let commands = self.runtime_setup_commands();

            self.add_function_with_kind(
                "_cobble_init".to_string(),
                commands,
                GeneratedCommandKind::RuntimeSetup,
            );
            self.stdlib
                .add_event_listener(EventType::Load, "_cobble_init".to_string());
        }
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
