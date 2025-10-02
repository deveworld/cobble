use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), String> {
        // Store the variable value for later use
        self.variables
            .insert(assign.target.clone(), assign.value.clone());

        // If we're not in a function, store as module-level variable
        // These will be automatically initialized in the _cobble_init function
        if self.current_function.is_none() {
            self.module_level_vars
                .insert(assign.target.clone(), assign.value.clone());
            return Ok(());
        }

        // Check if we need to use the complex expression evaluator
        // Do this before borrowing to avoid borrow checker issues
        let needs_complex_eval = if let Expression::Binary(left, _, right) = &assign.value {
            // Check if either side is a binary expression (nested)
            matches!(**left, Expression::Binary(_, _, _))
                || matches!(**right, Expression::Binary(_, _, _))
        } else {
            false
        };

        if needs_complex_eval {
            // Handle complex nested expressions
            self.data_pack.track_objective("temp");
            self.variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            let expr_commands =
                self.evaluate_expression_to_target(&assign.value, &assign.target)?;

            if let Some(ref mut commands) = self.current_function {
                commands.extend(expr_commands);
            }
            return Ok(());
        }

        // If it's a score assignment, generate scoreboard command
        if let Some(ref mut commands) = self.current_function {
            match &assign.value {
                Expression::Number(n) => {
                    // Direct number assignment
                    self.data_pack.track_objective("temp");
                    self.variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.scoreboard_variables.insert(assign.target.clone());
                    let score = *n as i32;
                    commands.push(format!(
                        "scoreboard players set {} temp {}",
                        assign.target, score
                    ));
                }
                Expression::Identifier(var) => {
                    // Variable-to-variable assignment
                    self.data_pack.track_objective("temp");
                    self.variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.scoreboard_variables.insert(assign.target.clone());

                    let var_obj = self
                        .variable_objectives
                        .get(var)
                        .unwrap_or(&"temp".to_string())
                        .clone();

                    commands.push(format!(
                        "scoreboard players operation {} temp = {} {}",
                        assign.target, var, var_obj
                    ));
                }
                Expression::Binary(left, op, right) => {
                    // Binary operation (already handled complex case above, so this is simple)
                    self.data_pack.track_objective("temp");
                    self.variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.scoreboard_variables.insert(assign.target.clone());

                    match (&**left, &**right) {
                        (Expression::Identifier(var), Expression::Number(n)) => {
                            // Handle variable op number (e.g., score = x + 5)
                            let value = *n as i32;
                            let var_obj = self
                                .variable_objectives
                                .get(var)
                                .unwrap_or(&"temp".to_string())
                                .clone();

                            match op {
                                BinaryOp::Add => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players add {} temp {}",
                                        assign.target, value
                                    ));
                                }
                                BinaryOp::Sub => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players remove {} temp {}",
                                        assign.target, value
                                    ));
                                }
                                BinaryOp::Mul => {
                                    self.data_pack.track_objective("multiplier");
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players set multiplier temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp *= multiplier temp",
                                        assign.target
                                    ));
                                }
                                BinaryOp::Div => {
                                    self.data_pack.track_objective("divisor");
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players set divisor temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= divisor temp",
                                        assign.target
                                    ));
                                }
                                _ => {}
                            }
                        }
                        (Expression::Number(n1), Expression::Number(n2)) => {
                            // Constant expression evaluation
                            let result = match op {
                                BinaryOp::Add => (*n1 + *n2) as i32,
                                BinaryOp::Sub => (*n1 - *n2) as i32,
                                BinaryOp::Mul => (*n1 * *n2) as i32,
                                BinaryOp::Div => (*n1 / *n2) as i32,
                                _ => 0,
                            };
                            commands.push(format!(
                                "scoreboard players set {} temp {}",
                                assign.target, result
                            ));
                        }
                        (Expression::Number(n), Expression::Identifier(var)) => {
                            // Handle Number + Variable (e.g., score = 5 + x)
                            self.data_pack.track_objective("temp");
                            // Mark target as scoreboard-backed
                            self.scoreboard_variables.insert(assign.target.clone());
                            let value = *n as i32;
                            let var_obj = self
                                .variable_objectives
                                .get(var)
                                .unwrap_or(&"temp".to_string())
                                .clone();

                            match op {
                                BinaryOp::Add => {
                                    // score = value + var → score = var, score += value
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players add {} temp {}",
                                        assign.target, value
                                    ));
                                }
                                BinaryOp::Sub => {
                                    // score = value - var → score = value, score -= var
                                    commands.push(format!(
                                        "scoreboard players set {} temp {}",
                                        assign.target, value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp -= {} {}",
                                        assign.target, var, var_obj
                                    ));
                                }
                                BinaryOp::Mul => {
                                    // score = value * var → score = var, score *= value
                                    self.data_pack.track_objective("multiplier");
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players set multiplier temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp *= multiplier temp",
                                        assign.target
                                    ));
                                }
                                BinaryOp::Div => {
                                    // score = value / var (not commonly used, but implemented)
                                    self.data_pack.track_objective("divisor");
                                    commands.push(format!(
                                        "scoreboard players set {} temp {}",
                                        assign.target, value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= {} {}",
                                        assign.target, var, var_obj
                                    ));
                                }
                                _ => {}
                            }
                        }
                        (Expression::Identifier(var1), Expression::Identifier(var2)) => {
                            // Handle variable op variable (e.g., z = x * y)
                            let var1_obj = self
                                .variable_objectives
                                .get(var1)
                                .unwrap_or(&"temp".to_string())
                                .clone();
                            let var2_obj = self
                                .variable_objectives
                                .get(var2)
                                .unwrap_or(&"temp".to_string())
                                .clone();

                            // First assign var1 to target
                            commands.push(format!(
                                "scoreboard players operation {} temp = {} {}",
                                assign.target, var1, var1_obj
                            ));

                            // Then apply operation with var2
                            match op {
                                BinaryOp::Add => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp += {} {}",
                                        assign.target, var2, var2_obj
                                    ));
                                }
                                BinaryOp::Sub => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp -= {} {}",
                                        assign.target, var2, var2_obj
                                    ));
                                }
                                BinaryOp::Mul => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp *= {} {}",
                                        assign.target, var2, var2_obj
                                    ));
                                }
                                BinaryOp::Div => {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= {} {}",
                                        assign.target, var2, var2_obj
                                    ));
                                }
                                _ => {}
                            }
                        }
                        _ => {
                            // Unsupported combination
                            return Err(format!(
                                "Unsupported binary operation: {:?} between {:?} and {:?}",
                                op, left, right
                            ));
                        }
                    }
                }
                _ => {
                    // Other expression types not supported in simple assignments
                }
            }
        }

        Ok(())
    }
}
