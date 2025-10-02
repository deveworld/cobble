use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_for(
        &mut self,
        for_loop: &ForLoop,
    ) -> Result<(), String> {
        let mut for_commands = Vec::new();

        // Handle range-based for loops
        if let Expression::Call(func, args) = &for_loop.iter {
            if let Expression::Identifier(name) = &**func {
                if name == "range" && args.len() == 1 {
                    if let Expression::Number(n) = &args[0] {
                        let count = *n as i32;

                        // Determine step value (default 1, or from for_loop.step)
                        let step = if let Some(ref step_expr) = for_loop.step {
                            match step_expr {
                                Expression::Number(s) => *s as i32,
                                _ => return Err("Step must be a constant number".to_string()),
                            }
                        } else {
                            1
                        };

                        if step == 0 {
                            return Err("Step cannot be zero".to_string());
                        }

                        // Generate a helper function for the loop
                        let loop_func_name = format!("loop_temp_{}", self.temp_counter);
                        self.temp_counter += 1;

                        // Track loop_counter objective
                        self.data_pack.track_objective("loop_counter");

                        // Track this variable's objective AND mark as scoreboard variable
                        self.variable_objectives
                            .insert(for_loop.target.clone(), "loop_counter".to_string());
                        self.scoreboard_variables.insert(for_loop.target.clone());

                        // Initialize loop counter based on step direction
                        let start_value = if step > 0 {
                            0
                        } else {
                            // For negative step, start at count + step (e.g., count - 1 for step = -1)
                            count + step
                        };

                        for_commands.push(format!(
                            "scoreboard players set {} loop_counter {}",
                            for_loop.target, start_value
                        ));

                        // Create loop function with body inside
                        let mut loop_commands = vec![];

                        // Process loop body and add commands to the loop function
                        let old_function = self.current_function.take();
                        let saved_context = self.current_context.clone();

                        self.current_function = Some(Vec::new());
                        for stmt in &for_loop.body {
                            self.process_statement(stmt)?;
                        }
                        if let Some(body_commands) = self.current_function.take() {
                            // Add body commands FIRST, before increment
                            loop_commands.extend(body_commands);
                        }

                        self.current_function = old_function;
                        self.current_context = saved_context;

                        // THEN add increment/decrement and recursive call
                        if step > 0 {
                            loop_commands.push(format!(
                                "scoreboard players add {} loop_counter {}",
                                for_loop.target, step
                            ));
                        } else {
                            // Use 'remove' for negative step (Java Edition compatibility)
                            loop_commands.push(format!(
                                "scoreboard players remove {} loop_counter {}",
                                for_loop.target, step.abs()
                            ));
                        }

                        // Condition depends on step direction
                        let condition = if step > 0 {
                            // For positive step: continue while i < count
                            format!("..{}", count - 1)
                        } else {
                            // For negative step: continue while i >= 0
                            "0..".to_string()
                        };

                        loop_commands.push(format!(
                            "execute if score {} loop_counter matches {} run function {}:{}",
                            for_loop.target,
                            condition,
                            self.data_pack.namespace,
                            loop_func_name
                        ));

                        // Add the loop function to the data pack
                        self.data_pack
                            .add_function(loop_func_name.clone(), loop_commands);

                        // Start the loop in the main function
                        for_commands.push(format!(
                            "function {}:{}",
                            self.data_pack.namespace, loop_func_name
                        ));
                    }
                }
            }
        } else {
            // Generic for loop comment
            for_commands.push(format!("# FOR loop: {} in ...", for_loop.target));
            let saved_function = self.current_function.take();
            for stmt in &for_loop.body {
                self.current_function = Some(Vec::new());
                self.process_statement(stmt)?;
                if let Some(body_cmds) = self.current_function.take() {
                    for_commands.extend(body_cmds);
                }
            }
            self.current_function = saved_function;
        }

        if let Some(ref mut commands) = self.current_function {
            commands.extend(for_commands);
        }

        Ok(())
    }

    pub(in crate::transpiler) fn process_while(
        &mut self,
        while_loop: &WhileLoop,
    ) -> Result<(), String> {
        let mut while_commands = Vec::new();

        // WARNING: While loops execute all iterations in a single tick
        // This can cause server lag with large iteration counts (>100)
        // Future improvement: Add schedule command support for tick-based iteration

        // Generate a recursive function for the while loop
        let loop_func_name = format!("while_temp_{}", self.temp_counter);
        self.temp_counter += 1;

        // Create while loop function
        let mut loop_commands = vec![];

        // Add condition check and body execution
        let condition_cmd = self.translate_condition(&while_loop.condition)?;

        // Process loop body
        let old_function = self.current_function.take();
        let saved_context = self.current_context.clone();

        self.current_function = Some(Vec::new());
        for stmt in &while_loop.body {
            self.process_statement(stmt)?;
        }
        if let Some(body_commands) = self.current_function.take() {
            for cmd in body_commands {
                // Strip any leading slash before adding to execute
                let clean_cmd = Self::strip_command_prefix(&cmd);
                // Check if condition already has "if" or "unless" prefix
                if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
                    loop_commands.push(format!("execute {} run {}", condition_cmd, clean_cmd));
                } else {
                    loop_commands.push(format!("execute if {} run {}", condition_cmd, clean_cmd));
                }
            }
        }

        self.current_function = old_function;
        self.current_context = saved_context;

        // Add recursive call
        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
            loop_commands.push(format!(
                "execute {} run function {}:{}",
                condition_cmd, self.data_pack.namespace, loop_func_name
            ));
        } else {
            loop_commands.push(format!(
                "execute if {} run function {}:{}",
                condition_cmd, self.data_pack.namespace, loop_func_name
            ));
        }

        // Add the loop function to the data pack
        self.data_pack
            .add_function(loop_func_name.clone(), loop_commands);

        // Start the loop
        while_commands.push(format!(
            "function {}:{}",
            self.data_pack.namespace, loop_func_name
        ));

        if let Some(ref mut commands) = self.current_function {
            commands.extend(while_commands);
        }

        Ok(())
    }
}
