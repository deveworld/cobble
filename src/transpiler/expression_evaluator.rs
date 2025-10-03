use crate::ast::{BinaryOp, Expression};
use crate::transpiler::data_pack::DataPack;
use std::collections::HashMap;

/// Evaluate expressions and translate conditions for Minecraft commands
pub struct ExpressionEvaluator<'a> {
    pub data_pack: &'a mut DataPack,
    pub variable_objectives: &'a HashMap<String, String>,
}

impl<'a> ExpressionEvaluator<'a> {
    pub fn new(
        data_pack: &'a mut DataPack,
        variable_objectives: &'a HashMap<String, String>,
    ) -> Self {
        Self {
            data_pack,
            variable_objectives,
        }
    }

    /// Helper method to evaluate a complex expression into a target variable
    /// Returns the commands needed to compute the expression
    pub fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String> {
        let mut commands = Vec::new();
        self.data_pack.track_objective("temp");

        match expr {
            Expression::Number(n) => {
                commands.push(format!(
                    "scoreboard players set {} temp {}",
                    target, *n as i32
                ));
            }
            Expression::Identifier(var) => {
                let var_obj = self
                    .variable_objectives
                    .get(var)
                    .unwrap_or(&"temp".to_string())
                    .clone();
                commands.push(format!(
                    "scoreboard players operation {} temp = {} {}",
                    target, var, var_obj
                ));
            }
            Expression::Binary(left, op, right) => {
                // For nested binary expressions like (a + b) + c:
                // First evaluate left side into target, then apply operation with right side

                // Evaluate left side into target
                let left_commands = self.evaluate_expression_to_target(left, target)?;
                commands.extend(left_commands);

                // Now apply the operation with the right side
                match &**right {
                    Expression::Number(n) => {
                        let value = *n as i32;
                        match op {
                            BinaryOp::Add => {
                                commands.push(format!(
                                    "scoreboard players add {} temp {}",
                                    target, value
                                ));
                            }
                            BinaryOp::Sub => {
                                commands.push(format!(
                                    "scoreboard players remove {} temp {}",
                                    target, value
                                ));
                            }
                            BinaryOp::Mul => {
                                self.data_pack.track_objective("multiplier");
                                commands.push(format!(
                                    "scoreboard players set multiplier temp {}",
                                    value
                                ));
                                commands.push(format!(
                                    "scoreboard players operation {} temp *= multiplier temp",
                                    target
                                ));
                            }
                            BinaryOp::Div => {
                                // Check for division by zero at compile time
                                if value == 0 {
                                    return Err(format!(
                                        "Division by zero in expression: dividing by {}",
                                        value
                                    ));
                                }
                                self.data_pack.track_objective("divisor");
                                commands
                                    .push(format!("scoreboard players set divisor temp {}", value));
                                commands.push(format!(
                                    "scoreboard players operation {} temp /= divisor temp",
                                    target
                                ));
                            }
                            BinaryOp::Mod => {
                                // Check for modulo by zero at compile time
                                if value == 0 {
                                    return Err(format!(
                                        "Modulo by zero in expression: modulo by {}",
                                        value
                                    ));
                                }
                                self.data_pack.track_objective("modulus");
                                commands
                                    .push(format!("scoreboard players set modulus temp {}", value));
                                commands.push(format!(
                                    "scoreboard players operation {} temp %= modulus temp",
                                    target
                                ));
                            }
                            BinaryOp::Pow => {
                                // Compile-time expansion: x^n becomes x*x*...*x (n times)
                                if value < 1 {
                                    return Err("Power exponent must be at least 1".to_string());
                                }
                                if value == 1 {
                                    // x^1 = x, no operation needed
                                } else {
                                    // Store original value for multiplication
                                    self.data_pack.track_objective("power_base");
                                    commands.push(format!(
                                        "scoreboard players operation power_base temp = {} temp",
                                        target
                                    ));
                                    // Multiply (value - 1) times
                                    for _ in 0..(value - 1) {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp *= power_base temp",
                                            target
                                        ));
                                    }
                                }
                            }
                            _ => return Err(format!("Unsupported binary operation: {:?}", op)),
                        }
                    }
                    Expression::Identifier(var) => {
                        let var_obj = self
                            .variable_objectives
                            .get(var)
                            .unwrap_or(&"temp".to_string())
                            .clone();
                        match op {
                            BinaryOp::Add => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp += {} {}",
                                    target, var, var_obj
                                ));
                            }
                            BinaryOp::Sub => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp -= {} {}",
                                    target, var, var_obj
                                ));
                            }
                            BinaryOp::Mul => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp *= {} {}",
                                    target, var, var_obj
                                ));
                            }
                            BinaryOp::Div => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp /= {} {}",
                                    target, var, var_obj
                                ));
                            }
                            BinaryOp::Mod => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp %= {} {}",
                                    target, var, var_obj
                                ));
                            }
                            BinaryOp::Pow => {
                                // Power with variable: target = target ^ var
                                // We need to implement iterative multiplication
                                // For now, this is complex and not commonly used
                                return Err("Power operator with variable exponent is not supported. Use constant exponents like: x ^ 2".to_string());
                            }
                            _ => return Err(format!("Unsupported binary operation: {:?}", op)),
                        }
                    }
                    Expression::Binary(_, _, _) => {
                        // Right side is also a binary expression - need to evaluate it first
                        // Use a temporary variable for the right side
                        self.data_pack.track_objective("expr_temp");
                        let right_commands =
                            self.evaluate_expression_to_target(right, "expr_temp")?;
                        commands.extend(right_commands);

                        // Now perform the operation
                        match op {
                            BinaryOp::Add => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp += expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Sub => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp -= expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Mul => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp *= expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Div => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp /= expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Mod => {
                                commands.push(format!(
                                    "scoreboard players operation {} temp %= expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Pow => {
                                // Power with nested expression: target = target ^ (complex_expr)
                                // This would require loop unrolling based on runtime value
                                // Not practical for Minecraft commands
                                return Err("Power operator with complex expressions is not supported. Use constant exponents like: x ^ 2".to_string());
                            }
                            _ => return Err(format!("Unsupported binary operation: {:?}", op)),
                        }
                    }
                    _ => {
                        return Err("Unsupported expression type in binary operation".to_string());
                    }
                }
            }
            _ => {
                return Err("Unsupported expression type".to_string());
            }
        }

        Ok(commands)
    }
}
