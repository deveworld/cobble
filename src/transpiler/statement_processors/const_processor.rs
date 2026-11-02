use crate::ast::*;
use crate::transpiler::{const_mod, TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_const_assignment(
        &mut self,
        const_assign: &ConstAssignment,
    ) -> Result<(), TranspileError> {
        // Module-level code runs from top to bottom, so its value can't
        // read a module name declared further down.
        if self.current_function.is_none() {
            self.check_module_level_reads(&const_assign.value)?;
            self.symbols
                .pending_module_declarations
                .remove(&const_assign.target);
        }

        // Infer the type of the constant value
        let value_type = self.infer_type(&const_assign.value);

        // Check if this assignment is type-safe (in case const is redeclared)
        self.check_type_assignment(&const_assign.target, &value_type)?;

        // Record the variable's type
        self.symbols
            .variable_types
            .insert(const_assign.target.clone(), value_type);

        // Evaluate the expression at compile time
        let value = self.evaluate_const_expr(&const_assign.value)?;

        // Store the constant value
        self.symbols
            .compile_time_constants
            .insert(const_assign.target.clone(), value);

        // Also store as a regular variable so it can be used in expressions
        self.symbols
            .variables
            .insert(const_assign.target.clone(), Expression::Number(value));

        // Constants are compile-time only, no runtime initialization needed

        Ok(())
    }

    /// Evaluate an expression at compile time (must be constant)
    pub(in crate::transpiler) fn evaluate_const_expr(
        &self,
        expr: &Expression,
    ) -> Result<f64, TranspileError> {
        match expr {
            Expression::Number(n) => Ok(*n),
            Expression::Boolean(b) => Ok(if *b { 1.0 } else { 0.0 }),
            Expression::Call(func, args)
                if matches!(&**func, Expression::Identifier(name) if matches!(name.as_str(), "_cobble_float" | "float" | "double" | "int") && !self.functions.params.contains_key(name))
                    && (args.len() == 1
                        || matches!(&**func, Expression::Identifier(name) if name == "_cobble_float")
                            && args.len() == 2) =>
            {
                let value = self.evaluate_const_expr(&args[0])?;
                let Expression::Identifier(name) = &**func else {
                    unreachable!()
                };
                Ok(match name.as_str() {
                    "double" => value,
                    "int" => f64::from(value as i32),
                    "_cobble_float" if args.len() == 2 => {
                        let Expression::String(text) = &args[1] else {
                            return Err("Invalid internal float literal.".into());
                        };
                        f64::from(text.parse::<f32>().map_err(|_| "Invalid float literal.")?)
                    }
                    _ => f64::from(value as f32),
                })
            }
            Expression::Identifier(name) => {
                // Look up in compile-time constants
                self.symbols
                    .compile_time_constants
                    .get(name)
                    .copied()
                    .ok_or_else(|| {
                        format!("Const expression references non-const variable: {}", name)
                    })
                    .map_err(TranspileError::from)
            }
            Expression::Binary(left, op, right) => {
                let left_val = self.evaluate_const_expr(left)?;
                let right_val = self.evaluate_const_expr(right)?;

                match op {
                    BinaryOp::Add => Ok(left_val + right_val),
                    BinaryOp::Sub => Ok(left_val - right_val),
                    BinaryOp::Mul => Ok(left_val * right_val),
                    // A `const` is a compile-time value that raw commands may
                    // substitute with a fraction (`~{HALF}` with
                    // `const HALF = 1 / 2`), so `/` is real division here, like
                    // Python's `/`. Integer semantics apply where an expression
                    // is computed into a score (see `const_div`).
                    BinaryOp::Div | BinaryOp::FloorDiv => {
                        if right_val == 0.0 {
                            Err("Division by zero in const expression".into())
                        } else {
                            let result = left_val / right_val;
                            Ok(if matches!(op, BinaryOp::FloorDiv) { result.floor() } else { result })
                        }
                    }
                    // Floor modulus, matching runtime `%=` (and Python's `%`).
                    BinaryOp::Mod => const_mod(left_val, right_val)
                        .ok_or_else(|| "Modulo by zero in const expression".into()),
                    BinaryOp::Pow => {
                        let base = left_val as i32;
                        let exp = right_val as i32;
                        if exp < 0 {
                            return Err("Power exponent must be non-negative in const expression"
                                .to_string()
                                .into());
                        }
                        // Real arithmetic, like `*`. A score that stores the
                        // result warns when it can't hold it.
                        Ok(f64::from(base).powi(exp))
                    }
                    _ => {
                        Err(format!(
                            "`{}` can't be worked out in a const; a const holds a number from `+`, `-`, `*`, `/`, `%`, and `**`",
                            Self::binary_op_source_key(op)
                        )
                        .into())
                    }
                }
            }
            Expression::Unary(op, operand) => {
                let val = self.evaluate_const_expr(operand)?;
                match op {
                    UnaryOp::Neg => Ok(-val),
                    UnaryOp::Pos => Ok(val),
                    UnaryOp::Not => Ok(if val == 0.0 { 1.0 } else { 0.0 }),
                }
            }
            Expression::String(_) | Expression::Array(_) | Expression::Map(_) => {
                Err("Only numeric constants are supported at this time."
                    .to_string()
                    .into())
            }
            _ => Err("Expression type cannot be evaluated at compile time"
                .to_string()
                .into()),
        }
    }
}
