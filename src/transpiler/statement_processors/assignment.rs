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
            // Storage types (Array, Map, String) use NBT storage, not scoreboard
            match &assign.value {
                Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                    let storage_path = format!("vars.{}", assign.target);
                    self.variable_storage_paths
                        .insert(assign.target.clone(), storage_path);
                }
                _ => {
                    self.data_pack.track_objective("temp");
                    self.variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.scoreboard_variables.insert(assign.target.clone());
                }
            }
            self.module_level_vars.push((
                assign.target.clone(),
                assign.value.clone(),
                self.current_statement_source.clone(),
            ));
            return Ok(());
        }

        if !matches!(
            &assign.value,
            Expression::Array(_) | Expression::Map(_) | Expression::String(_)
        ) {
            self.variable_storage_paths.remove(&assign.target);
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

        if Self::is_runtime_condition_expression(&assign.value) {
            self.data_pack.track_objective("temp");
            self.variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            self.scoreboard_variables.insert(assign.target.clone());

            let processed_condition = self.preprocess_condition(&assign.value)?;
            let condition_cmd =
                self.normalize_if_condition(self.translate_condition(&processed_condition)?)?;
            let condition_execute_args = Self::condition_execute_args(&condition_cmd);

            if let Some(ref mut commands) = self.current_function {
                commands.push(format!("scoreboard players set {} temp 0", assign.target));
                commands.push(format!(
                    "execute {} run scoreboard players set {} temp 1",
                    condition_execute_args, assign.target
                ));
            }
            return Ok(());
        }

        // Check if we need to use the complex expression evaluator
        // Do this before borrowing to avoid borrow checker issues
        // 4. Handle Complex Expressions (Math, Binary, etc.)
        let needs_complex_eval = match &assign.value {
            Expression::Binary(left, _, right) => {
                // Simple binary (atom op atom) handled by optimized code in assignment.rs
                // Complex binary (nested expressions) need the expression evaluator
                !matches!(
                    (&**left, &**right),
                    (
                        Expression::Identifier(_) | Expression::Number(_),
                        Expression::Identifier(_) | Expression::Number(_)
                    )
                )
            }
            Expression::Unary(_, _) | Expression::Call(_, _) => true,
            // Attribute/Subscript falling through here means they failed storage resolution
            // which likely means invalid base. `evaluate_expression_to_target` will handle error reporting.
            Expression::Attribute(_, _) | Expression::Subscript(_, _) => true,
            _ => false,
        };

        if needs_complex_eval {
            // Handle complex nested expressions
            self.data_pack.track_objective("temp");
            self.variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            self.scoreboard_variables.insert(assign.target.clone());
            let expr_commands =
                self.evaluate_expression_to_target(&assign.value, &assign.target)?;

            if let Some(ref mut commands) = self.current_function {
                commands.extend(expr_commands);
            }
            return Ok(());
        }

        // Check for Storage assignments (Array, Map, String)
        let storage_assignment = matches!(
            &assign.value,
            Expression::Array(_) | Expression::Map(_) | Expression::String(_)
        );

        if storage_assignment {
            let storage_path = format!("vars.{}", assign.target);
            self.variable_storage_paths
                .insert(assign.target.clone(), storage_path.clone());

            let snbt = self.serialize_to_snbt(&assign.value)?;

            let cmd = format!(
                "data modify storage {}:global {} set value {}",
                self.data_pack.namespace, storage_path, snbt
            );

            if let Some(ref mut commands) = self.current_function {
                commands.push(cmd);
            }
            return Ok(());
        }

        // If it's a score assignment, generate scoreboard command
        if let Some(ref mut commands) = self.current_function {
            match &assign.value {
                Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                    // Handled above
                    return Ok(());
                }
                Expression::Boolean(b) => {
                    // Boolean assignment
                    // Store as scoreboard (0/1) AND storage (byte) if needed?
                    // For now, keep existing scoreboard behavior for boolean
                    // But if we want to put it in storage:
                    // Let's stick to scoreboard for consistency with existing code
                    // unless explicitly requested?
                    // Existing code does NOTHING for Boolean assignment!
                    // See original code: "String and Boolean values ... don't generate scoreboard commands"
                    // This means they were effectively compile-time only or broken.
                    // Let's fix it by using scoreboard for boolean.
                    self.data_pack.track_objective("temp");
                    self.variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.scoreboard_variables.insert(assign.target.clone());
                    let val = if *b { 1 } else { 0 };
                    commands.push(format!(
                        "scoreboard players set {} temp {}",
                        assign.target, val
                    ));
                }
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
                    // Check if source is a storage variable (Array/Map)
                    let src_path_opt = self.variable_storage_paths.get(var).cloned();
                    if let Some(src_path) = src_path_opt {
                        // Storage copy: target = src
                        // Mark target as storage variable
                        let target_path = format!("vars.{}", assign.target);
                        self.variable_storage_paths
                            .insert(assign.target.clone(), target_path.clone());

                        let namespace = self.data_pack.namespace.clone();
                        commands.push(format!(
                            "data modify storage {}:global {} set from storage {}:global {}",
                            namespace, target_path, namespace, src_path
                        ));
                    } else {
                        // Variable-to-variable assignment (Scoreboard)
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
                            if n.fract() != 0.0 {
                                eprintln!(
                                    "⚠️  Warning: Float value {} in binary operation will lose precision.\n\
                                    Scoreboard only supports integers. Fractional part will be truncated to: {}",
                                    n, *n as i32
                                );
                            }
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
                                    if value < 0 {
                                        commands.push(format!(
                                            "scoreboard players remove {} temp {}",
                                            assign.target, -value
                                        ));
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players add {} temp {}",
                                            assign.target, value
                                        ));
                                    }
                                }
                                BinaryOp::Sub => {
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    if value < 0 {
                                        commands.push(format!(
                                            "scoreboard players add {} temp {}",
                                            assign.target, -value
                                        ));
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players remove {} temp {}",
                                            assign.target, value
                                        ));
                                    }
                                }
                                BinaryOp::Mul => {
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players set #multiplier temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp *= #multiplier temp",
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
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players set #divisor temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp /= #divisor temp",
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
                                    // Optimization: Skip self-assignment if target == var
                                    if assign.target != *var || var_obj != "temp" {
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var, var_obj
                                        ));
                                    }
                                    commands.push(format!(
                                        "scoreboard players set #modulus temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp %= #modulus temp",
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
                                    // Limit maximum exponent to prevent excessive command generation
                                    const MAX_POWER_EXPONENT: i32 = 100;
                                    if value > MAX_POWER_EXPONENT {
                                        return Err(format!(
                                            "Power exponent too large: {} > {}.\n\
                                            \n\
                                            Large exponents generate {} multiplication commands, which is excessive.\n\
                                            Solution: Use a smaller exponent or implement iterative multiplication:\n\
                                            \n\
                                            result = 1\n\
                                            for i in range({}):\n\
                                                result = result * base",
                                            value, MAX_POWER_EXPONENT, value - 1, value
                                        ));
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
                                            commands.push(format!(
                                                "scoreboard players operation #power_base temp = {} temp",
                                                assign.target
                                            ));
                                            for _ in 0..(value - 1) {
                                                commands.push(format!(
                                                    "scoreboard players operation {} temp *= #power_base temp",
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
                            let result_f64 = match op {
                                BinaryOp::Add => *n1 + *n2,
                                BinaryOp::Sub => *n1 - *n2,
                                BinaryOp::Mul => *n1 * *n2,
                                BinaryOp::Div => {
                                    if *n2 == 0.0 {
                                        return Err(format!(
                                            "Division by zero in constant expression: {} / {}",
                                            n1, n2
                                        ));
                                    }
                                    *n1 / *n2
                                }
                                BinaryOp::Mod => {
                                    if *n2 == 0.0 {
                                        return Err(format!(
                                            "Modulo by zero in constant expression: {} % {}",
                                            n1, n2
                                        ));
                                    }
                                    (*n1 as i32 % *n2 as i32) as f64
                                }
                                BinaryOp::Pow => {
                                    let base = *n1 as i32;
                                    let exp = *n2 as i32;
                                    if exp < 0 {
                                        return Err(
                                            "Power exponent must be non-negative".to_string()
                                        );
                                    }
                                    match base.checked_pow(exp as u32) {
                                        Some(result) => result as f64,
                                        None => {
                                            eprintln!(
                                                "⚠️  Warning: Power operation {}^{} overflows i32, clamping to i32::MAX",
                                                base, exp
                                            );
                                            i32::MAX as f64
                                        }
                                    }
                                }
                                _ => 0.0,
                            };

                            let result = result_f64 as i32;
                            if result_f64.fract() != 0.0 {
                                eprintln!(
                                    "⚠️  Warning: Constant expression result {} will lose precision.\n\
                                    Scoreboard only supports integers. Fractional part will be truncated to: {}",
                                    result_f64, result
                                );
                            }

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
                            if n.fract() != 0.0 {
                                eprintln!(
                                    "⚠️  Warning: Float value {} in binary operation will lose precision.\n\
                                    Scoreboard only supports integers. Fractional part will be truncated to: {}",
                                    n, *n as i32
                                );
                            }
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
                                    if value < 0 {
                                        commands.push(format!(
                                            "scoreboard players remove {} temp {}",
                                            assign.target, -value
                                        ));
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players add {} temp {}",
                                            assign.target, value
                                        ));
                                    }
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
                                    commands.push(format!(
                                        "scoreboard players operation {} temp = {} {}",
                                        assign.target, var, var_obj
                                    ));
                                    commands.push(format!(
                                        "scoreboard players set #multiplier temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} temp *= #multiplier temp",
                                        assign.target
                                    ));
                                }
                                BinaryOp::Div => {
                                    // score = value / var (not commonly used, but implemented)
                                    // Check if var is a compile-time constant with value 0
                                    if let Some(const_val) = self.compile_time_constants.get(var) {
                                        if *const_val == 0.0 {
                                            return Err(format!(
                                                "Division by zero: Variable '{}' has constant value 0.\n\
                                                \n\
                                                Division by zero causes undefined behavior in Minecraft.\n\
                                                Solution: Check the divisor before division:\n\
                                                \n\
                                                if {} != 0:\n\
                                                    {} = {} / {}",
                                                var, var, assign.target, value, var
                                            ));
                                        }
                                    }
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
                                    // Check if var is a compile-time constant with value 0
                                    if let Some(const_val) = self.compile_time_constants.get(var) {
                                        if *const_val == 0.0 {
                                            return Err(format!(
                                                "Modulo by zero: Variable '{}' has constant value 0.\n\
                                                \n\
                                                Modulo by zero causes undefined behavior in Minecraft.\n\
                                                Solution: Check the divisor before modulo:\n\
                                                \n\
                                                if {} != 0:\n\
                                                    {} = {} % {}",
                                                var, var, assign.target, value, var
                                            ));
                                        }
                                    }
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
                                        commands.push(format!(
                                            "scoreboard players operation {} temp = {} {}",
                                            assign.target, var2, var_obj
                                        ));
                                        commands.push(format!(
                                            "scoreboard players set #multiplier temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp *= #multiplier temp",
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
                                        if num < 0 {
                                            commands.push(format!(
                                                "scoreboard players remove {} temp {}",
                                                assign.target, -num
                                            ));
                                        } else {
                                            commands.push(format!(
                                                "scoreboard players add {} temp {}",
                                                assign.target, num
                                            ));
                                        }
                                    }
                                    BinaryOp::Sub => {
                                        if num < 0 {
                                            commands.push(format!(
                                                "scoreboard players add {} temp {}",
                                                assign.target, -num
                                            ));
                                        } else {
                                            commands.push(format!(
                                                "scoreboard players remove {} temp {}",
                                                assign.target, num
                                            ));
                                        }
                                    }
                                    BinaryOp::Mul => {
                                        commands.push(format!(
                                            "scoreboard players set #multiplier temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp *= #multiplier temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Div => {
                                        if num == 0 {
                                            return Err(
                                                "Division by zero in assignment".to_string()
                                            );
                                        }
                                        commands.push(format!(
                                            "scoreboard players set #divisor temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp /= #divisor temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Mod => {
                                        if num == 0 {
                                            return Err("Modulo by zero in assignment".to_string());
                                        }
                                        commands.push(format!(
                                            "scoreboard players set #modulus temp {}",
                                            num
                                        ));
                                        commands.push(format!(
                                            "scoreboard players operation {} temp %= #modulus temp",
                                            assign.target
                                        ));
                                    }
                                    BinaryOp::Pow => {
                                        if num < 0 {
                                            return Err(
                                                "Power exponent must be non-negative".to_string()
                                            );
                                        }
                                        // Limit maximum exponent to prevent excessive command generation
                                        const MAX_POWER_EXPONENT: i32 = 100;
                                        if num > MAX_POWER_EXPONENT {
                                            return Err(format!(
                                                "Power exponent too large: {} > {}.\n\
                                                \n\
                                                Large exponents generate {} multiplication commands, which is excessive.\n\
                                                Solution: Use a smaller exponent or implement iterative multiplication in a loop.",
                                                num, MAX_POWER_EXPONENT, num - 1
                                            ));
                                        }
                                        if num == 0 {
                                            commands.push(format!(
                                                "scoreboard players set {} temp 1",
                                                assign.target
                                            ));
                                        } else if num == 1 {
                                            // nothing to do
                                        } else {
                                            commands.push(format!(
                                                "scoreboard players operation #power_base temp = {} temp",
                                                assign.target
                                            ));
                                            for _ in 0..(num - 1) {
                                                commands.push(format!(
                                                    "scoreboard players operation {} temp *= #power_base temp",
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
                                        if let Some(const_val) =
                                            self.compile_time_constants.get(var2)
                                        {
                                            if *const_val == 0.0 {
                                                return Err(format!(
                                                    "Division by zero: Variable '{}' has constant value 0.\n\
                                                    \n\
                                                    Division by zero causes undefined behavior in Minecraft.\n\
                                                    Solution: Check the divisor before division:\n\
                                                    \n\
                                                    if {} != 0:\n\
                                                        {} = {} / {}",
                                                    var2, var2, assign.target, assign.target, var2
                                                ));
                                            }
                                        }
                                        commands.push(format!(
                                            "scoreboard players operation {} temp /= {} {}",
                                            assign.target, var2, var2_obj
                                        ));
                                    }
                                    BinaryOp::Mod => {
                                        // Check if var2 is a compile-time constant with value 0
                                        if let Some(const_val) =
                                            self.compile_time_constants.get(var2)
                                        {
                                            if *const_val == 0.0 {
                                                return Err(format!(
                                                    "Modulo by zero: Variable '{}' has constant value 0.\n\
                                                    \n\
                                                    Modulo by zero causes undefined behavior in Minecraft.\n\
                                                    Solution: Check the divisor before modulo:\n\
                                                    \n\
                                                    if {} != 0:\n\
                                                        {} = {} % {}",
                                                    var2, var2, assign.target, assign.target, var2
                                                ));
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
                Expression::Call(func, _args) => {
                    // Function calls in assignments are not supported
                    let func_name = match &**func {
                        Expression::Identifier(name) => name.clone(),
                        Expression::Attribute(obj, method) => {
                            if let Expression::Identifier(module) = &**obj {
                                format!("{}.{}", module, method)
                            } else {
                                "unknown".to_string()
                            }
                        }
                        _ => "unknown".to_string(),
                    };

                    return Err(format!(
                        "Cannot assign function call result to variable.\n\n\
                        Attempted: {} = {}()\n\n\
                        Minecraft functions don't return values that can be assigned to variables.\n\n\
                        Solutions:\n\
                        1. Call the function separately (not in an assignment):\n\
                           {}()\n\
                           # Then use scoreboard operations to track state\n\n\
                        2. If you need to track state, use scoreboard variables:\n\
                           # In the called function:\n\
                           def {}():\n\
                               result = 42  # This sets a scoreboard value\n\
                           \n\
                           # In the caller:\n\
                           {}()\n\
                           # Now 'result' variable can be used\n\n\
                        3. Pass the target variable name as a parameter:\n\
                           def set_value(target_var):\n\
                               /scoreboard players set {{target_var}} temp 42\n\
                           \n\
                           set_value(\"my_var\")",
                        assign.target, func_name, func_name, func_name, func_name
                    ));
                }
                Expression::None => {
                    // None value
                    return Err(format!(
                        "Cannot assign None/null to variable '{}'.\n\n\
                        Minecraft scoreboards require numeric values.\n\n\
                        Solutions:\n\
                        1. Use 0 to represent 'no value':\n\
                           {} = 0\n\n\
                        2. Use -1 to represent 'unset':\n\
                           {} = -1\n\n\
                        3. Check for a specific value before using:\n\
                           if {} != -1:\n\
                               # value is set",
                        assign.target, assign.target, assign.target, assign.target
                    ));
                }
                _ => {
                    // Catch-all for any other unsupported expression types
                    return Err(format!(
                        "Unsupported expression type in assignment to variable '{}'.\n\n\
                        Expression type: {:?}\n\n\
                        Supported assignment types:\n\
                        - Numbers: x = 10\n\
                        - Variables: x = y\n\
                        - Arithmetic: x = y + z * 2\n\
                        - Unary operations: x = -y\n\
                        - Strings (in commands only): message = \"text\" (use in tellraw)\n\
                        - Booleans (in commands only): flag = True (use in tellraw)\n\n\
                        Not supported in assignments:\n\
                        - Function calls: x = func() (call separately)\n\
                        - Attribute access: x = obj.field\n\
                        - Subscripts: x = arr[0]\n\
                        - None/null values",
                        assign.target, assign.value
                    ));
                }
            }
        }

        Ok(())
    }

    fn is_runtime_condition_expression(expr: &Expression) -> bool {
        match expr {
            Expression::Binary(_, op, _) => matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::And
                    | BinaryOp::Or
            ),
            Expression::Unary(UnaryOp::Not, _) => true,
            _ => false,
        }
    }

    pub(in crate::transpiler) fn serialize_to_snbt(
        &self,
        expr: &Expression,
    ) -> Result<String, String> {
        match expr {
            Expression::Number(n) => {
                if n.fract() == 0.0 {
                    Ok(format!("{}", *n as i32))
                } else {
                    Ok(format!("{}f", n))
                }
            }
            Expression::String(s) => {
                let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
                Ok(format!("\"{}\"", escaped))
            }
            Expression::Boolean(b) => Ok(if *b {
                "1b".to_string()
            } else {
                "0b".to_string()
            }),
            Expression::None => Err(
                "None/null values are not supported in Minecraft SNBT storage literals because NBT has no null type. Use a data pack JSON resource helper when you need JSON null, or choose an explicit sentinel value for storage.".to_string(),
            ),
            Expression::Array(items) => {
                let mut result = String::from("[");
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        result.push(',');
                    }
                    result.push_str(&self.serialize_to_snbt(item)?);
                }
                result.push(']');
                Ok(result)
            }
            Expression::Map(entries) => {
                let mut result = String::from("{");
                for (i, (key, value)) in entries.iter().enumerate() {
                    if i > 0 {
                        result.push(',');
                    }
                    result.push_str(&Self::serialize_snbt_key(key));
                    result.push(':');
                    result.push_str(&self.serialize_to_snbt(value)?);
                }
                result.push('}');
                Ok(result)
            }
            Expression::Identifier(name) => {
                if let Some(val) = self.compile_time_constants.get(name) {
                    Ok(format!("{}", val))
                } else {
                    Err(format!("Variables inside array/map literals are not yet supported in this context ('{}'). Use constants or literal values.", name))
                }
            }
            _ => Err(format!(
                "Unsupported expression in SNBT serialization: {:?}",
                expr
            )),
        }
    }

    fn serialize_snbt_key(key: &str) -> String {
        if !key.is_empty()
            && key
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '+')
        {
            return key.to_string();
        }

        let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
        format!("\"{}\"", escaped)
    }
}
