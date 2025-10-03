use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_match(
        &mut self,
        match_stmt: &MatchStatement,
    ) -> Result<(), String> {
        // Create a temporary variable to hold the match value
        let temp_var = format!("match_temp_{}", self.temp_counter);
        self.temp_counter += 1;

        // Evaluate the match expression into the temporary variable
        self.data_pack.track_objective("temp");
        match &match_stmt.value {
            Expression::Identifier(name) => {
                // If it's already a variable, just use it
                let obj = self.variable_objectives.get(name).unwrap_or(&"temp".to_string()).clone();
                let (result_var, result_obj) = (name.as_str(), obj.as_str());
                return self.process_match_with_var(result_var, result_obj, match_stmt);
            }
            Expression::Number(n) => {
                // If it's a constant, assign it to temp variable
                let value = *n as i32;
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!("scoreboard players set {} temp {}", temp_var, value));
                }
            }
            _ => {
                // Complex expression - evaluate it first
                let commands = self.evaluate_expression_to_target(&match_stmt.value, &temp_var)?;
                if let Some(ref mut cmds) = self.current_function {
                    cmds.extend(commands);
                }
            }
        }

        self.process_match_with_var(&temp_var, "temp", match_stmt)
    }

    fn process_match_with_var(
        &mut self,
        result_var: &str,
        result_obj: &str,
        match_stmt: &MatchStatement,
    ) -> Result<(), String> {

        // Convert cases to (min, max, body_idx) tuples
        let mut cases: Vec<(i32, i32, usize)> = Vec::new();
        let mut wildcard_case: Option<usize> = None;

        for (idx, case) in match_stmt.cases.iter().enumerate() {
            match &case.pattern {
                MatchPattern::Literal(val) => {
                    cases.push((*val, *val, idx));
                }
                MatchPattern::Range(start, end) => {
                    cases.push((*start, *end, idx));
                }
                MatchPattern::Wildcard => {
                    // Wildcard case handled separately
                    wildcard_case = Some(idx);
                }
            }
        }

        // Sort cases by min value for the 4-way split algorithm
        cases.sort_by_key(|(min, _, _)| *min);

        // Generate switch tree using 4-way split algorithm
        if !cases.is_empty() {
            self.generate_switch_tree(result_var, result_obj, &cases, match_stmt)?;
        }

        // Handle wildcard case if present (executes if no other case matched)
        if let Some(wildcard_idx) = wildcard_case {
            let case = &match_stmt.cases[wildcard_idx];

            // Generate condition that excludes all ranges
            let mut unless_conditions = Vec::new();
            for (min, max, _) in &cases {
                unless_conditions.push(format!("unless score {} {} matches {}..{}",
                    result_var, result_obj, min, max));
            }

            if case.body.len() == 1 {
                // Inline single statement with condition
                let saved_function = self.current_function.take();
                self.current_function = Some(Vec::new());
                self.process_statement(&case.body[0])?;

                if let Some(stmt_commands) = self.current_function.take() {
                    self.current_function = saved_function;

                    if let Some(ref mut commands) = self.current_function {
                        for cmd in stmt_commands {
                            let clean_cmd = Self::strip_command_prefix(&cmd);
                            if unless_conditions.is_empty() {
                                commands.push(clean_cmd);
                            } else {
                                commands.push(format!(
                                    "execute {} run {}",
                                    unless_conditions.join(" "),
                                    clean_cmd
                                ));
                            }
                        }
                    }
                } else {
                    self.current_function = saved_function;
                }
            } else {
                // Create function for multi-statement wildcard
                let func_name = format!("match_default_{}", self.temp_counter);
                self.temp_counter += 1;

                self.create_match_case_function(&func_name, &case.body)?;

                // Call function only if no other case matched
                let call_cmd = format!("function {}:{}", self.data_pack.namespace, func_name);
                if let Some(ref mut commands) = self.current_function {
                    if unless_conditions.is_empty() {
                        commands.push(call_cmd);
                    } else {
                        // Chain all unless conditions in a single execute command
                        commands.push(format!(
                            "execute {} run {}",
                            unless_conditions.join(" "),
                            call_cmd
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Generate switch tree using 4-way split algorithm (CBScript algorithm)
    fn generate_switch_tree(
        &mut self,
        var: &str,
        obj: &str,
        cases: &[(i32, i32, usize)],
        match_stmt: &MatchStatement,
    ) -> Result<(), String> {
        if cases.is_empty() {
            return Ok(());
        }

        // Base case: single case
        if cases.len() == 1 {
            let (min, max, case_idx) = cases[0];
            let case = &match_stmt.cases[case_idx];

            if case.body.len() == 1 {
                // Inline single command
                if self.current_function.is_some() {
                    let range = if min == max {
                        format!("{}", min)
                    } else {
                        format!("{}..{}", min, max)
                    };

                    // Save and restore function context for processing the statement
                    let saved_commands = self.current_function.take();
                    self.current_function = Some(Vec::new());

                    for stmt in &case.body {
                        self.process_statement(stmt)?;
                    }

                    let body_commands = self.current_function.take().unwrap();
                    self.current_function = saved_commands;

                    for cmd in body_commands {
                        let exec_cmd = format!("execute if score {} {} matches {} run {}", var, obj, range, cmd);
                        if let Some(ref mut cmds) = self.current_function {
                            cmds.push(exec_cmd);
                        }
                    }
                }
            } else {
                // Create function for multi-statement case
                let func_name = format!("match_case_{}_{}", min, self.temp_counter);
                self.temp_counter += 1;

                self.create_match_case_function(&func_name, &case.body)?;

                if let Some(ref mut commands) = self.current_function {
                    let range = if min == max {
                        format!("{}", min)
                    } else {
                        format!("{}..{}", min, max)
                    };
                    let call_cmd = format!(
                        "execute if score {} {} matches {} run function {}:{}",
                        var, obj, range, self.data_pack.namespace, func_name
                    );
                    commands.push(call_cmd);
                }
            }
            return Ok(());
        }

        // Recursive case: split into 4 quarters
        for q in 0..4 {
            let imin = q * cases.len() / 4;
            let imax = (q + 1) * cases.len() / 4;

            if imin >= imax {
                continue;
            }

            let sub_cases = &cases[imin..imax];
            let vmin = sub_cases[0].0;
            let vmax = sub_cases[sub_cases.len() - 1].1;

            if sub_cases.len() == 1 {
                // Single case in this quarter - process directly
                self.generate_switch_tree(var, obj, sub_cases, match_stmt)?;
            } else {
                // Multiple cases - create a function and recursively process
                let func_name = format!("match_switch_{}_{}", vmin, self.temp_counter);
                self.temp_counter += 1;

                // Save current function context
                let saved_commands = self.current_function.take();
                self.current_function = Some(Vec::new());

                // Recursively generate the switch tree in the new function
                self.generate_switch_tree(var, obj, sub_cases, match_stmt)?;

                // Get the generated commands
                let func_commands = self.current_function.take().unwrap();

                // Restore previous function context
                self.current_function = saved_commands;

                // Add the generated function to the data pack
                self.data_pack
                    .add_function(func_name.clone(), func_commands);

                // Call the function with range condition
                if let Some(ref mut commands) = self.current_function {
                    let call_cmd = format!(
                        "execute if score {} {} matches {}..{} run function {}:{}",
                        var, obj, vmin, vmax, self.data_pack.namespace, func_name
                    );
                    commands.push(call_cmd);
                }
            }
        }

        Ok(())
    }

    /// Create a function for a match case body
    fn create_match_case_function(
        &mut self,
        func_name: &str,
        body: &[Statement],
    ) -> Result<(), String> {
        // Save current function context
        let saved_commands = self.current_function.take();

        // Create new function
        self.current_function = Some(Vec::new());

        // Process body statements
        for stmt in body {
            self.process_statement(stmt)?;
        }

        // Get the generated commands
        let func_commands = self.current_function.take().unwrap();

        // Restore previous function context
        self.current_function = saved_commands;

        // Add function to data pack
        self.data_pack
            .add_function(func_name.to_string(), func_commands);

        Ok(())
    }

}
