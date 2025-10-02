use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_if(
        &mut self,
        if_stmt: &IfStatement,
    ) -> Result<(), String> {
        // Preprocess condition to handle complex expressions
        let processed_condition = self.preprocess_condition(&if_stmt.condition)?;

        let mut condition_cmd = self.translate_condition(&processed_condition)?;

        // Handle OR conditions specially
        if condition_cmd.starts_with("OR(") {
            self.data_pack.track_objective("or_result");
            condition_cmd = self.handle_or_condition(&condition_cmd)?;
        }

        // Check if we need to create a separate function for complex if statements
        // If the then_branch has multiple statements or nested control flow, use a function
        let needs_function = if_stmt.then_branch.len() > 3
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
            let saved_function = self.current_function.take();
            let saved_context = self.current_context.clone();

            self.current_function = Some(Vec::new());
            for stmt in &if_stmt.then_branch {
                self.process_statement(stmt)?;
            }

            if let Some(if_commands) = self.current_function.take() {
                self.data_pack
                    .add_function(if_func_name.clone(), if_commands);
            }

            self.current_function = saved_function;
            self.current_context = saved_context;

            // Add execute command to call the function
            if let Some(ref mut commands) = self.current_function {
                // condition_cmd may already start with "if" or "unless" (from And/Not operators)
                if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
                    commands.push(format!(
                        "execute {} run function {}:{}",
                        condition_cmd, self.data_pack.namespace, if_func_name
                    ));
                } else {
                    commands.push(format!(
                        "execute if {} run function {}:{}",
                        condition_cmd, self.data_pack.namespace, if_func_name
                    ));
                }
            }
        } else {
            // For simple if statements, inline the commands
            let mut if_commands = Vec::new();

            let saved_function = self.current_function.take();
            for stmt in &if_stmt.then_branch {
                self.current_function = Some(Vec::new());
                self.process_statement(stmt)?;

                if let Some(stmt_commands) = self.current_function.take() {
                    for cmd in stmt_commands {
                        // Strip any leading slash from the command before adding to execute
                        let clean_cmd = Self::strip_command_prefix(&cmd);
                        // condition_cmd may already start with "if" or "unless" (from And/Not operators)
                        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
                            if_commands
                                .push(format!("execute {} run {}", condition_cmd, clean_cmd));
                        } else {
                            if_commands
                                .push(format!("execute if {} run {}", condition_cmd, clean_cmd));
                        }
                    }
                }
            }
            self.current_function = saved_function;

            if let Some(ref mut commands) = self.current_function {
                commands.extend(if_commands);
            }
        }

        // Handle elif branches
        let mut previous_conditions = vec![condition_cmd.clone()];

        for (elif_condition, elif_branch) in &if_stmt.elif_branches {
            let processed_elif_condition = self.preprocess_condition(elif_condition)?;
            let elif_condition_cmd = self.translate_condition(&processed_elif_condition)?;

            // Build the compound condition: unless (all previous conditions) if (current condition)
            let mut compound_condition = String::new();
            for prev_cond in &previous_conditions {
                // Check if condition already starts with "unless"
                if let Some(inner) = prev_cond.strip_prefix("unless ") {
                    // Already negated - just add "if" to re-negate (double negative)
                    // Skip "unless "
                    compound_condition.push_str(&format!("if {} ", inner));
                } else {
                    compound_condition.push_str(&format!("unless {} ", prev_cond));
                }
            }
            // Add the elif condition
            if elif_condition_cmd.starts_with("unless ") {
                compound_condition.push_str(&elif_condition_cmd);
            } else {
                compound_condition.push_str(&format!("if {}", elif_condition_cmd));
            }

            // Check if elif needs a function
            let elif_needs_function = elif_branch.len() > 3
                || elif_branch.iter().any(|stmt| {
                    matches!(
                        stmt,
                        Statement::If(_) | Statement::For(_) | Statement::While(_)
                    )
                });

            if elif_needs_function {
                let elif_func_name = format!("elif_temp_{}", self.temp_counter);
                self.temp_counter += 1;

                let saved_function = self.current_function.take();
                let saved_context = self.current_context.clone();

                self.current_function = Some(Vec::new());
                for stmt in elif_branch {
                    self.process_statement(stmt)?;
                }

                if let Some(elif_commands) = self.current_function.take() {
                    self.data_pack
                        .add_function(elif_func_name.clone(), elif_commands);
                }

                self.current_function = saved_function;
                self.current_context = saved_context;

                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!(
                        "execute {} run function {}:{}",
                        compound_condition, self.data_pack.namespace, elif_func_name
                    ));
                }
            } else {
                let mut elif_commands = Vec::new();

                let saved_function = self.current_function.take();
                for stmt in elif_branch {
                    self.current_function = Some(Vec::new());
                    self.process_statement(stmt)?;

                    if let Some(stmt_commands) = self.current_function.take() {
                        for cmd in stmt_commands {
                            let clean_cmd = Self::strip_command_prefix(&cmd);
                            elif_commands
                                .push(format!("execute {} run {}", compound_condition, clean_cmd));
                        }
                    }
                }
                self.current_function = saved_function;

                if let Some(ref mut commands) = self.current_function {
                    commands.extend(elif_commands);
                }
            }

            previous_conditions.push(elif_condition_cmd);
        }

        // Handle else branch
        if let Some(else_branch) = &if_stmt.else_branch {
            // Build condition: unless (all previous conditions)
            let mut else_condition = String::new();
            for prev_cond in &previous_conditions {
                // Check if condition already starts with "unless"
                if let Some(inner) = prev_cond.strip_prefix("unless ") {
                    // Already negated - use "if" to re-negate (double negative)
                    // Skip "unless "
                    else_condition.push_str(&format!("if {} ", inner));
                } else {
                    else_condition.push_str(&format!("unless {} ", prev_cond));
                }
            }
            else_condition = else_condition.trim_end().to_string();

            let else_needs_function = else_branch.len() > 3
                || else_branch.iter().any(|stmt| {
                    matches!(
                        stmt,
                        Statement::If(_) | Statement::For(_) | Statement::While(_)
                    )
                });

            if else_needs_function {
                let else_func_name = format!("else_temp_{}", self.temp_counter);
                self.temp_counter += 1;

                let saved_function = self.current_function.take();
                let saved_context = self.current_context.clone();

                self.current_function = Some(Vec::new());
                for stmt in else_branch {
                    self.process_statement(stmt)?;
                }

                if let Some(else_commands) = self.current_function.take() {
                    self.data_pack
                        .add_function(else_func_name.clone(), else_commands);
                }

                self.current_function = saved_function;
                self.current_context = saved_context;

                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!(
                        "execute {} run function {}:{}",
                        else_condition, self.data_pack.namespace, else_func_name
                    ));
                }
            } else {
                let mut else_commands = Vec::new();

                let saved_function = self.current_function.take();
                for stmt in else_branch {
                    self.current_function = Some(Vec::new());
                    self.process_statement(stmt)?;

                    if let Some(stmt_commands) = self.current_function.take() {
                        for cmd in stmt_commands {
                            let clean_cmd = Self::strip_command_prefix(&cmd);
                            else_commands
                                .push(format!("execute {} run {}", else_condition, clean_cmd));
                        }
                    }
                }
                self.current_function = saved_function;

                if let Some(ref mut commands) = self.current_function {
                    commands.extend(else_commands);
                }
            }
        }

        Ok(())
    }
}
