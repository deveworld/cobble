use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_const_assignment(
        &mut self,
        const_assign: &ConstAssignment,
    ) -> Result<(), String> {
        // Infer the type of the constant value
        let value_type = self.infer_type(&const_assign.value);

        // Check if this assignment is type-safe (in case const is redeclared)
        self.check_type_assignment(&const_assign.target, &value_type)?;

        // Record the variable's type
        self.variable_types.insert(const_assign.target.clone(), value_type);

        // Evaluate the expression at compile time
        let value = self.evaluate_const_expr(&const_assign.value)?;

        // Store the constant value
        self.compile_time_constants.insert(const_assign.target.clone(), value);

        // Also store as a regular variable so it can be used in expressions
        self.variables.insert(
            const_assign.target.clone(),
            Expression::Number(value),
        );

        Ok(())
    }

    /// Evaluate an expression at compile time (must be constant)
    fn evaluate_const_expr(&self, expr: &Expression) -> Result<f64, String> {
        match expr {
            Expression::Number(n) => Ok(*n),
            Expression::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Expression::Identifier(name) => {
                // Look up in compile-time constants
                self.compile_time_constants
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        format!(
                            "Const expression references non-const variable: {}",
                            name
                        )
                    })
            }
            Expression::Binary(left, op, right) => {
                let left_val = self.evaluate_const_expr(left)?;
                let right_val = self.evaluate_const_expr(right)?;

                match op {
                    BinaryOp::Add => Ok(left_val + right_val),
                    BinaryOp::Sub => Ok(left_val - right_val),
                    BinaryOp::Mul => Ok(left_val * right_val),
                    BinaryOp::Div => {
                        if right_val == 0.0 {
                            Err("Division by zero in const expression".to_string())
                        } else {
                            Ok(left_val / right_val)
                        }
                    }
                    BinaryOp::Mod => {
                        if right_val == 0.0 {
                            Err("Modulo by zero in const expression".to_string())
                        } else {
                            // Use integer modulo for consistency with runtime behavior
                            Ok(((left_val as i32) % (right_val as i32)) as f64)
                        }
                    }
                    BinaryOp::Pow => Ok(left_val.powf(right_val)),
                    _ => Err(format!("Operator {:?} not supported in const expressions", op)),
                }
            }
            Expression::Unary(op, operand) => {
                let val = self.evaluate_const_expr(operand)?;
                match op {
                    UnaryOp::Neg => Ok(-val),
                    UnaryOp::Pos => Ok(val),
                    UnaryOp::Not => Ok(if val == 0.0 { 1.0 } else { 0.0 }),
                    _ => Err(format!("Operator {:?} not supported in const expressions", op)),
                }
            }
            _ => Err(format!(
                "Expression {:?} cannot be evaluated at compile time",
                expr
            )),
        }
    }
}
