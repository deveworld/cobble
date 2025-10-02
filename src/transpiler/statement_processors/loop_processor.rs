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
                        // Generate a helper function for the loop
                        let loop_func_name = format!("loop_temp_{}", self.temp_counter);
                        self.temp_counter += 1;

                        // Track loop_counter objective
                        self.data_pack.track_objective("loop_counter");

                        // Track this variable's objective AND mark as scoreboard variable
                        self.variable_objectives
                            .insert(for_loop.target.clone(), "loop_counter".to_string());
                        self.scoreboard_variables.insert(for_loop.target.clone());

                        // Initialize loop counter
                        for_commands.push(format!(
                            "scoreboard players set {} loop_counter 0",
                            for_loop.target
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

                        // THEN add increment and recursive call
                        loop_commands.push(format!(
                            "scoreboard players add {} loop_counter 1",
                            for_loop.target
                        ));
                        loop_commands.push(format!(
                            "execute if score {} loop_counter matches ..{} run function {}:{}",
                            for_loop.target,
                            count - 1,
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
