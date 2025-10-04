use crate::ast::{BinaryOp, Expression, UnaryOp};
use std::collections::HashMap;

/// Translates Python-like conditions to Minecraft execute conditions
pub struct ConditionTranslator<'a> {
    pub variable_objectives: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
}

impl<'a> ConditionTranslator<'a> {
    pub fn new(
        variable_objectives: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
    ) -> Self {
        Self {
            variable_objectives,
            compile_time_constants,
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
                        let left_final =
                            if left_cond.starts_with("if ") || left_cond.starts_with("unless ") {
                                left_cond
                            } else {
                                format!("if {}", left_cond)
                            };

                        let right_final =
                            if right_cond.starts_with("if ") || right_cond.starts_with("unless ") {
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
                        let left_score = self.extract_scoreboard(left);
                        let right_score = self.extract_scoreboard(right);
                        let left_literal = self.extract_literal(left);
                        let right_literal = self.extract_literal(right);

                        match (left_score, right_score, left_literal, right_literal) {
                            (
                                Some((var_name, objective)),
                                Some((other_var, other_objective)),
                                _,
                                _,
                            ) => match op {
                                BinaryOp::Eq => Ok(format!(
                                    "score {} {} = {} {}",
                                    var_name, objective, other_var, other_objective
                                )),
                                BinaryOp::NotEq => Ok(format!(
                                    "unless score {} {} = {} {}",
                                    var_name, objective, other_var, other_objective
                                )),
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
                                _ => {
                                    Err("Unsupported operator for variable comparison".to_string())
                                }
                            },
                            (Some((var_name, objective)), _, _, Some(value)) => {
                                self.score_vs_literal_condition(&var_name, &objective, op, value)
                            }
                            (_, Some((_var_name, _objective)), Some(_), _) => {
                                if let Some(reversed) = self.reverse_operator(op) {
                                    // Recursively translate with swapped sides
                                    let swapped =
                                        Expression::Binary(right.clone(), reversed, left.clone());
                                    self.translate(&swapped)
                                } else {
                                    Err("Unsupported operator for literal comparison".to_string())
                                }
                            }
                            (None, None, Some(left_value), Some(right_value)) => {
                                self.literal_vs_literal_condition(left_value, right_value, op)
                            }
                            _ => {
                                Err("Condition must compare scoreboard values or literals"
                                    .to_string())
                            }
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
                        } else if inner_cond.starts_with("OR(") || inner_cond.starts_with("OR_AND(")
                        {
                            // NOT with OR - mark it for special handling
                            Ok(format!("NOT_{}", inner_cond))
                        } else if inner_cond.starts_with("score ")
                            || inner_cond.starts_with("entity ")
                            || inner_cond.starts_with("block ")
                        {
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
            Expression::Boolean(b) => {
                // Boolean literals - use a condition that always evaluates correctly
                // regardless of execution context
                if *b {
                    // True: use a condition that always succeeds
                    // Using a dummy scoreboard check that's always true
                    Ok("score #true_const __internal__ matches 1..".to_string())
                } else {
                    // False: use a condition that always fails
                    Ok("score #false_const __internal__ matches 1..".to_string())
                }
            }
            _ => Err(format!(
                "Unsupported condition expression. Conditions must be:\n\
                 - Comparisons (x > y, x == 5, etc.)\n\
                 - Boolean variables (x)\n\
                 - Boolean literals (True, False)\n\
                 - Logical operators (and, or, not)\n\
                 Got: {:?}",
                condition
            )),
        }
    }

    fn extract_literal(&self, expr: &Expression) -> Option<i32> {
        match expr {
            Expression::Number(n) => Some(*n as i32),
            Expression::Boolean(b) => Some(if *b { 1 } else { 0 }),
            Expression::Identifier(name) => self
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32),
            _ => None,
        }
    }

    fn extract_scoreboard(&self, expr: &Expression) -> Option<(String, String)> {
        if let Expression::Identifier(name) = expr {
            if self.compile_time_constants.contains_key(name) {
                None
            } else {
                let objective = self
                    .variable_objectives
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "temp".to_string());
                Some((name.clone(), objective))
            }
        } else {
            None
        }
    }

    fn reverse_operator(&self, op: &BinaryOp) -> Option<BinaryOp> {
        use BinaryOp::*;
        match op {
            Eq => Some(Eq),
            NotEq => Some(NotEq),
            Lt => Some(Gt),
            LtEq => Some(GtEq),
            Gt => Some(Lt),
            GtEq => Some(LtEq),
            _ => None,
        }
    }

    fn score_vs_literal_condition(
        &self,
        var_name: &str,
        objective: &str,
        op: &BinaryOp,
        value: i32,
    ) -> Result<String, String> {
        use BinaryOp::*;

        Ok(match op {
            Eq => format!("score {} {} matches {}", var_name, objective, value),
            NotEq => format!("unless score {} {} matches {}", var_name, objective, value),
            Gt => format!(
                "score {} {} matches {}..",
                var_name,
                objective,
                value.saturating_add(1)
            ),
            GtEq => format!("score {} {} matches {}..", var_name, objective, value),
            Lt => format!(
                "score {} {} matches ..{}",
                var_name,
                objective,
                value.saturating_sub(1)
            ),
            LtEq => format!("score {} {} matches ..{}", var_name, objective, value),
            _ => {
                return Err("Unsupported operator for literal comparison".to_string());
            }
        })
    }

    fn literal_vs_literal_condition(
        &self,
        left: i32,
        right: i32,
        op: &BinaryOp,
    ) -> Result<String, String> {
        use BinaryOp::*;

        let result = match op {
            Eq => left == right,
            NotEq => left != right,
            Lt => left < right,
            LtEq => left <= right,
            Gt => left > right,
            GtEq => left >= right,
            _ => return Err("Unsupported operator for literal comparison".to_string()),
        };

        // Use internal scoreboard constants for compile-time boolean results
        if result {
            Ok("score #true_const __internal__ matches 1..".to_string())
        } else {
            Ok("score #false_const __internal__ matches 1..".to_string())
        }
    }
}
