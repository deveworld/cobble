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

                        // Create a macro function for the loop body
                        // This allows loop variables to be used in commands like /say
                        let body_func_name = format!("loop_body_{}", self.temp_counter - 1);

                        // Process loop body as a macro function with loop variable as parameter
                        let old_function = self.current_function.take();
                        let saved_context = self.current_context.clone();

                        // Set up function context with loop variable as a parameter
                        self.current_context = crate::transpiler::FunctionContext::with_params(vec![for_loop.target.clone()]);
                        self.current_function = Some(Vec::new());

                        for stmt in &for_loop.body {
                            self.process_statement(stmt)?;
                        }

                        if let Some(body_commands) = self.current_function.take() {
                            // Store as a macro function
                            self.data_pack.add_function(body_func_name.clone(), body_commands);
                            // Track this function as having parameters
                            self.function_params.insert(body_func_name.clone(), vec![for_loop.target.clone()]);
                        }

                        self.current_function = old_function;
                        self.current_context = saved_context;

                        // Create loop control function
                        let mut loop_commands = vec![];

                        // Store loop variable value into storage for macro function
                        loop_commands.push(format!(
                            "execute store result storage {}:global args.{} int 1 run scoreboard players get {} loop_counter",
                            self.data_pack.namespace, for_loop.target, for_loop.target
                        ));

                        // Call the macro body function with the loop variable
                        loop_commands.push(format!(
                            "function {}:{} with storage {}:global args",
                            self.data_pack.namespace, body_func_name, self.data_pack.namespace
                        ));

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
            // Unsupported iterator type - provide clear error message
            return Err(format!(
                "For loops only support range() iterator.\n\
                 Syntax: for {} in range(N):\n\
                 \n\
                 Examples:\n\
                 - for i in range(10):       # Loop 10 times (0..9)\n\
                 - for i in range(10) by 2:  # Count by 2s\n\
                 - for i in range(10) by -1: # Count backwards\n\
                 \n\
                 Iterating over lists/arrays is not yet supported.",
                for_loop.target
            ));
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

        // Check for obvious infinite loops
        if let Expression::Boolean(true) = &while_loop.condition {
            eprintln!(
                "⚠️  Warning: Infinite loop detected (while True). \n\
                    This will run forever and freeze Minecraft!\n\
                    Consider using a condition that can become false."
            );
        }

        // Generate a recursive function for the while loop
        let loop_func_name = format!("while_temp_{}", self.temp_counter);
        self.temp_counter += 1;

        // Create while loop function
        let mut loop_commands = vec![];

        // Add condition check and body execution
        let processed_condition = self.preprocess_condition(&while_loop.condition)?;
        let condition_cmd = self.translate_condition(&processed_condition)?;

        // IMPORTANT: We need to wrap the body in a conditional function call
        // to prevent bugs where body statements modify condition variables.
        // The condition should be evaluated ONCE per iteration, not per statement.

        // Create inner body function that executes unconditionally
        let body_func_name = format!("while_body_{}", self.temp_counter);
        self.temp_counter += 1;

        // Process loop body into the body function
        let old_function = self.current_function.take();
        let saved_context = self.current_context.clone();

        self.current_function = Some(Vec::new());
        for stmt in &while_loop.body {
            self.process_statement(stmt)?;
        }

        if let Some(body_commands) = self.current_function.take() {
            // Add body function to data pack
            self.data_pack.add_function(body_func_name.clone(), body_commands);
        }

        self.current_function = old_function;
        self.current_context = saved_context;

        // In the while loop function, check condition once and call body
        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
            loop_commands.push(format!(
                "execute {} run function {}:{}",
                condition_cmd, self.data_pack.namespace, body_func_name
            ));
        } else {
            loop_commands.push(format!(
                "execute if {} run function {}:{}",
                condition_cmd, self.data_pack.namespace, body_func_name
            ));
        }

        // Add recursive call (check condition again after body execution)
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
