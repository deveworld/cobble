use crate::ast::*;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_if(
        &mut self,
        if_stmt: &IfStatement,
    ) -> Result<(), TranspileError> {
        // Preprocess condition to handle complex expressions
        let processed_condition = self.preprocess_condition(&if_stmt.condition)?;

        let translated = self.translate_condition(&processed_condition)?;
        let condition_cmd = self.lower_translated_condition(translated)?;

        let branch_var = if !if_stmt.elif_branches.is_empty() || if_stmt.else_branch.is_some() {
            let var = format!("if_branch_{}", self.next_temp_id());
            self.data_pack.track_objective("temp");
            if let Some(ref mut commands) = self.current_function {
                commands.push(format!("scoreboard players set {} temp 0", var));
            }
            Some(var)
        } else {
            None
        };

        let condition_execute_args = Self::condition_execute_args(&condition_cmd);
        let mark_branch_taken =
            |commands: &mut Vec<String>, condition: &str, branch_var: &Option<String>| {
                if let Some(var) = branch_var {
                    commands.push(format!(
                        "execute {} run scoreboard players set {} temp 1",
                        condition, var
                    ));
                }
            };

        if let Some(ref mut commands) = self.current_function {
            mark_branch_taken(commands, &condition_execute_args, &branch_var);
        }

        // Capture the branch body first; the number of commands it produced
        // decides between inlining and routing through a helper function.
        // Inlining is only safe for a single command: prefixing several
        // commands with the condition re-evaluates it per command, so an
        // earlier command that changes the condition variables would cut off
        // the rest of the branch.
        let saved_context = self.current_context.clone();
        let capture = self.capture_statements(&if_stmt.then_branch)?;
        self.current_context = saved_context;

        if capture.commands.len() > 1 {
            // Create a separate function for the if branch
            let if_func_name = format!("if_temp_{}", self.next_temp_id());

            let needs_storage = capture.requires_macro_context();
            self.add_captured_function(if_func_name.clone(), capture);

            // Add execute command to call the function
            let call_cmd = self.function_call_command(&if_func_name, needs_storage);
            let return_cmd = self.return_propagation_command();
            if let Some(ref mut commands) = self.current_function {
                commands.push(format!(
                    "execute {} run {}",
                    condition_execute_args, call_cmd
                ));
                if let Some(return_cmd) = return_cmd {
                    commands.push(return_cmd);
                }
            }
        } else {
            // At most one command: inline it under the condition
            self.append_transformed_capture(capture, |cmd| {
                Self::guard_command(&condition_execute_args, cmd)
            })?;
        }

        // Handle elif branches
        for (elif_condition, elif_branch) in &if_stmt.elif_branches {
            let processed_elif_condition = self.preprocess_condition(elif_condition)?;
            let translated = self.translate_condition(&processed_elif_condition)?;
            let elif_condition_cmd = self.lower_translated_condition(translated)?;
            let elif_execute_args = Self::condition_execute_args(&elif_condition_cmd);
            let compound_condition = if let Some(ref var) = branch_var {
                format!("unless score {} temp matches 1 {}", var, elif_execute_args)
            } else {
                elif_execute_args
            };
            let body_condition = if let Some(ref var) = branch_var {
                let elif_taken_var = format!("elif_taken_{}", self.next_temp_id());
                self.data_pack.track_objective("temp");
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("scoreboard players set {} temp 0", elif_taken_var));
                    commands.push(format!(
                        "execute {} run scoreboard players set {} temp 1",
                        compound_condition, elif_taken_var
                    ));
                    commands.push(format!(
                        "execute if score {} temp matches 1 run scoreboard players set {} temp 1",
                        elif_taken_var, var
                    ));
                }
                format!("if score {} temp matches 1", elif_taken_var)
            } else {
                compound_condition.clone()
            };

            // Same capture-first rule as the then branch: inline only a
            // single-command body, otherwise route through a helper function.
            let saved_context = self.current_context.clone();
            let capture = self.capture_statements(elif_branch)?;
            self.current_context = saved_context;

            if capture.commands.len() > 1 {
                let elif_func_name = format!("elif_temp_{}", self.next_temp_id());

                let needs_storage = capture.requires_macro_context();
                self.add_captured_function(elif_func_name.clone(), capture);

                let call_cmd = self.function_call_command(&elif_func_name, needs_storage);
                let return_cmd = self.return_propagation_command();
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("execute {} run {}", body_condition, call_cmd));
                    if let Some(return_cmd) = return_cmd {
                        commands.push(return_cmd);
                    }
                }
            } else {
                self.append_transformed_capture(capture, |cmd| {
                    Self::guard_command(&body_condition, cmd)
                })?;
            }
        }

        // Handle else branch
        if let Some(else_branch) = &if_stmt.else_branch {
            let else_condition = if let Some(ref var) = branch_var {
                format!("unless score {} temp matches 1", var)
            } else {
                format!("unless {}", condition_cmd)
            };

            // Same capture-first rule as the then branch: inline only a
            // single-command body, otherwise route through a helper function.
            let saved_context = self.current_context.clone();
            let capture = self.capture_statements(else_branch)?;
            self.current_context = saved_context;

            if capture.commands.len() > 1 {
                let else_func_name = format!("else_temp_{}", self.next_temp_id());

                let needs_storage = capture.requires_macro_context();
                self.add_captured_function(else_func_name.clone(), capture);

                let call_cmd = self.function_call_command(&else_func_name, needs_storage);
                let return_cmd = self.return_propagation_command();
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("execute {} run {}", else_condition, call_cmd));
                    if let Some(return_cmd) = return_cmd {
                        commands.push(return_cmd);
                    }
                }
            } else {
                self.append_transformed_capture(capture, |cmd| {
                    Self::guard_command(&else_condition, cmd)
                })?;
            }
        }

        Ok(())
    }

    /// Run one generated command under `execute <condition_args>`, merging
    /// an inner `execute` chain. A macro line keeps its `$` at the very front
    /// (`$execute if ... run tellraw $(p) ...`); `execute ... run $tellraw`
    /// would be an unknown command.
    pub(in crate::transpiler) fn guard_command(condition_args: &str, cmd: &str) -> String {
        let (macro_marker, cmd) = match cmd.strip_prefix('$') {
            Some(rest) => ("$", rest),
            None => ("", cmd),
        };
        let clean_cmd = Self::strip_command_prefix(cmd);
        if let Some(inner_parts) = clean_cmd.strip_prefix("execute ") {
            format!("{}execute {} {}", macro_marker, condition_args, inner_parts)
        } else {
            format!(
                "{}execute {} run {}",
                macro_marker, condition_args, clean_cmd
            )
        }
    }

    pub(in crate::transpiler) fn condition_execute_args(condition_cmd: &str) -> String {
        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
            condition_cmd.to_string()
        } else {
            format!("if {}", condition_cmd)
        }
    }
}
