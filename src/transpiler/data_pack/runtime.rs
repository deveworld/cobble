use super::{DataPack, GeneratedCommand, GeneratedCommandKind};
use crate::stdlib::EventType;

impl DataPack {
    pub fn track_objective(&mut self, objective: &str) {
        self.used_objectives.insert(objective.to_string());
    }

    fn runtime_setup_commands(&self) -> Vec<String> {
        let mut commands = vec!["gamerule max_command_sequence_length 1000000000".to_string()];

        let mut objectives: Vec<_> = self.used_objectives.iter().collect();
        objectives.sort();
        for objective in objectives {
            commands.push(format!("scoreboard objectives add {} dummy", objective));
        }

        if self.used_objectives.contains("__internal__") {
            commands.push("scoreboard players set #true_const __internal__ 1".to_string());
            commands.push("scoreboard players set #false_const __internal__ 0".to_string());
        }

        commands
    }

    fn is_runtime_setup_command(&self, command: &str) -> bool {
        Self::is_runtime_setup_command_for(&self.used_objectives, command)
    }

    fn is_runtime_setup_command_for(
        used_objectives: &std::collections::HashSet<String>,
        command: &str,
    ) -> bool {
        if command == "gamerule max_command_sequence_length 1000000000"
            || command == "scoreboard players set #true_const __internal__ 1"
            || command == "scoreboard players set #false_const __internal__ 0"
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
                    if Self::is_runtime_setup_command_for(&used_objectives, &command) {
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
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("scoreboard players set #math_temp math -1".to_string());
                commands.push("execute if score #math_result math matches ..-1 run scoreboard players operation #math_result math *= #math_temp math".to_string());
            }
            "min" => {
                commands.push(
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("execute if score #math_input2 math < #math_result math run scoreboard players operation #math_result math = #math_input2 math".to_string());
            }
            "max" => {
                commands.push(
                    "scoreboard players operation #math_result math = #math_input math".to_string(),
                );
                commands.push("execute if score #math_input2 math > #math_result math run scoreboard players operation #math_result math = #math_input2 math".to_string());
            }
            "sqrt" => {
                commands.push("scoreboard players set #math_result math 0".to_string());
                commands.push(
                    "scoreboard players operation #sqrt_high math = #math_input math".to_string(),
                );
                commands.push("scoreboard players set #sqrt_low math 0".to_string());
                commands.push("scoreboard players set #sqrt_limit math 46340".to_string());
                commands.push("scoreboard players set #sqrt_two math 2".to_string());
                commands.push(
                    "execute if score #sqrt_high math matches ..-1 run scoreboard players set #sqrt_high math 0"
                        .to_string(),
                );
                commands.push(
                    "execute if score #sqrt_high math > #sqrt_limit math run scoreboard players operation #sqrt_high math = #sqrt_limit math"
                        .to_string(),
                );

                for _ in 0..16 {
                    commands.push(
                        "scoreboard players operation #sqrt_mid math = #sqrt_low math".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid math += #sqrt_high math"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_mid math /= #sqrt_two math".to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "scoreboard players operation #sqrt_square math *= #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players operation #math_result math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players operation #sqrt_low math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math <= #math_input math run scoreboard players add #sqrt_low math 1"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math > #math_input math run scoreboard players operation #sqrt_high math = #sqrt_mid math"
                            .to_string(),
                    );
                    commands.push(
                        "execute if score #sqrt_square math > #math_input math run scoreboard players remove #sqrt_high math 1"
                            .to_string(),
                    );
                }
            }
            _ => {}
        }

        self.add_function_with_kind(func_name, commands, GeneratedCommandKind::StdLib);
    }
}
