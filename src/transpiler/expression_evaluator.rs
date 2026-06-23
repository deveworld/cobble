use crate::ast::{BinaryOp, Expression, UnaryOp};
use crate::transpiler::data_pack::DataPack;
use std::collections::HashMap;

/// Evaluate expressions and translate conditions for Minecraft commands
pub struct ExpressionEvaluator<'a> {
    pub data_pack: &'a mut DataPack,
    pub variable_objectives: &'a HashMap<String, String>,
    pub variable_storage_paths: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
}

impl<'a> ExpressionEvaluator<'a> {
    pub fn new(
        data_pack: &'a mut DataPack,
        variable_objectives: &'a HashMap<String, String>,
        variable_storage_paths: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
    ) -> Self {
        Self {
            data_pack,
            variable_objectives,
            variable_storage_paths,
            compile_time_constants,
        }
    }

    fn resolve_storage_path(&self, expr: &Expression) -> Option<String> {
        match expr {
            Expression::Identifier(name) => self.variable_storage_paths.get(name).cloned(),
            Expression::Attribute(obj, attr) => {
                let base_path = self.resolve_storage_path(obj)?;
                Some(format!("{}.{}", base_path, attr))
            }
            Expression::Subscript(obj, index) => {
                let base_path = self.resolve_storage_path(obj)?;
                match &**index {
                    Expression::Number(n) => Some(format!("{}[{}]", base_path, *n as i32)),
                    Expression::String(s) => Some(format!("{}.{}", base_path, s)),
                    _ => None, // Dynamic index not supported in path resolution yet
                }
            }
            _ => None,
        }
    }

    fn target_objective(&self, target: &str) -> &str {
        self.variable_objectives
            .get(target)
            .map(String::as_str)
            .unwrap_or_else(|| {
                if target.starts_with("#math_") {
                    "math"
                } else {
                    "temp"
                }
            })
    }

    /// Helper method to evaluate a complex expression into a target variable
    #[allow(clippy::single_match)]
    pub fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String> {
        let mut commands = Vec::new();
        let target_obj = self.target_objective(target).to_string();
        self.data_pack.track_objective("temp");
        self.data_pack.track_objective(&target_obj);

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
                    "scoreboard players set {} {} {}",
                    target, target_obj, *n as i32
                ));
            }
            Expression::Identifier(var) => {
                if let Some(const_value) = self.compile_time_constants.get(var) {
                    let truncated = *const_value as i32;
                    commands.push(format!(
                        "scoreboard players set {} {} {}",
                        target, target_obj, truncated
                    ));
                } else if let Some(storage_path) = self.variable_storage_paths.get(var) {
                    // Load from storage to scoreboard
                    let namespace = &self.data_pack.namespace;
                    commands.push(format!(
                        "execute store result score {} {} run data get storage {}:global {}",
                        target, target_obj, namespace, storage_path
                    ));
                } else {
                    let var_obj = self
                        .variable_objectives
                        .get(var)
                        .unwrap_or(&"temp".to_string())
                        .clone();
                    // Skip no-op self-assignment (e.g. target temp = target temp)
                    if !(var == target && var_obj == target_obj) {
                        commands.push(format!(
                            "scoreboard players operation {} {} = {} {}",
                            target, target_obj, var, var_obj
                        ));
                    }
                }
            }
            Expression::Call(func, args) => {
                // Handle intrinsics like math.sqrt
                match &**func {
                    Expression::Attribute(obj, method) => {
                        if let Expression::Identifier(module) = &**obj {
                            if module == "math" {
                                let op = method.as_str();
                                match op {
                                    "sqrt" | "abs" => {
                                        if args.len() != 1 {
                                            return Err(format!("math.{} takes 1 argument", op));
                                        }
                                        let input_var = "#math_input";
                                        self.data_pack.track_objective("math");

                                        let eval_cmds = self
                                            .evaluate_expression_to_target(&args[0], input_var)?;
                                        commands.extend(eval_cmds);

                                        self.data_pack.ensure_math_helper(op);
                                        commands.push(format!(
                                            "function {}:_cobble_math_{}",
                                            self.data_pack.namespace, op
                                        ));

                                        self.data_pack.track_objective("temp");
                                        commands.push(format!("scoreboard players operation {} temp = #math_result math", target));
                                        return Ok(commands);
                                    }
                                    "min" | "max" => {
                                        if args.len() != 2 {
                                            return Err(format!("math.{} takes 2 arguments", op));
                                        }
                                        let input1 = "#math_input";
                                        let input2 = "#math_input2";
                                        self.data_pack.track_objective("math");

                                        let mut eval_cmds =
                                            self.evaluate_expression_to_target(&args[0], input1)?;
                                        eval_cmds.extend(
                                            self.evaluate_expression_to_target(&args[1], input2)?,
                                        );
                                        commands.extend(eval_cmds);

                                        self.data_pack.ensure_math_helper(op);
                                        commands.push(format!(
                                            "function {}:_cobble_math_{}",
                                            self.data_pack.namespace, op
                                        ));

                                        self.data_pack.track_objective("temp");
                                        commands.push(format!("scoreboard players operation {} temp = #math_result math", target));
                                        return Ok(commands);
                                    }
                                    _ => return Err(format!("Unknown math function: {}", op)),
                                }
                            }
                        }
                    }
                    _ => {}
                }
                return Err(
                    "Function calls in expressions are not supported (except math intrinsics)."
                        .to_string(),
                );
            }
            Expression::Subscript(base, index) => {
                // Handle array/map access: base[index]
                if let Some(storage_path) = self.resolve_storage_path(base) {
                    // 3. Evaluate index
                    let index_str = match &**index {
                        Expression::Number(n) => format!("[{}]", *n as i32),
                        Expression::String(s) => format!(".{}", s),
                        Expression::Identifier(name) => {
                            if let Some(val) = self.compile_time_constants.get(name) {
                                format!("[{}]", *val as i32)
                            } else {
                                return Err(format!("Dynamic index '{}' in subscript is not yet supported. Use a literal or constant.", name));
                            }
                        }
                        _ => return Err("Unsupported index type".to_string()),
                    };

                    let full_path = format!("{}{}", storage_path, index_str);
                    let namespace = &self.data_pack.namespace;

                    commands.push(format!(
                        "execute store result score {} {} run data get storage {}:global {}",
                        target, target_obj, namespace, full_path
                    ));
                } else {
                    return Err(
                        "Subscript base must resolve to a storage path (list/map).".to_string()
                    );
                }
            }
            Expression::Attribute(obj, attr) => {
                // Handle attribute access as map access: obj.attr
                if let Some(storage_path) = self.resolve_storage_path(obj) {
                    let full_path = format!("{}.{}", storage_path, attr);
                    let namespace = &self.data_pack.namespace;
                    commands.push(format!(
                        "execute store result score {} {} run data get storage {}:global {}",
                        target, target_obj, namespace, full_path
                    ));
                } else {
                    return Err("Attribute base must resolve to a storage path".to_string());
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
                                if value < 0 {
                                    commands.push(format!(
                                        "scoreboard players remove {} {} {}",
                                        target, target_obj, -value
                                    ));
                                } else {
                                    commands.push(format!(
                                        "scoreboard players add {} {} {}",
                                        target, target_obj, value
                                    ));
                                }
                            }
                            BinaryOp::Sub => {
                                if value < 0 {
                                    commands.push(format!(
                                        "scoreboard players add {} {} {}",
                                        target, target_obj, -value
                                    ));
                                } else {
                                    commands.push(format!(
                                        "scoreboard players remove {} {} {}",
                                        target, target_obj, value
                                    ));
                                }
                            }
                            BinaryOp::Mul => {
                                commands.push(format!(
                                    "scoreboard players set #multiplier temp {}",
                                    value
                                ));
                                commands.push(format!(
                                    "scoreboard players operation {} {} *= #multiplier temp",
                                    target, target_obj
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
                                commands.push(format!(
                                    "scoreboard players set #divisor temp {}",
                                    value
                                ));
                                commands.push(format!(
                                    "scoreboard players operation {} {} /= #divisor temp",
                                    target, target_obj
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
                                commands.push(format!(
                                    "scoreboard players set #modulus temp {}",
                                    value
                                ));
                                commands.push(format!(
                                    "scoreboard players operation {} {} %= #modulus temp",
                                    target, target_obj
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
                                    commands.push(format!(
                                        "scoreboard players set {} {} 1",
                                        target, target_obj
                                    ));
                                } else if value == 1 {
                                    // x^1 = x, no operation needed
                                } else {
                                    // Store original value for multiplication
                                    commands.push(format!(
                                        "scoreboard players operation #power_base temp = {} {}",
                                        target, target_obj
                                    ));
                                    // Multiply (value - 1) times
                                    for _ in 0..(value - 1) {
                                        commands.push(format!(
                                            "scoreboard players operation {} {} *= #power_base temp",
                                            target, target_obj
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
                                    if value < 0 {
                                        commands.push(format!(
                                            "scoreboard players remove {} {} {}",
                                            target, target_obj, -value
                                        ));
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players add {} {} {}",
                                            target, target_obj, value
                                        ));
                                    }
                                }
                                BinaryOp::Sub => {
                                    if value < 0 {
                                        commands.push(format!(
                                            "scoreboard players add {} {} {}",
                                            target, target_obj, -value
                                        ));
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players remove {} {} {}",
                                            target, target_obj, value
                                        ));
                                    }
                                }
                                BinaryOp::Mul => {
                                    commands.push(format!(
                                        "scoreboard players set #multiplier temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} {} *= #multiplier temp",
                                        target, target_obj
                                    ));
                                }
                                BinaryOp::Div => {
                                    if value == 0 {
                                        return Err("Division by zero in expression".to_string());
                                    }
                                    commands.push(format!(
                                        "scoreboard players set #divisor temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} {} /= #divisor temp",
                                        target, target_obj
                                    ));
                                }
                                BinaryOp::Mod => {
                                    if value == 0 {
                                        return Err("Modulo by zero in expression".to_string());
                                    }
                                    commands.push(format!(
                                        "scoreboard players set #modulus temp {}",
                                        value
                                    ));
                                    commands.push(format!(
                                        "scoreboard players operation {} {} %= #modulus temp",
                                        target, target_obj
                                    ));
                                }
                                BinaryOp::Pow => {
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
                                            Solution: Use a smaller exponent or implement iterative multiplication in a loop.",
                                            value, MAX_POWER_EXPONENT, value - 1
                                        ));
                                    }
                                    if value == 0 {
                                        commands.push(format!(
                                            "scoreboard players set {} {} 1",
                                            target, target_obj
                                        ));
                                    } else if value == 1 {
                                        // x^1 = x, nothing to do (target already holds left side)
                                    } else {
                                        commands.push(format!(
                                            "scoreboard players operation #power_base temp = {} {}",
                                            target, target_obj
                                        ));
                                        for _ in 0..(value - 1) {
                                            commands.push(format!(
                                                "scoreboard players operation {} {} *= #power_base temp",
                                                target, target_obj
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
                                        "scoreboard players operation {} {} += {} {}",
                                        target, target_obj, var, var_obj
                                    ));
                                }
                                BinaryOp::Sub => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} -= {} {}",
                                        target, target_obj, var, var_obj
                                    ));
                                }
                                BinaryOp::Mul => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} *= {} {}",
                                        target, target_obj, var, var_obj
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
                                        "scoreboard players operation {} {} /= {} {}",
                                        target, target_obj, var, var_obj
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
                                        "scoreboard players operation {} {} %= {} {}",
                                        target, target_obj, var, var_obj
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
                            self.evaluate_expression_to_target(right, "#expr_temp")?;
                        commands.extend(right_commands);

                        // Now perform the operation
                        match op {
                            BinaryOp::Add => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} += #expr_temp temp",
                                    target, target_obj
                                ));
                            }
                            BinaryOp::Sub => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} -= #expr_temp temp",
                                    target, target_obj
                                ));
                            }
                            BinaryOp::Mul => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} *= #expr_temp temp",
                                    target, target_obj
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
                                    "scoreboard players operation {} {} /= #expr_temp temp",
                                    target, target_obj
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
                                    "scoreboard players operation {} {} %= #expr_temp temp",
                                    target, target_obj
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

                        // Multiply by -1 using temp objective fake player
                        commands.push("scoreboard players set #neg_const temp -1".to_string());
                        commands.push(format!(
                            "scoreboard players operation {} {} *= #neg_const temp",
                            target, target_obj
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
                }
            }
            _ => {
                return Err("Unsupported expression type".to_string());
            }
        }

        Ok(commands)
    }
}
