use super::expression_evaluator::ExpressionEvaluator;
use super::storage_paths::{StoragePathOps, StorageValueOps};
use super::{TranspileError, Transpiler};
use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};

pub(in crate::transpiler) fn numeric_literal(expr: &Expression) -> Option<f64> {
    match expr {
        Expression::Number(value) => Some(*value),
        Expression::Boolean(value) => Some(f64::from(*value)),
        Expression::Unary(UnaryOp::Neg, inner) => numeric_literal(inner).map(|value| -value),
        Expression::Unary(UnaryOp::Pos, inner) => numeric_literal(inner),
        Expression::Call(func, args)
            if matches!(&**func, Expression::Identifier(name) if name == "_cobble_float")
                && !args.is_empty() =>
        {
            numeric_literal(&args[0])
        }
        _ => None,
    }
}

pub(in crate::transpiler) fn numeric_computation(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Binary(
            _,
            BinaryOp::Add
                | BinaryOp::Sub
                | BinaryOp::Mul
                | BinaryOp::Div
                | BinaryOp::FloorDiv
                | BinaryOp::Mod
                | BinaryOp::Pow,
            _
        ) | Expression::Unary(UnaryOp::Neg | UnaryOp::Pos, _)
            | Expression::Call(..)
    )
}

pub(in crate::transpiler) fn helper_uses_storage_numbers(name: &str) -> bool {
    matches!(
        name,
        "_cobble_list_sum"
            | "_cobble_list_min"
            | "_cobble_list_max"
            | "_cobble_list_sort"
            | "_cobble_list_index"
            | "_cobble_list_count"
            | "_cobble_list_remove"
            | "_cobble_value_add"
            | "_cobble_value_mul"
    )
}

impl Transpiler {
    pub(in crate::transpiler) fn uses_storage_numbers(&self) -> bool {
        !self.in_prelude
            || self
                .current_function_name
                .as_deref()
                .is_some_and(helper_uses_storage_numbers)
    }

    pub(in crate::transpiler) fn numeric_value_commands(
        &mut self,
        path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let numeric_backend = self.uses_storage_numbers();
        if numeric_backend {
            self.prepare_numeric_expression(expr)?;
        }
        let recursive = self.in_recursive_function();
        let mut evaluator = ExpressionEvaluator {
            data_pack: &mut self.data_pack,
            symbols: &self.symbols,
            functions: &mut self.functions,
            current_params: &self.current_context.params,
            temp_counter: &mut self.temp_counter,
            source: self.current_statement_source.clone(),
            caller_recursive: recursive,
            warnings: &self.warnings,
            numeric_scope: self.current_function_name.clone(),
            numeric_locals: self.current_context.locals.to_vec(),
            numeric_backend,
        };
        evaluator.numeric_value_commands(path, expr)
    }

    fn may_hold_double(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Number(_) | Expression::Boolean(_) => false,
            Expression::Identifier(name) => {
                !name.starts_with('@')
                    && !name.starts_with('#')
                    && !name.starts_with("__cobble_cond_operand_")
                    && (self.current_context.is_param(name)
                        || (self
                            .real_variables
                            .contains(&self.variable_storage_path(name))
                            && self.symbols.variable_types.get(name) != Some(&CobbleType::Float))
                        || matches!(
                            self.symbols.variable_types.get(name),
                            Some(CobbleType::Double | CobbleType::Unknown) | None
                        ))
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if matches!(name.as_str(), "float" | "_cobble_float" | "int" | "len") && !self.functions.params.contains_key(name)) => {
                false
            }
            Expression::Call(..) | Expression::Subscript(..) | Expression::Attribute(..) => true,
            Expression::Binary(left, _, right) => {
                self.may_hold_double(left) || self.may_hold_double(right)
            }
            Expression::Unary(_, inner) => self.may_hold_double(inner),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn prepare_numeric_expression(
        &mut self,
        expr: &Expression,
    ) -> Result<(), TranspileError> {
        let worker = match expr {
            Expression::Binary(left, op, right)
                if self.may_hold_double(left) || self.may_hold_double(right) =>
            {
                match op {
                    BinaryOp::Add => Some("add"),
                    BinaryOp::Sub => Some("sub"),
                    BinaryOp::Mul => Some("mul"),
                    BinaryOp::Div => Some("div"),
                    BinaryOp::FloorDiv => Some("floor_div"),
                    BinaryOp::Mod => Some("mod"),
                    BinaryOp::Pow => Some("pow"),
                    _ => Some("compare"),
                }
            }
            Expression::Unary(UnaryOp::Neg, inner) if self.may_hold_double(inner) => Some("mul"),
            _ => None,
        };
        if let Some(worker) = worker {
            self.require_prelude(&format!("_cobble_numeric_double_{worker}"))?;
        }
        if let Expression::Call(func, args) = expr {
            let name = match &**func {
                Expression::Identifier(name) if !self.functions.params.contains_key(name) => {
                    Some(name.as_str())
                }
                Expression::Attribute(module, method) if matches!(&**module, Expression::Identifier(name) if name == "math") => {
                    Some(method.as_str())
                }
                _ => None,
            };
            if let Some(name) = name {
                if matches!(name, "float" | "_cobble_float" | "double" | "int") {
                    if let [arg] = args.as_slice() {
                        let literal = numeric_literal(arg).or_else(|| match arg {
                            Expression::Identifier(name) => {
                                self.symbols.compile_time_constants.get(name).copied()
                            }
                            Expression::String(text) => text.parse::<f64>().ok(),
                            _ => None,
                        });
                        if literal.is_none() {
                            let name = if name == "_cobble_float" {
                                "float"
                            } else {
                                name
                            };
                            self.require_prelude(&format!("_cobble_numeric_double_cast_{name}"))?;
                        }
                    }
                } else if args.iter().any(|arg| self.may_hold_double(arg)) {
                    match name {
                        "sqrt" => self.require_prelude("_cobble_numeric_double_sqrt")?,
                        "abs" => {
                            self.require_prelude("_cobble_numeric_double_compare")?;
                            self.require_prelude("_cobble_numeric_double_mul")?;
                        }
                        "min" | "max" => {
                            self.require_prelude("_cobble_numeric_double_compare")?;
                        }
                        _ => {}
                    }
                }
            }
        }
        for child in super::desugar::sub_expressions(expr) {
            self.prepare_numeric_expression(child)?;
        }
        Ok(())
    }

    pub(in crate::transpiler) fn record_potential_real_globals(
        &mut self,
        program: &crate::ast::Program,
    ) {
        if self.in_prelude {
            return;
        }
        for statement in &program.statements {
            if let crate::ast::Statement::FunctionDef(function) = statement {
                for (name, value) in super::function_context::global_assignments(function) {
                    if self.may_hold_float(&value) {
                        self.real_variables.insert(format!("vars.{name}"));
                    }
                }
            }
        }
    }
}

impl ExpressionEvaluator<'_> {
    fn numeric_temporary(&mut self) -> String {
        let id = self.next_temp_id();
        match self.numeric_scope.as_deref() {
            Some(function) => format!("locals.{function}._cobble_number_{id}"),
            None => format!("_cobble_number_{id}"),
        }
    }

    pub(in crate::transpiler) fn numeric_value_commands(
        &mut self,
        path: &str,
        expr: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let ns = self.data_pack.namespace.clone();
        if let Expression::Call(func, args) = expr {
            if let Expression::Attribute(module, method) = &**func {
                if matches!(&**module, Expression::Identifier(name) if name == "math") {
                    if matches!(method.as_str(), "abs" | "min" | "max") {
                        return self.numeric_value_commands(
                            path,
                            &Expression::Call(
                                Box::new(Expression::Identifier(method.clone())),
                                args.clone(),
                            ),
                        );
                    }
                    if method == "sqrt" {
                        let [arg] = args.as_slice() else {
                            return Err("math.sqrt() takes one value.".into());
                        };
                        let input = self.numeric_temporary();
                        let mut commands = self.numeric_value_commands(&input, arg)?;
                        super::numeric_runtime::add_functions(self.data_pack);
                        self.data_pack.ensure_math_helper("sqrt");
                        commands.extend([
                            format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {input}"),
                            format!("data modify storage {ns}:global _cobble_numeric.right set value 0"),
                            format!("function {ns}:_cobble_numeric_sqrt"),
                            format!("data modify storage {ns}:global {path} set from storage {ns}:global _cobble_numeric.result"),
                        ]);
                        return Ok(commands);
                    }
                }
            }
            if let Expression::Identifier(name) = &**func {
                if matches!(name.as_str(), "abs" | "min" | "max")
                    && !self.functions.params.contains_key(name)
                {
                    if args.is_empty() || (name == "abs" && args.len() != 1) {
                        return Err(format!("Invalid argument count for {name}().").into());
                    }
                    let mut commands = Vec::new();
                    let mut values = Vec::new();
                    for arg in args {
                        let value = self.numeric_temporary();
                        commands.extend(self.numeric_value_commands(&value, arg)?);
                        values.push(value);
                    }
                    let chosen = self.numeric_temporary();
                    commands.push(format!(
                        "data modify storage {ns}:global {chosen} set from storage {ns}:global {}",
                        values[0]
                    ));
                    super::numeric_runtime::add_functions(self.data_pack);
                    if name == "abs" {
                        commands.extend([
                            format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {chosen}"),
                            format!("data modify storage {ns}:global _cobble_numeric.right set value 0"),
                            format!("function {ns}:_cobble_numeric_compare"),
                            format!("execute store result score #numeric_abs temp run data get storage {ns}:global _cobble_numeric.result 1"),
                            format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {chosen}"),
                            format!("data modify storage {ns}:global _cobble_numeric.right set value -1"),
                            format!("data modify storage {ns}:global _cobble_numeric.sign_text set string storage {ns}:global {chosen} 0 1"),
                            format!("execute if data storage {ns}:global _cobble_numeric{{sign_text:\"-\"}} run scoreboard players set #numeric_abs temp -1"),
                            format!("execute if score #numeric_abs temp matches ..-1 run function {ns}:_cobble_numeric_mul"),
                            format!("execute if score #numeric_abs temp matches ..-1 run data modify storage {ns}:global {chosen} set from storage {ns}:global _cobble_numeric.result"),
                        ]);
                    } else {
                        for value in &values[1..] {
                            commands.extend([
                                format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {value}"),
                                format!("data modify storage {ns}:global _cobble_numeric.right set from storage {ns}:global {chosen}"),
                                format!("function {ns}:_cobble_numeric_compare"),
                                format!("execute store result score #numeric_select temp run data get storage {ns}:global _cobble_numeric.result 1"),
                                format!("execute if score #numeric_select temp matches {} run data modify storage {ns}:global {chosen} set from storage {ns}:global {value}", if name == "min" { "..-1" } else { "1.." }),
                            ]);
                        }
                    }
                    commands.push(format!("data modify storage {ns}:global {path} set from storage {ns}:global {chosen}"));
                    return Ok(commands);
                }
                if matches!(name.as_str(), "float" | "_cobble_float" | "double" | "int")
                    && !self.functions.params.contains_key(name)
                {
                    let arg = match args.as_slice() {
                        [arg] => arg,
                        [arg, Expression::String(_)] if name == "_cobble_float" => arg,
                        _ => return Err(format!("{name}() takes one value.").into()),
                    };
                    let literal = numeric_literal(arg).or_else(|| match arg {
                        Expression::Identifier(name) => {
                            self.symbols.compile_time_constants.get(name).copied()
                        }
                        Expression::String(text) => text.parse::<f64>().ok(),
                        _ => None,
                    });
                    if let Some(value) = literal {
                        if !value.is_finite() {
                            return Err("Numeric values must be finite.".into());
                        }
                        let value = if name == "double" {
                            format!("{value:e}d")
                        } else if name == "int" {
                            if value.trunc() < f64::from(i32::MIN)
                                || value.trunc() > f64::from(i32::MAX)
                            {
                                return Err("The value does not fit in a 32-bit integer.".into());
                            }
                            (value as i32).to_string()
                        } else {
                            let text = if name == "_cobble_float" && args.len() == 2 {
                                args.get(1)
                            } else {
                                Some(arg)
                            };
                            let value = if let Some(Expression::String(text)) = text {
                                text.parse::<f32>().map_err(|_| "Invalid float text.")?
                            } else {
                                value as f32
                            };
                            if !value.is_finite() {
                                return Err("The value does not fit in a finite float.".into());
                            }
                            format!("{value:e}f")
                        };
                        return Ok(vec![format!(
                            "data modify storage {ns}:global {path} set value {value}"
                        )]);
                    }
                    let input = self.numeric_temporary();
                    let mut commands = match arg {
                        Expression::String(_) => self.storage_value_commands(&input, arg)?,
                        _ => self.numeric_value_commands(&input, arg)?,
                    };
                    super::numeric_runtime::add_functions(self.data_pack);
                    let worker = if name == "_cobble_float" {
                        "float"
                    } else {
                        name
                    };
                    commands.extend([
                        format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {input}"),
                        format!("data modify storage {ns}:global _cobble_numeric.right set value 0"),
                        format!("function {ns}:_cobble_numeric_dispatch_double_cast_{worker}"),
                        format!("data modify storage {ns}:global {path} set from storage {ns}:global _cobble_numeric.result"),
                    ]);
                    return Ok(commands);
                }
            }
        }
        match expr {
            Expression::Number(value) => {
                let text = if value.fract() == 0.0 {
                    self.warn_score_value(*value);
                    (*value as i32).to_string()
                } else {
                    let value = *value as f32;
                    if !value.is_finite() {
                        return Err("The value does not fit in a finite float.".into());
                    }
                    format!("{value:e}f")
                };
                Ok(vec![format!(
                    "data modify storage {ns}:global {path} set value {text}"
                )])
            }
            Expression::Boolean(value) => Ok(vec![format!(
                "data modify storage {ns}:global {path} set value {}b",
                i32::from(*value)
            )]),
            Expression::Identifier(name)
                if name.starts_with('@') || name.starts_with("__cobble_cond_operand_") =>
            {
                self.data_pack.track_objective("temp");
                let holder = self
                    .symbols
                    .selector_aliases
                    .get(name.trim_start_matches('@'))
                    .map(String::as_str)
                    .unwrap_or(name);
                Ok(vec![format!("execute store result storage {ns}:global {path} int 1 run scoreboard players get {holder} temp")])
            }
            Expression::Identifier(name) => {
                if let Some(value) = self.symbols.compile_time_constants.get(name).copied() {
                    let expr = match self.symbols.variable_types.get(name) {
                        Some(CobbleType::Double) => Expression::Call(
                            Box::new(Expression::Identifier("double".into())),
                            vec![Expression::Number(value)],
                        ),
                        Some(CobbleType::Float) => Expression::Call(
                            Box::new(Expression::Identifier("_cobble_float".into())),
                            vec![Expression::Number(value)],
                        ),
                        _ => Expression::Number(value),
                    };
                    return self.numeric_value_commands(path, &expr);
                }
                if !self.current_params.contains(name)
                    && !self.symbols.variable_storage_paths.contains_key(name)
                    && self.symbols.scoreboard_objective(name).is_some()
                    && (name.starts_with('#')
                        || (!self.numeric_locals.contains(name)
                            && !self.symbols.module_variables.contains(name)))
                {
                    let objective = self
                        .symbols
                        .scoreboard_objective(name)
                        .expect("checked score");
                    self.data_pack.track_objective(objective);
                    return Ok(vec![format!("execute store result storage {ns}:global {path} int 1 run scoreboard players get {name} {objective}")]);
                }
                let source = if self.current_params.contains(name) {
                    format!("args.{name}")
                } else if let Some(source) = self.symbols.variable_storage_paths.get(name) {
                    source.clone()
                } else if let Some(function) = self
                    .numeric_scope
                    .as_deref()
                    .filter(|_| self.numeric_locals.contains(name))
                {
                    format!("locals.{function}.{name}")
                } else {
                    format!("vars.{name}")
                };
                if source == path {
                    return Ok(Vec::new());
                }
                Ok(vec![format!("data modify storage {ns}:global {path} set value 0"), format!("data modify storage {ns}:global {path} set from storage {ns}:global {source}")])
            }
            Expression::Subscript(..) | Expression::Attribute(..) => {
                let Some(plan) = self.resolve_storage_path_plan(expr) else {
                    return Err("A numeric read needs a storage path.".into());
                };
                let mut commands = vec![format!(
                    "data modify storage {ns}:global {path} set value 0"
                )];
                commands.extend(self.copy_storage_commands(path, &plan)?);
                Ok(commands)
            }
            Expression::Unary(UnaryOp::Pos, inner) => self.numeric_value_commands(path, inner),
            Expression::Unary(UnaryOp::Neg, inner) => self.numeric_value_commands(
                path,
                &Expression::Binary(
                    Box::new(Expression::Number(-1.0)),
                    BinaryOp::Mul,
                    inner.clone(),
                ),
            ),
            Expression::Binary(left, op, right)
                if matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::FloorDiv
                        | BinaryOp::Mod
                        | BinaryOp::Pow
                ) =>
            {
                for operand in [left.as_ref(), right.as_ref()] {
                    if let Expression::Identifier(name) = operand {
                        self.reject_string_score_operand(name)?;
                    }
                }
                if matches!(op, BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod)
                    && numeric_literal(right).or_else(|| match &**right {
                        Expression::Identifier(name) => {
                            self.symbols.compile_time_constants.get(name).copied()
                        }
                        _ => None,
                    }) == Some(0.0)
                {
                    return Err(if matches!(op, BinaryOp::Mod) {
                        "Modulo by zero is not allowed."
                    } else {
                        "Division by zero is not allowed."
                    }
                    .into());
                }
                let left_path = self.numeric_temporary();
                let right_path = self.numeric_temporary();
                let mut commands = self.numeric_value_commands(&left_path, left)?;
                commands.extend(self.numeric_value_commands(&right_path, right)?);
                super::numeric_runtime::add_functions(self.data_pack);
                let op = match op {
                    BinaryOp::Add => "add",
                    BinaryOp::Sub => "sub",
                    BinaryOp::Mul => "mul",
                    BinaryOp::Div => "div",
                    BinaryOp::FloorDiv => "floor_div",
                    BinaryOp::Mod => "mod",
                    _ => "pow",
                };
                commands.extend([
                    format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {left_path}"),
                    format!("data modify storage {ns}:global _cobble_numeric.right set from storage {ns}:global {right_path}"),
                    format!("function {ns}:_cobble_numeric_{op}"),
                    format!("data modify storage {ns}:global {path} set from storage {ns}:global _cobble_numeric.result"),
                ]);
                Ok(commands)
            }
            Expression::Call(func, args) => {
                let scratch = format!("#numeric_call_{}", self.next_temp_id());
                let mut commands = self.call_to_target(func, args, &scratch, "temp")?;
                let callee = self.resolve_user_function_call_name(func);
                if callee.is_some_and(|name| self.functions.numeric_returns.contains(name)) {
                    commands.push(format!("data modify storage {ns}:global {path} set from storage {ns}:global _cobble_return"));
                } else {
                    commands.push(format!("execute store result storage {ns}:global {path} int 1 run scoreboard players get {scratch} temp"));
                }
                Ok(commands)
            }
            _ => Err("This value is not a numeric expression.".into()),
        }
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn numeric_condition_value(
        &mut self,
        left: &Expression,
        comparison: Option<(&BinaryOp, &Expression)>,
    ) -> Result<Expression, TranspileError> {
        if self.may_hold_double(left)
            || comparison.is_some_and(|(_, right)| self.may_hold_double(right))
        {
            self.require_prelude("_cobble_numeric_double_compare")?;
        }
        let id = self.next_temp_id();
        let root = match self.current_function_name.as_deref() {
            Some(function) => format!("locals.{function}._cobble_comparison_{id}"),
            None => format!("_cobble_comparison_{id}"),
        };
        let flag = format!("expr_cond_temp_{id}");
        let ns = self.data_pack.namespace.clone();
        let mut commands = if matches!(left, Expression::Identifier(name) if name.starts_with('@') || name.starts_with("__cobble_cond_operand_"))
        {
            self.numeric_value_commands(&format!("{root}.left"), left)?
        } else {
            self.storage_value_commands(&format!("{root}.left"), left)?
        };
        let worker = if let Some((op, right)) = comparison {
            commands.extend(if matches!(right, Expression::Identifier(name) if name.starts_with('@') || name.starts_with("__cobble_cond_operand_")) {
                self.numeric_value_commands(&format!("{root}.right"), right)?
            } else { self.storage_value_commands(&format!("{root}.right"), right)? });
            if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                "equal"
            } else {
                "compare"
            }
        } else {
            "truth"
        };
        super::numeric_runtime::add_functions(&mut self.data_pack);
        commands.push(format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {root}.left"));
        if comparison.is_some() {
            commands.push(format!("data modify storage {ns}:global _cobble_numeric.right set from storage {ns}:global {root}.right"));
        }
        commands.extend([
            format!("function {ns}:_cobble_numeric_{worker}"),
            format!("execute store result score {flag} temp run data get storage {ns}:global _cobble_numeric.result 1"),
        ]);
        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(flag.clone(), "temp".into());
        self.symbols.scoreboard_variables.insert(flag.clone());
        self.extend_current_function(commands);
        Ok(Expression::Binary(
            Box::new(Expression::Identifier(flag)),
            comparison
                .map(|(op, _)| op.clone())
                .unwrap_or(BinaryOp::NotEq),
            Box::new(Expression::Number(0.0)),
        ))
    }

    pub(in crate::transpiler) fn may_hold_float(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Number(value) => value.fract() != 0.0,
            Expression::Boolean(_) => false,
            Expression::Identifier(name)
                if name.starts_with('@') || name.starts_with("__cobble_cond_operand_") =>
            {
                false
            }
            Expression::Identifier(name) => {
                self.current_context.is_param(name)
                    || self
                        .real_variables
                        .contains(&self.variable_storage_path(name))
                    || (self.symbols.variable_types.get(name) == Some(&CobbleType::Integer)
                        && self.symbols.variable_storage_paths.contains_key(name))
                    || self
                        .symbols
                        .compile_time_constants
                        .get(name)
                        .is_some_and(|value| value.fract() != 0.0)
                    || matches!(
                        self.symbols.variable_types.get(name),
                        Some(CobbleType::Float | CobbleType::Double | CobbleType::Unknown) | None
                    )
            }
            Expression::Binary(left, _, right) => {
                self.may_hold_float(left) || self.may_hold_float(right)
            }
            Expression::Unary(_, inner) => self.may_hold_float(inner),
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if name == "len" && !self.functions.params.contains_key(name)) => {
                false
            }
            Expression::Call(..) => true,
            Expression::Subscript(..) | Expression::Attribute(..) => true,
            _ => matches!(
                self.value_type(expr),
                CobbleType::Float | CobbleType::Double | CobbleType::Unknown
            ),
        }
    }

    pub(in crate::transpiler) fn condition_uses_numeric_storage(&self, expr: &Expression) -> bool {
        if !self.uses_storage_numbers() {
            return false;
        }
        if let Expression::Identifier(name) = expr {
            if self.symbols.minecraft_conditions.contains_key(name) {
                return false;
            }
        }
        !matches!(
            self.value_type(expr),
            CobbleType::String | CobbleType::List | CobbleType::Map
        )
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn numeric_compound_storage(
        &mut self,
        plan: &super::StoragePathPlan,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        use super::GeneratedCommandKind;
        let worker = match op {
            BinaryOp::Add => "add",
            BinaryOp::Sub => "sub",
            BinaryOp::Mul => "mul",
            BinaryOp::Div => "div",
            BinaryOp::FloorDiv => "floor_div",
            BinaryOp::Mod => "mod",
            _ => "pow",
        };
        self.require_prelude(&format!("_cobble_numeric_double_{worker}"))?;
        let id = self.next_temp_id();
        let ns = self.data_pack.namespace.clone();
        let root = match self.current_function_name.as_deref() {
            Some(function) => format!("locals.{function}._cobble_update_{id}"),
            None => format!("_cobble_update_{id}"),
        };
        let mut commands = vec![format!(
            "data modify storage {ns}:global {root}.left set value 0"
        )];
        let args = if let Some(path) = plan.as_static() {
            commands.push(format!(
                "data modify storage {ns}:global {root}.left set from storage {ns}:global {path}"
            ));
            None
        } else {
            let (prepare, args) = self.prepare_dynamic_storage_path(id, plan)?;
            commands.extend(prepare);
            let saved = format!("{root}.target");
            commands.push(format!(
                "data modify storage {ns}:global {saved} set from storage {ns}:global {args}"
            ));
            let helper = format!("_cobble_numeric_read_{id}");
            let lines = plan.helper_lines(id, |path| vec![format!("$data modify storage {ns}:global {root}.left set from storage {ns}:global {path}")]);
            self.add_generated_helper_function(
                helper.clone(),
                lines,
                GeneratedCommandKind::ControlFlow,
            );
            commands.push(format!(
                "function {ns}:{helper} with storage {ns}:global {saved}"
            ));
            Some(saved)
        };
        commands.extend(self.numeric_value_commands(&format!("{root}.right"), right)?);
        super::numeric_runtime::add_functions(&mut self.data_pack);
        let op = match op {
            BinaryOp::Add => "add",
            BinaryOp::Sub => "sub",
            BinaryOp::Mul => "mul",
            BinaryOp::Div => "div",
            BinaryOp::FloorDiv => "floor_div",
            BinaryOp::Mod => "mod",
            _ => "pow",
        };
        commands.extend([
            format!("data modify storage {ns}:global _cobble_numeric.left set from storage {ns}:global {root}.left"),
            format!("data modify storage {ns}:global _cobble_numeric.right set from storage {ns}:global {root}.right"),
            format!("function {ns}:_cobble_numeric_{op}"),
            format!("data modify storage {ns}:global {root}.result set from storage {ns}:global _cobble_numeric.result"),
        ]);
        if let Some(path) = plan.as_static() {
            commands.push(format!(
                "data modify storage {ns}:global {path} set from storage {ns}:global {root}.result"
            ));
        } else {
            let helper = format!("_cobble_numeric_write_{id}");
            let lines = plan.helper_lines(id, |path| vec![format!("$data modify storage {ns}:global {path} set from storage {ns}:global {root}.result")]);
            self.add_generated_helper_function(
                helper.clone(),
                lines,
                GeneratedCommandKind::ControlFlow,
            );
            commands.push(format!(
                "function {ns}:{helper} with storage {ns}:global {}",
                args.expect("dynamic target")
            ));
        }
        Ok(commands)
    }
}
