use super::{DataPack, GeneratedCommandKind};
use crate::stdlib::EventType;

impl DataPack {
    pub fn track_objective(&mut self, objective: &str) {
        self.used_objectives.insert(objective.to_string());
    }

    pub fn ensure_init_function(&mut self) {
        // Get or create the init function from load event handlers
        let load_handlers = self.stdlib.get_event_handlers(&EventType::Load);

        if let Some(init_func_name) = load_handlers.first().cloned() {
            // Add objectives to the beginning of the load function
            if let Some(commands) = self.functions.get_mut(&init_func_name) {
                let mut setup_commands = Vec::new();

                // Add gamerule first
                let gamerule_cmd = "gamerule max_command_sequence_length 1000000000".to_string();
                if !commands.contains(&gamerule_cmd) {
                    setup_commands.push(gamerule_cmd);
                }

                // Then add objectives
                let mut objectives: Vec<_> = self.used_objectives.iter().collect();
                objectives.sort();
                for objective in objectives {
                    let obj_cmd = format!("scoreboard objectives add {} dummy", objective);
                    // Only add if not already present
                    if !commands.contains(&obj_cmd) {
                        setup_commands.push(obj_cmd);
                    }
                }

                // Initialize Boolean literal constants if __internal__ objective is used
                if self.used_objectives.contains("__internal__") {
                    let internal_init_1 =
                        "scoreboard players set #true_const __internal__ 1".to_string();
                    let internal_init_2 =
                        "scoreboard players set #false_const __internal__ 0".to_string();
                    if !commands.contains(&internal_init_1) {
                        setup_commands.push(internal_init_1);
                    }
                    if !commands.contains(&internal_init_2) {
                        setup_commands.push(internal_init_2);
                    }
                }

                // Prepend setup commands to existing commands
                if !setup_commands.is_empty() {
                    let inserted_count = setup_commands.len();
                    let mut updated_commands = setup_commands;
                    updated_commands.extend(commands.clone());

                    let mut updated_metadata = Self::metadata_for_commands(
                        &updated_commands[..inserted_count],
                        GeneratedCommandKind::RuntimeSetup,
                    );
                    if let Some(existing_metadata) = self.command_metadata.remove(&init_func_name) {
                        for (index, command) in existing_metadata {
                            updated_metadata.insert(index + inserted_count, command);
                        }
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
            }
        } else if !self.used_objectives.is_empty() {
            // No load handler exists, create a default init function
            let mut commands = Vec::new();

            // Add gamerule first
            commands.push("gamerule max_command_sequence_length 1000000000".to_string());

            // Then add objectives
            let mut objectives: Vec<_> = self.used_objectives.iter().collect();
            objectives.sort();
            for objective in objectives {
                commands.push(format!("scoreboard objectives add {} dummy", objective));
            }

            // Initialize Boolean literal constants if __internal__ objective is used
            if self.used_objectives.contains("__internal__") {
                commands.push("scoreboard players set #true_const __internal__ 1".to_string());
                commands.push("scoreboard players set #false_const __internal__ 0".to_string());
            }

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
