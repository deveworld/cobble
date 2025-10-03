use crate::ast::{BinaryOp, Expression, UnaryOp};
use std::collections::HashMap;

/// Translates Python-like conditions to Minecraft execute conditions
pub struct ConditionTranslator<'a> {
    pub variable_objectives: &'a HashMap<String, String>,
}

impl<'a> ConditionTranslator<'a> {
    pub fn new(variable_objectives: &'a HashMap<String, String>) -> Self {
        Self {
            variable_objectives,
        }
    }

    pub fn translate(&self, condition: &Expression) -> Result<String, String> {
        // Translate Python-like conditions to Minecraft execute conditions
        match condition {
            Expression::Binary(left, op, right) => {
                match op {
                    BinaryOp::And => {
                        // And: chain multiple conditions with "if ... if ..."
                        let left_cond = self.translate(left)?;
                        let right_cond = self.translate(right)?;

                        // Check if either condition contains OR - if so, mark the whole expression as OR
                        // The if_processor will handle expanding nested ORs
                        if left_cond.contains("OR(") || right_cond.contains("OR(") {
                            // Return combined OR expression for if_processor to handle
                            // Format: OR_AND(left, right) to indicate AND with nested OR
                            return Ok(format!("OR_AND({}, {})", left_cond, right_cond));
                        }

                        // Add "if" prefix if not present (for chaining)
                        let left_final = if left_cond.starts_with("if ") || left_cond.starts_with("unless ") {
                            left_cond
                        } else {
                            format!("if {}", left_cond)
                        };

                        let right_final = if right_cond.starts_with("if ") || right_cond.starts_with("unless ") {
                            right_cond
                        } else {
                            format!("if {}", right_cond)
                        };

                        Ok(format!("{} {}", left_final, right_final))
                    }
                    BinaryOp::Or => {
                        // Or: requires setting a flag variable
                        // Returns a special marker that if_processor will handle
                        let left_cond = self.translate(left)?;
                        let right_cond = self.translate(right)?;

                        // Return special format: OR(cond1, cond2)
                        Ok(format!("OR({}, {})", left_cond, right_cond))
                    }
                    // Comparison operators
                    _ => {
                        // Extract variable name
                        let var_name = match &**left {
                            Expression::Identifier(name) => name.clone(),
                            _ => return Err("Left side of condition must be a variable".to_string()),
                        };

                        // Get the objective for this variable (defaults to "temp")
                        let objective = self
                            .variable_objectives
                            .get(&var_name)
                            .unwrap_or(&"temp".to_string())
                            .clone();

                        // Check if right side is a number or a variable
                        match &**right {
                            Expression::Number(n) => {
                                let value = *n as i32;
                                // Generate condition with literal value
                                match op {
                                    BinaryOp::Eq => Ok(format!(
                                        "score {} {} matches {}",
                                        var_name, objective, value
                                    )),
                                    BinaryOp::NotEq => {
                                        // Use "unless" instead of "!" to avoid issues with elif/else
                                        Ok(format!(
                                            "unless score {} {} matches {}",
                                            var_name, objective, value
                                        ))
                                    }
                                    BinaryOp::Gt => Ok(format!(
                                        "score {} {} matches {}..",
                                        var_name,
                                        objective,
                                        value + 1
                                    )),
                                    BinaryOp::GtEq => Ok(format!(
                                        "score {} {} matches {}..",
                                        var_name, objective, value
                                    )),
                                    BinaryOp::Lt => Ok(format!(
                                        "score {} {} matches ..{}",
                                        var_name,
                                        objective,
                                        value - 1
                                    )),
                                    BinaryOp::LtEq => Ok(format!(
                                        "score {} {} matches ..{}",
                                        var_name, objective, value
                                    )),
                                    _ => Ok(format!("entity @s[scores={{{}={}}}]", var_name, value)),
                                }
                            }
                    Expression::Identifier(other_var) => {
                        // Generate condition comparing two variables
                        let other_objective = self
                            .variable_objectives
                            .get(other_var)
                            .unwrap_or(&"temp".to_string())
                            .clone();
                        match op {
                            BinaryOp::Eq => Ok(format!(
                                "score {} {} = {} {}",
                                var_name, objective, other_var, other_objective
                            )),
                            BinaryOp::NotEq => {
                                // Use "unless" instead of "!" to avoid issues with elif/else
                                Ok(format!(
                                    "unless score {} {} = {} {}",
                                    var_name, objective, other_var, other_objective
                                ))
                            }
                            BinaryOp::Gt => Ok(format!(
                                "score {} {} > {} {}",
                                var_name, objective, other_var, other_objective
                            )),
                            BinaryOp::GtEq => Ok(format!(
                                "score {} {} >= {} {}",
                                var_name, objective, other_var, other_objective
                            )),
                            BinaryOp::Lt => Ok(format!(
                                "score {} {} < {} {}",
                                var_name, objective, other_var, other_objective
                            )),
                            BinaryOp::LtEq => Ok(format!(
                                "score {} {} <= {} {}",
                                var_name, objective, other_var, other_objective
                            )),
                            _ => Err("Unsupported operator for variable comparison".to_string()),
                        }
                    }
                    _ => Err("Right side of condition must be a number or variable".to_string()),
                }
                    }
                }
            }
            Expression::Identifier(var) => {
                // Simple boolean check: treat as "var > 0"
                let objective = self
                    .variable_objectives
                    .get(var)
                    .unwrap_or(&"temp".to_string())
                    .clone();
                Ok(format!("score {} {} matches 1..", var, objective))
            }
            Expression::Unary(op, expr) => {
                match op {
                    UnaryOp::Not => {
                        // Not: convert "if" to "unless" or vice versa
                        let inner_cond = self.translate(expr)?;
                        if inner_cond.starts_with("unless ") {
                            // Double negative: "unless" becomes "if"
                            Ok(inner_cond.replace("unless ", "if "))
                        } else if inner_cond.starts_with("OR(") || inner_cond.starts_with("OR_AND(") {
                            // NOT with OR - mark it for special handling
                            Ok(format!("NOT_{}", inner_cond))
                        } else if inner_cond.starts_with("score ") || inner_cond.starts_with("entity ") || inner_cond.starts_with("block ") {
                            // Add "unless" prefix
                            Ok(format!("unless {}", inner_cond))
                        } else {
                            // Already has "if", convert to "unless"
                            Ok(inner_cond.replace("if ", "unless "))
                        }
                    }
                    _ => Err("Unsupported unary operator in condition".to_string()),
                }
            }
            _ => Ok("entity @s".to_string()),
        }
    }
}
