use crate::ast::{BinaryOp, Expression, UnaryOp};
use crate::transpiler::data_pack::DataPack;
use std::collections::HashMap;

/// Evaluate expressions and translate conditions for Minecraft commands
pub struct ExpressionEvaluator<'a> {
    pub data_pack: &'a mut DataPack,
    pub variable_objectives: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
}

impl<'a> ExpressionEvaluator<'a> {
    pub fn new(
        data_pack: &'a mut DataPack,
        variable_objectives: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
    ) -> Self {
        Self {
            data_pack,
            variable_objectives,
            compile_time_constants,
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
                if n.fract() != 0.0 {
                    eprintln!(
                        "⚠️  Warning: Float value {} will lose precision.\n\
                        Scoreboard only supports integers. Fractional part will be truncated to: {}",
                        n, *n as i32
                    );
                }
                commands.push(format!(
                    "scoreboard players set {} temp {}",
                    target, *n as i32
                ));
            }
            Expression::Identifier(var) => {
                if let Some(const_value) = self.compile_time_constants.get(var) {
                    let truncated = *const_value as i32;
                    self.data_pack.track_objective("temp");
                    commands.push(format!(
                        "scoreboard players set {} temp {}",
                        target, truncated
                    ));
                } else {
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
                        if n.fract() != 0.0 {
                            eprintln!(
                                "⚠️  Warning: Float value {} in expression will lose precision.\n\
                                Scoreboard only supports integers. Fractional part will be truncated to: {}",
                                n, *n as i32
                            );
                        }
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
                                commands
                                    .push(format!("scoreboard players set modulus temp {}", value));
                                commands.push(format!(
                                    "scoreboard players operation {} temp %= modulus temp",
                                    target
                                ));
                            }
                            BinaryOp::Pow => {
                                // Compile-time expansion: x^n becomes x*x*...*x (n times)
                                if value < 0 {
                                    return Err("Power exponent must be non-negative".to_string());
                                }
                                // Limit maximum exponent to prevent excessive command generation
                                const MAX_POWER_EXPONENT: i32 = 100;
                                if value > MAX_POWER_EXPONENT {
                                    return Err(format!(
                                        "Power exponent too large: {} > {}.\n\
                                        \n\
                                        Large exponents generate {} multiplication commands, which is excessive.\n\
                                        Solution: Use a smaller exponent or implement iterative multiplication in a loop.",
                                        value, MAX_POWER_EXPONENT, value - 1
                                    ));
                                }
                                if value == 0 {
                                    // x^0 = 1
                                    commands
                                        .push(format!("scoreboard players set {} temp 1", target));
                                } else if value == 1 {
                                    // x^1 = x, no operation needed
                                } else {
                                    // Store original value for multiplication
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
                        if let Some(const_value) = self.compile_time_constants.get(var) {
                            let value = *const_value as i32;
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
                                    if value == 0 {
                                        return Err("Division by zero in expression".to_string());
                                    }
                                    commands.push(format!(
                                        "scoreboard players set divisor temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= divisor temp",
                                        target
                                    ));
                                }
                                BinaryOp::Mod => {
                                    if value == 0 {
                                        return Err("Modulo by zero in expression".to_string());
                                    }
                                    commands.push(format!(
                                        "scoreboard players set modulus temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp %= modulus temp",
                                        target
                                    ));
                                }
                                BinaryOp::Pow => {
                                    if value < 0 {
                                        return Err(
                                            "Power exponent must be non-negative".to_string()
                                        );
                                    }
                                    if value == 0 {
                                        commands.push(format!(
                                            "scoreboard players set {} temp 1",
                                            target
                                        ));
                                    } else if value == 1 {
                                        // x^1 = x, nothing to do (target already holds left side)
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players operation power_base temp = {} temp",
                                            target
                                        ));
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
                        } else {
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
                                    // Add runtime warning for potential division by zero
                                    eprintln!(
                                        "⚠️  Warning: Runtime division by variable '{}' at line.\n\
                                        If '{}' is 0 at runtime, Minecraft will silently return 0.\n\
                                        Consider adding a check: if {} != 0",
                                        var, var, var
                                    );
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= {} {}",
                                        target, var, var_obj
                                    ));
                                }
                                BinaryOp::Mod => {
                                    // Add runtime warning for potential modulo by zero
                                    eprintln!(
                                        "⚠️  Warning: Runtime modulo by variable '{}' at line.\n\
                                        If '{}' is 0 at runtime, Minecraft will silently return 0.\n\
                                        Consider adding a check: if {} != 0",
                                        var, var, var
                                    );
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
                    }
                    Expression::Binary(_, _, _) => {
                        // Right side is also a binary expression - need to evaluate it first
                        // Use a temporary variable for the right side
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
                                // Add runtime warning for potential division by zero with complex expression
                                eprintln!(
                                    "⚠️  Warning: Runtime division by expression result.\n\
                                    If the expression evaluates to 0 at runtime, Minecraft will silently return 0.\n\
                                    Consider validating the divisor before division."
                                );
                                commands.push(format!(
                                    "scoreboard players operation {} temp /= expr_temp temp",
                                    target
                                ));
                            }
                            BinaryOp::Mod => {
                                // Add runtime warning for potential modulo by zero with complex expression
                                eprintln!(
                                    "⚠️  Warning: Runtime modulo by expression result.\n\
                                    If the expression evaluates to 0 at runtime, Minecraft will silently return 0.\n\
                                    Consider validating the divisor before modulo."
                                );
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
            Expression::Unary(op, expr) => {
                match op {
                    UnaryOp::Neg => {
                        // Unary negation: -expr
                        // Evaluate the expression first, then multiply by -1
                        let expr_commands = self.evaluate_expression_to_target(expr, target)?;
                        commands.extend(expr_commands);

                        // Multiply by -1
                        self.data_pack.track_objective("multiplier");
                        commands.push("scoreboard players set multiplier temp -1".to_string());
                        commands.push(format!(
                            "scoreboard players operation {} temp *= multiplier temp",
                            target
                        ));
                    }
                    UnaryOp::Pos => {
                        // Unary plus: +expr (no-op, just evaluate the expression)
                        let expr_commands = self.evaluate_expression_to_target(expr, target)?;
                        commands.extend(expr_commands);
                    }
                    UnaryOp::Not => {
                        return Err(
                            "Logical NOT operator cannot be used in arithmetic expressions"
                                .to_string(),
                        );
                    }
                    _ => {
                        return Err(format!("Unsupported unary operator: {:?}", op));
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
