use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), String> {
        // Infer the type of the value being assigned
        let value_type = self.infer_type(&assign.value);

        // Check if this assignment is type-safe
        self.check_type_assignment(&assign.target, &value_type)?;

        // Record the variable's type
        self.variable_types
            .insert(assign.target.clone(), value_type);

        // Store the variable value for later use
        self.variables
            .insert(assign.target.clone(), assign.value.clone());

        // If we're not in a function, store as module-level variable
        // These will be automatically initialized in the _cobble_init function
        if self.current_function.is_none() {
            self.data_pack.track_objective("temp");
            self.variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            self.scoreboard_variables.insert(assign.target.clone());
            self.module_level_vars
                .insert(assign.target.clone(), assign.value.clone());
            return Ok(());
        }

        // Try to evaluate constant expressions first
        if let Some(const_value) = self.try_eval_const(&assign.value) {
            // The expression is a constant - fold it at compile time
            if let Some(ref mut commands) = self.current_function {
                self.data_pack.track_objective("temp");
                self.variable_objectives
                    .insert(assign.target.clone(), "temp".to_string());
                self.scoreboard_variables.insert(assign.target.clone());

                // Warn if number exceeds scoreboard range
                if const_value > i32::MAX as f64 || const_value < i32::MIN as f64 {
                    eprintln!(
                        "⚠️  Warning: Constant expression result {} for variable '{}' exceeds Minecraft scoreboard range.\n\
                        Scoreboard range: {} to {}\n\
                        Value will be clamped to: {}",
                        const_value,
                        assign.target,
                        i32::MIN,
                        i32::MAX,
                        if const_value > i32::MAX as f64 { i32::MAX } else { i32::MIN }
                    );
                }

                // Warn if float has fractional part
                if const_value.fract() != 0.0 {
                    eprintln!(
                        "⚠️  Warning: Constant expression result {} for variable '{}' will lose precision.\n\
                        Scoreboard only supports integers. Fractional part will be truncated to: {}",
                        const_value,
                        assign.target,
                        const_value as i32
                    );
                }

                commands.push(format!(
                    "scoreboard players set {} temp {}",
                    assign.target, const_value as i32
                ));
            }
            return Ok(());
        }

        // Check if we need to use the complex expression evaluator
        // Do this before borrowing to avoid borrow checker issues
        let needs_complex_eval = match &assign.value {
            Expression::Binary(left, _, right) => {
                // Check if either side is a binary expression (nested) or unary expression
                matches!(**left, Expression::Binary(_, _, _))
                    || matches!(**right, Expression::Binary(_, _, _))
                    || matches!(**left, Expression::Unary(_, _))
                    || matches!(**right, Expression::Unary(_, _))
            }
            Expression::Unary(_, _) => {
                // All unary expressions need the complex evaluator
                true
            }
            _ => false,
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

                    // Warn if number exceeds scoreboard range
                    if *n > i32::MAX as f64 || *n < i32::MIN as f64 {
                        eprintln!(
                            "⚠️  Warning: Value {} for variable '{}' exceeds Minecraft scoreboard range.\n\
                            Scoreboard range: {} to {}\n\
                            Value will be clamped to: {}",
                            n,
                            assign.target,
                            i32::MIN,
                            i32::MAX,
                            if *n > i32::MAX as f64 { i32::MAX } else { i32::MIN }
                        );
                    }

                    // Warn if float has fractional part
                    if n.fract() != 0.0 {
                        eprintln!(
                            "⚠️  Warning: Float value {} for variable '{}' will lose precision.\n\
                            Scoreboard only supports integers. Fractional part will be truncated to: {}",
                            n,
                            assign.target,
                            *n as i32
                        );
                    }

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
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players add {} temp {}",
                                        assign.target, value
                                    ));
                                }
                                BinaryOp::Sub => {
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players remove {} temp {}",
                                        assign.target, value
                                    ));
                                }
                                BinaryOp::Mul => {
                                    self.data_pack.track_objective("multiplier");
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
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
                                    // Check for division by zero at compile time
                                    if value == 0 {
                                        return Err(format!(
                                            "Division by zero in assignment: {} = {} / {}",
                                            assign.target, var, value
                                        ));
                                    }
                                    self.data_pack.track_objective("divisor");
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players set divisor temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= divisor temp",
                                        assign.target
                                    ));
                                }
                                BinaryOp::Mod => {
                                    // Check for modulo by zero at compile time
                                    if value == 0 {
                                        return Err(format!(
                                            "Modulo by zero in assignment: {} = {} % {}",
                                            assign.target, var, value
                                        ));
                                    }
                                    self.data_pack.track_objective("modulus");
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players set modulus temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp %= modulus temp",
                                        assign.target
                                    ));
                                }
                                BinaryOp::Pow => {
                                    // Power operation: compile-time expansion
                                    if value < 0 {
                                        return Err(
                                            "Power exponent must be non-negative".to_string()
                                        );
                                    }
                                    if value == 0 {
                                        // x^0 = 1
                                        commands.push(format!(
                                            "scoreboard players set {} temp 1",
                                            assign.target
                                        ));
                                    } else {
                                        // Optimization: Skip self-assignment if target == var
                                        if assign.target != *var || var_obj != "temp" {
                                            commands.push(format!(
                                                "scoreboard players operation {} temp = {} {}",
                                                assign.target, var, var_obj
                                            ));
                                        }
                                        if value > 1 {
                                            self.data_pack.track_objective("power_base");
                                            commands.push(format!(
                                                "scoreboard players operation power_base temp = {} temp",
                                                assign.target
                                            ));
                                            for _ in 0..(value - 1) {
                                                commands.push(format!(
                                                    "scoreboard players operation {} temp *= power_base temp",
                                                    assign.target
                                                ));
                                            }
                                        }
                                    }
                                }
                                _ => {}
                            }
                        }
                        (Expression::Number(n1), Expression::Number(n2)) => {
                            // Constant expression evaluation with error checking
                            let result = match op {
                                BinaryOp::Add => (*n1 + *n2) as i32,
                                BinaryOp::Sub => (*n1 - *n2) as i32,
                                BinaryOp::Mul => (*n1 * *n2) as i32,
                                BinaryOp::Div => {
                                    if *n2 == 0.0 {
                                        return Err(format!(
                                            "Division by zero in constant expression: {} / {}",
                                            n1, n2
                                        ));
                                    }
                                    (*n1 / *n2) as i32
                                }
                                BinaryOp::Mod => {
                                    if *n2 == 0.0 {
                                        return Err(format!(
                                            "Modulo by zero in constant expression: {} % {}",
                                            n1, n2
                                        ));
                                    }
                                    (*n1 as i32) % (*n2 as i32)
                                }
                                BinaryOp::Pow => {
                                    let base = *n1 as i32;
                                    let exp = *n2 as i32;
                                    if exp < 0 {
                                        return Err(
                                            "Power exponent must be non-negative".to_string()
                                        );
                                    }
                                    base.pow(exp as u32)
                                }
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
                                BinaryOp::Mod => {
                                    // score = value % var
                                    self.data_pack.track_objective("modulus");
                                    commands.push(format!(
                                        "scoreboard players set {} temp {}",
                                        assign.target, value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp %= {} {}",
                                        assign.target, var, var_obj
                                    ));
                                }
                                BinaryOp::Pow => {
                                    // Power with variable exponent is not supported at compile time
                                    return Err("Power with variable exponent is not supported. Use constant exponents only.".to_string());
                                }
                                _ => {}
                            }
                        }
                        (Expression::Identifier(var1), Expression::Identifier(var2)) => {
                            let left_const = self.compile_time_constants.get(var1).copied();
                            let right_const = self.compile_time_constants.get(var2).copied();

                            if let Some(value) = left_const {
                                let num = value as i32;
                                let var_obj = self
                                    .variable_objectives
                                    .get(var2)
                                    .unwrap_or(&"temp".to_string())
                                    .clone();

                                match op {
                                    BinaryOp::Add => {
                                        commands.push(format!(
                                            "scoreboard players set {} temp {}",
                                            assign.target, num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp += {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                    }
                                    BinaryOp::Sub => {
                                        commands.push(format!(
                                            "scoreboard players set {} temp {}",
                                            assign.target, num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp -= {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                    }
                                    BinaryOp::Mul => {
                                        self.data_pack.track_objective("multiplier");
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                        commands.push(format!(
                                            "scoreboard players set multiplier temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp *= multiplier temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Div => {
                                        commands.push(format!(
                                            "scoreboard players set {} temp {}",
                                            assign.target, num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp /= {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                    }
                                    BinaryOp::Mod => {
                                        commands.push(format!(
                                            "scoreboard players set {} temp {}",
                                            assign.target, num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp %= {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                    }
                                    BinaryOp::Pow => {
                                        return Err("Power with variable exponent is not supported. Use constant exponents only.".to_string());
                                    }
                                    _ => {}
                                }
                            } else if let Some(value) = right_const {
                                let num = value as i32;
                                let var1_obj = self
                                    .variable_objectives
                                    .get(var1)
                                    .unwrap_or(&"temp".to_string())
                                    .clone();

                                if assign.target != *var1 || var1_obj != "temp" {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var1, var1_obj
                                    ));
                                }

                                match op {
                                    BinaryOp::Add => {
                                        commands.push(format!(
                                            "scoreboard players add {} temp {}",
                                            assign.target, num
                                        ));
                                    }
                                    BinaryOp::Sub => {
                                        commands.push(format!(
                                            "scoreboard players remove {} temp {}",
                                            assign.target, num
                                        ));
                                    }
                                    BinaryOp::Mul => {
                                        self.data_pack.track_objective("multiplier");
                                        commands.push(format!(
                                            "scoreboard players set multiplier temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp *= multiplier temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Div => {
                                        if num == 0 {
                                            return Err(
                                                "Division by zero in assignment".to_string()
                                            );
                                        }
                                        self.data_pack.track_objective("divisor");
                                        commands.push(format!(
                                            "scoreboard players set divisor temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp /= divisor temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Mod => {
                                        if num == 0 {
                                            return Err("Modulo by zero in assignment".to_string());
                                        }
                                        self.data_pack.track_objective("modulus");
                                        commands.push(format!(
                                            "scoreboard players set modulus temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp %= modulus temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Pow => {
                                        if num < 0 {
                                            return Err(
                                                "Power exponent must be non-negative".to_string()
                                            );
                                        }
                                        if num == 0 {
                                            commands.push(format!(
                                                "scoreboard players set {} temp 1",
                                                assign.target
                                            ));
                                        } else if num == 1 {
                                            // nothing to do
                                        } else {
                                            self.data_pack.track_objective("power_base");
                                            commands.push(format!(
                                                "scoreboard players operation power_base temp = {} temp",
                                                assign.target
                                            ));
                                            for _ in 0..(num - 1) {
                                                commands.push(format!(
                                                    "scoreboard players operation {} temp *= power_base temp",
                                                    assign.target
                                                ));
                                            }
                                        }
                                    }
                                    _ => {}
                                }
                            } else {
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

                                if assign.target != *var1 || var1_obj != "temp" {
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var1, var1_obj
                                    ));
                                }

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
                                        // Check if var2 is a compile-time constant with value 0
                                        if let Some(const_val) = self.compile_time_constants.get(var2) {
                                            if *const_val == 0.0 {
                                                eprintln!(
                                                    "⚠️  Warning: Division by variable '{}' which has constant value 0.\n\
                                                    This will cause undefined behavior in Minecraft (typically returns 0).\n\
                                                    Consider checking the divisor before division.",
                                                    var2
                                                );
                                            }
                                        }
                                        commands.push(format!(
                                            "scoreboard players operation {} temp /= {} {}",
                                            assign.target, var2, var2_obj
                                        ));
                                    }
                                    BinaryOp::Mod => {
                                        // Check if var2 is a compile-time constant with value 0
                                        if let Some(const_val) = self.compile_time_constants.get(var2) {
                                            if *const_val == 0.0 {
                                                eprintln!(
                                                    "⚠️  Warning: Modulo by variable '{}' which has constant value 0.\n\
                                                    This will cause undefined behavior in Minecraft (typically returns 0).\n\
                                                    Consider checking the divisor before modulo operation.",
                                                    var2
                                                );
                                            }
                                        }
                                        commands.push(format!(
                                            "scoreboard players operation {} temp %= {} {}",
                                            assign.target, var2, var2_obj
                                        ));
                                    }
                                    BinaryOp::Pow => {
                                        return Err("Power with variable exponent is not supported. Use constant exponents only.".to_string());
                                    }
                                    _ => {}
                                }
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
