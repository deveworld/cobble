use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_if(
        &mut self,
        if_stmt: &IfStatement,
    ) -> Result<(), String> {
        // Preprocess condition to handle complex expressions
        let processed_condition = self.preprocess_condition(&if_stmt.condition)?;

        let condition_cmd =
            self.normalize_if_condition(self.translate_condition(&processed_condition)?)?;

        let branch_var = if !if_stmt.elif_branches.is_empty() || if_stmt.else_branch.is_some() {
            let var = format!("if_branch_{}", self.temp_counter);
            self.temp_counter += 1;
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

        // Check if we need to create a separate function for complex if statements
        // If the then_branch has multiple statements or nested control flow, use a function
        // IMPORTANT: We use > 1 (not > 3) to prevent bugs where earlier statements modify
        // the condition variables, causing later statements to not execute
        let needs_function = if_stmt.then_branch.len() > 1
            || if_stmt.then_branch.iter().any(|stmt| {
                matches!(
                    stmt,
                    Statement::If(_) | Statement::For(_) | Statement::While(_)
                )
            });

        if needs_function {
            // Create a separate function for the if branch
            let if_func_name = format!("if_temp_{}", self.temp_counter);
            self.temp_counter += 1;

            // Process then branch in a new function
            let saved_context = self.current_context.clone();
            let capture = self.capture_statements(&if_stmt.then_branch)?;
            let needs_storage = capture.requires_macro_context();
            self.add_captured_function(if_func_name.clone(), capture);
            self.current_context = saved_context;

            // Add execute command to call the function
            let call_cmd = self.function_call_command(&if_func_name, needs_storage);
            if let Some(ref mut commands) = self.current_function {
                commands.push(format!(
                    "execute {} run {}",
                    condition_execute_args, call_cmd
                ));
            }
        } else {
            // For simple if statements, inline the commands
            for stmt in &if_stmt.then_branch {
                let capture = self.capture_statement(stmt)?;
                self.append_transformed_capture(capture, |cmd| {
                    let clean_cmd = Self::strip_command_prefix(cmd);
                    let execute_prefix = format!("execute {}", condition_execute_args);
                    if let Some(inner_parts) = clean_cmd.strip_prefix("execute ") {
                        format!("{} {}", execute_prefix, inner_parts)
                    } else {
                        format!("{} run {}", execute_prefix, clean_cmd)
                    }
                })?;
            }
        }

        // Handle elif branches
        for (elif_condition, elif_branch) in &if_stmt.elif_branches {
            let processed_elif_condition = self.preprocess_condition(elif_condition)?;
            let elif_condition_cmd =
                self.normalize_if_condition(self.translate_condition(&processed_elif_condition)?)?;
            let elif_execute_args = Self::condition_execute_args(&elif_condition_cmd);
            let compound_condition = if let Some(ref var) = branch_var {
                format!("unless score {} temp matches 1 {}", var, elif_execute_args)
            } else {
                elif_execute_args
            };
            let body_condition = if let Some(ref var) = branch_var {
                let elif_taken_var = format!("elif_taken_{}", self.temp_counter);
                self.temp_counter += 1;
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

            // Check if elif needs a function
            let elif_needs_function = elif_branch.len() > 1
                || elif_branch.iter().any(|stmt| {
                    matches!(
                        stmt,
                        Statement::If(_) | Statement::For(_) | Statement::While(_)
                    )
                });

            if elif_needs_function {
                let elif_func_name = format!("elif_temp_{}", self.temp_counter);
                self.temp_counter += 1;

                let saved_context = self.current_context.clone();
                let capture = self.capture_statements(elif_branch)?;
                let needs_storage = capture.requires_macro_context();
                self.add_captured_function(elif_func_name.clone(), capture);
                self.current_context = saved_context;

                let call_cmd = self.function_call_command(&elif_func_name, needs_storage);
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("execute {} run {}", body_condition, call_cmd));
                }
            } else {
                for stmt in elif_branch {
                    let capture = self.capture_statement(stmt)?;
                    self.append_transformed_capture(capture, |cmd| {
                        let clean_cmd = Self::strip_command_prefix(cmd);
                        if let Some(inner_parts) = clean_cmd.strip_prefix("execute ") {
                            format!("execute {} {}", body_condition, inner_parts)
                        } else {
                            format!("execute {} run {}", body_condition, clean_cmd)
                        }
                    })?;
                }
            }
        }

        // Handle else branch
        if let Some(else_branch) = &if_stmt.else_branch {
            let else_condition = if let Some(ref var) = branch_var {
                format!("unless score {} temp matches 1", var)
            } else {
                format!("unless {}", condition_cmd)
            };

            let else_needs_function = else_branch.len() > 1
                || else_branch.iter().any(|stmt| {
                    matches!(
                        stmt,
                        Statement::If(_) | Statement::For(_) | Statement::While(_)
                    )
                });

            if else_needs_function {
                let else_func_name = format!("else_temp_{}", self.temp_counter);
                self.temp_counter += 1;

                let saved_context = self.current_context.clone();
                let capture = self.capture_statements(else_branch)?;
                let needs_storage = capture.requires_macro_context();
                self.add_captured_function(else_func_name.clone(), capture);
                self.current_context = saved_context;

                let call_cmd = self.function_call_command(&else_func_name, needs_storage);
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("execute {} run {}", else_condition, call_cmd));
                }
            } else {
                for stmt in else_branch {
                    let capture = self.capture_statement(stmt)?;
                    self.append_transformed_capture(capture, |cmd| {
                        let clean_cmd = Self::strip_command_prefix(cmd);
                        if let Some(inner_parts) = clean_cmd.strip_prefix("execute ") {
                            format!("execute {} {}", else_condition, inner_parts)
                        } else {
                            format!("execute {} run {}", else_condition, clean_cmd)
                        }
                    })?;
                }
            }
        }

        Ok(())
    }

    pub(in crate::transpiler) fn normalize_if_condition(
        &mut self,
        mut condition_cmd: String,
    ) -> Result<String, String> {
        // Handle OR conditions specially
        let is_negated = condition_cmd.starts_with("NOT_");
        let clean_cmd = if is_negated {
            &condition_cmd[4..] // Remove "NOT_" prefix
        } else {
            &condition_cmd
        };

        if clean_cmd.starts_with("OR(") || clean_cmd.starts_with("OR_AND(") {
            condition_cmd = self.handle_or_and_condition(clean_cmd)?;

            // Apply NOT if needed
            if is_negated {
                condition_cmd = if condition_cmd.starts_with("score ") {
                    format!("unless {}", condition_cmd)
                } else {
                    condition_cmd.replace("if ", "unless ")
                };
            }
        }

        Ok(condition_cmd)
    }

    pub(in crate::transpiler) fn condition_execute_args(condition_cmd: &str) -> String {
        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
            condition_cmd.to_string()
        } else {
            format!("if {}", condition_cmd)
        }
    }
}
