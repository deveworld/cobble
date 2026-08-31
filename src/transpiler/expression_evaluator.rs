use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};
use crate::transpiler::data_pack::DataPack;
use crate::transpiler::storage_paths::StoragePathOps;
use crate::transpiler::{
    FunctionReturnKind, GeneratedCommandKind, SourceLocation, StoragePathPlan, TranspileError,
    MAX_POWER_EXPONENT,
};
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Evaluate expressions and translate conditions for Minecraft commands
pub struct ExpressionEvaluator<'a> {
    pub data_pack: &'a mut DataPack,
    pub variable_objectives: &'a HashMap<String, String>,
    pub variable_storage_paths: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
    pub function_params: &'a HashMap<String, Vec<String>>,
    pub function_returns: &'a HashMap<String, FunctionReturnKind>,
    pub qualified_function_aliases: &'a HashMap<String, String>,
    pub current_params: &'a [String],
    pub variable_types: &'a HashMap<String, CobbleType>,
    pub temp_counter: &'a mut u32,
    pub source: Option<SourceLocation>,
    /// Name of the user function whose body is currently being lowered, for
    /// rejecting recursive calls at the call site.
    pub current_function_name: Option<&'a str>,
    /// Caller -> callees edges shared with the transpiler's cycle check.
    pub call_edges: &'a mut BTreeMap<String, BTreeSet<String>>,
}

impl<'a> ExpressionEvaluator<'a> {
    // Mirrors the transpiler's borrowed tables one-to-one; grouping them into a context
    // struct would require changing the only construction site in mod.rs.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        data_pack: &'a mut DataPack,
        variable_objectives: &'a HashMap<String, String>,
        variable_storage_paths: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
        function_params: &'a HashMap<String, Vec<String>>,
        function_returns: &'a HashMap<String, FunctionReturnKind>,
        qualified_function_aliases: &'a HashMap<String, String>,
        current_params: &'a [String],
        variable_types: &'a HashMap<String, CobbleType>,
        temp_counter: &'a mut u32,
        source: Option<SourceLocation>,
        current_function_name: Option<&'a str>,
        call_edges: &'a mut BTreeMap<String, BTreeSet<String>>,
    ) -> Self {
        Self {
            data_pack,
            variable_objectives,
            variable_storage_paths,
            compile_time_constants,
            function_params,
            function_returns,
            qualified_function_aliases,
            current_params,
            variable_types,
            temp_counter,
            source,
            current_function_name,
            call_edges,
        }
    }

    fn expression_path(expr: &Expression) -> Option<String> {
        match expr {
            Expression::Identifier(name) => Some(name.clone()),
            Expression::Attribute(obj, attr) => {
                Some(format!("{}.{}", Self::expression_path(obj)?, attr))
            }
            _ => None,
        }
    }

    fn resolve_user_function_call_name<'b>(&'b self, func: &'b Expression) -> Option<&'b str> {
        match func {
            Expression::Identifier(name) if self.function_params.contains_key(name) => {
                Some(name.as_str())
            }
            Expression::Attribute(obj, method) => {
                let module_name = Self::expression_path(obj)?;
                let key = format!("{}.{}", module_name, method);
                self.qualified_function_aliases
                    .get(&key)
                    .map(String::as_str)
            }
            _ => None,
        }
    }

    fn next_temp_id(&mut self) -> u32 {
        let id = *self.temp_counter;
        *self.temp_counter += 1;
        id
    }

    fn is_current_param(&self, name: &str) -> bool {
        self.current_params.iter().any(|param| param == name)
    }

    /// Whether `target` is read anywhere inside `expr` (as a bare identifier).
    fn expression_reads_target(expr: &Expression, target: &str) -> bool {
        match expr {
            Expression::Identifier(name) => name == target,
            Expression::Attribute(obj, _) => Self::expression_reads_target(obj, target),
            Expression::Subscript(obj, index) => {
                Self::expression_reads_target(obj, target)
                    || Self::expression_reads_target(index, target)
            }
            Expression::Binary(left, _, right) => {
                Self::expression_reads_target(left, target)
                    || Self::expression_reads_target(right, target)
            }
            Expression::Unary(_, inner) => Self::expression_reads_target(inner, target),
            Expression::Call(func, args) => {
                Self::expression_reads_target(func, target)
                    || args
                        .iter()
                        .any(|arg| Self::expression_reads_target(arg, target))
            }
            Expression::Array(items) => items
                .iter()
                .any(|item| Self::expression_reads_target(item, target)),
            Expression::Map(entries) => entries
                .iter()
                .any(|(_, value)| Self::expression_reads_target(value, target)),
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => false,
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

    fn static_power_exponent(&self, expr: &Expression) -> Option<i32> {
        match expr {
            Expression::Number(value) => Some(*value as i32),
            Expression::Identifier(name) => self
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32),
            _ => None,
        }
    }

    fn apply_runtime_power_to_target(
        &mut self,
        commands: &mut Vec<String>,
        target: &str,
        target_obj: &str,
        exponent: &str,
        exponent_obj: &str,
    ) {
        self.data_pack.track_objective("temp");
        self.data_pack.track_objective(target_obj);

        let id = self.next_temp_id();
        let base_var = format!("#power_base_{}", id);
        let exponent_var = format!("#power_exp_{}", id);
        let helper_name = format!("_cobble_power_{}", id);
        let namespace = self.data_pack.namespace.clone();

        commands.push(format!(
            "scoreboard players operation {} temp = {} {}",
            base_var, target, target_obj
        ));
        commands.push(format!(
            "scoreboard players operation {} temp = {} {}",
            exponent_var, exponent, exponent_obj
        ));
        commands.push(format!(
            "scoreboard players set {} {} 0",
            target, target_obj
        ));
        commands.push(format!(
            "execute if score {} temp matches 0 run scoreboard players set {} {} 1",
            exponent_var, target, target_obj
        ));
        commands.push(format!(
            "execute if score {} temp matches 1.. run scoreboard players set {} {} 1",
            exponent_var, target, target_obj
        ));
        commands.push(format!(
            "execute if score {} temp matches 1.. run function {}:{}",
            exponent_var, namespace, helper_name
        ));

        let helper_commands = vec![
            format!(
                "scoreboard players operation {} {} *= {} temp",
                target, target_obj, base_var
            ),
            format!("scoreboard players remove {} temp 1", exponent_var),
            format!(
                "execute if score {} temp matches 1.. run function {}:{}",
                exponent_var, namespace, helper_name
            ),
        ];
        self.add_generated_helper_function(
            helper_name,
            helper_commands,
            GeneratedCommandKind::ControlFlow,
        );
    }

    fn read_dynamic_storage_path_to_target(
        &mut self,
        plan: &StoragePathPlan,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let id = self.next_temp_id();
        let (mut commands, args_path) = self.prepare_dynamic_storage_path(id, plan)?;
        let helper_name = format!("_cobble_storage_read_{}", id);
        let namespace = self.data_pack.namespace.clone();
        let helper_commands = vec![
            format!("scoreboard players set {} {} 0", target, target_obj),
            format!(
                "$execute store result score {} {} run data get storage {}:global {}",
                target,
                target_obj,
                namespace,
                plan.template()
            ),
        ];
        self.add_generated_helper_function(
            helper_name.clone(),
            helper_commands,
            GeneratedCommandKind::ControlFlow,
        );
        commands.push(format!(
            "function {}:{} with storage {}:global {}",
            namespace, helper_name, namespace, args_path
        ));
        commands.push(format!(
            "data remove storage {}:global {}",
            namespace, args_path
        ));
        Ok(commands)
    }

    fn user_function_call_to_target(
        &mut self,
        func_name: &str,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(caller) = self.current_function_name {
            if caller == func_name {
                return Err(format!(
                    "Recursive call to '{}' is not supported. Rewrite the recursion as a loop (while/for) instead.",
                    func_name
                ).into());
            }
            self.call_edges
                .entry(caller.to_string())
                .or_default()
                .insert(func_name.to_string());
        }
        let Some(param_names) = self.function_params.get(func_name).cloned() else {
            return Err(format!(
                "Undefined function `{}` in expression. Define it before using its return value.",
                func_name
            )
            .into());
        };

        if args.len() != param_names.len() {
            return Err(format!(
                "Function '{}' expects {} argument(s), but {} provided.",
                func_name,
                param_names.len(),
                args.len()
            )
            .into());
        }

        let namespace = self.data_pack.namespace.clone();
        let mut commands = Vec::new();
        let preserve_caller_args = !self.current_params.is_empty();
        let staged_args_path = if !param_names.is_empty() {
            let id = self.next_temp_id();
            let call_path = format!("_call_args_{}", id);
            let caller_path = if preserve_caller_args {
                let caller_path = format!("_caller_args_{}", id);
                commands.push(format!(
                    "data remove storage {}:global {}",
                    namespace, caller_path
                ));
                commands.push(format!(
                    "data modify storage {}:global {} set from storage {}:global args",
                    namespace, caller_path, namespace
                ));
                Some(caller_path)
            } else {
                None
            };
            commands.push(format!(
                "data remove storage {}:global {}",
                namespace, call_path
            ));
            Some((call_path, caller_path))
        } else {
            None
        };

        for (param_name, arg) in param_names.iter().zip(args.iter()) {
            let target_path = staged_args_path
                .as_ref()
                .map(|(path, _)| format!("{}.{}", path, param_name))
                .unwrap_or_else(|| format!("args.{}", param_name));
            commands.extend(self.function_argument_to_storage_commands(&target_path, arg)?);
        }

        if let Some((call_path, _)) = staged_args_path.as_ref() {
            commands.push(format!(
                "data modify storage {}:global args set from storage {}:global {}",
                namespace, namespace, call_path
            ));
        }

        if param_names.is_empty() {
            commands.push(format!(
                "execute store result score {} {} run function {}:{}",
                target, target_obj, namespace, func_name
            ));
        } else {
            commands.push(format!(
                "execute store result score {} {} run function {}:{} with storage {}:global args",
                target, target_obj, namespace, func_name, namespace
            ));
        }

        if let Some((call_path, caller_path)) = staged_args_path {
            if let Some(caller_path) = caller_path {
                commands.push(format!(
                    "data modify storage {}:global args set from storage {}:global {}",
                    namespace, namespace, caller_path
                ));
                commands.push(format!(
                    "data remove storage {}:global {}",
                    namespace, caller_path
                ));
            }
            commands.push(format!(
                "data remove storage {}:global {}",
                namespace, call_path
            ));
        }

        Ok(commands)
    }

    fn function_argument_to_storage_commands(
        &mut self,
        target_path: &str,
        arg: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        match arg {
            Expression::String(value) => {
                let escaped = value
                    .replace('\\', "\\\\")
                    .replace('"', "\\\"")
                    .replace('\n', "\\n")
                    .replace('\r', "\\r")
                    .replace('\t', "\\t");
                Ok(vec![format!(
                    "data modify storage {}:global {} set value \"{}\"",
                    namespace, target_path, escaped
                )])
            }
            Expression::Number(value) => Ok(vec![format!(
                "data modify storage {}:global {} set value {}",
                namespace, target_path, *value as i32
            )]),
            Expression::Boolean(value) => Ok(vec![format!(
                "data modify storage {}:global {} set value {}",
                namespace,
                target_path,
                if *value { 1 } else { 0 }
            )]),
            Expression::Identifier(name) => {
                if name.starts_with('@') {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set value \"{}\"",
                        namespace, target_path, name
                    )]);
                }
                if self.is_current_param(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global args.{}",
                        namespace, target_path, namespace, name
                    )]);
                }
                if let Some(source_path) = self.variable_storage_paths.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if let Some(value) = self.compile_time_constants.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set value {}",
                        namespace, target_path, *value as i32
                    )]);
                }
                let objective = self
                    .variable_objectives
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("temp");
                Ok(vec![format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                    namespace, target_path, name, objective
                )])
            }
            _ => {
                let id = self.next_temp_id();
                let temp_score = format!("#arg_temp_{}", id);
                let mut commands = self.evaluate_expression_to_target(arg, &temp_score)?;
                commands.push(format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    namespace, target_path, temp_score
                ));
                Ok(commands)
            }
        }
    }

    /// Helper method to evaluate a complex expression into a target variable
    #[allow(clippy::single_match)]
    pub fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let mut commands = Vec::new();
        let target_obj = self.target_objective(target).to_string();
        self.data_pack.track_objective("temp");
        self.data_pack.track_objective(&target_obj);

        // Self-referential RHS: evaluating directly into `target` would overwrite it
        // before later reads (e.g. `x = 2 - x` would clobber `x` with 2 first).
        // Route through a fresh scratch score and copy into the target at the end.
        // A bare `target = target` stays on the direct path, which skips it as a no-op.
        if !matches!(expr, Expression::Identifier(name) if name == target)
            && Self::expression_reads_target(expr, target)
        {
            let scratch = format!("#expr_result_{}", self.next_temp_id());
            commands.extend(self.evaluate_expression_to_target(expr, &scratch)?);
            commands.push(format!(
                "scoreboard players operation {} {} = {} temp",
                target, target_obj, scratch
            ));
            return Ok(commands);
        }

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
                    // A string's scoreboard mirror only holds its length, so
                    // evaluating it as a score value is always a bug.
                    if matches!(self.variable_types.get(var), Some(CobbleType::String)) {
                        return Err(format!(
                            "Variable '{}' holds a storage-backed string and cannot be used in arithmetic; its scoreboard mirror only stores the string length. Use integer values instead.",
                            var
                        ).into());
                    }
                    // Load from storage to scoreboard
                    let namespace = &self.data_pack.namespace;
                    commands.push(format!(
                        "execute store result score {} {} run data get storage {}:global {}",
                        target, target_obj, namespace, storage_path
                    ));
                } else if self.variable_objectives.contains_key(var) {
                    let var_obj = self.variable_objectives.get(var).unwrap().clone();
                    if !(var == target && var_obj == target_obj) {
                        commands.push(format!(
                            "scoreboard players operation {} {} = {} {}",
                            target, target_obj, var, var_obj
                        ));
                    }
                } else if self.is_current_param(var) {
                    let namespace = &self.data_pack.namespace;
                    commands.push(format!(
                        "execute store result score {} {} run data get storage {}:global args.{}",
                        target, target_obj, namespace, var
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
                if matches!(&**func, Expression::Identifier(name) if name == "len") {
                    if args.len() != 1 {
                        return Err(format!("len() takes 1 argument, got {}", args.len()).into());
                    }
                    match &args[0] {
                        Expression::String(value) => {
                            commands.push(format!(
                                "scoreboard players set {} {} {}",
                                target,
                                target_obj,
                                value.chars().count()
                            ));
                            return Ok(commands);
                        }
                        Expression::Array(items) => {
                            commands.push(format!(
                                "scoreboard players set {} {} {}",
                                target,
                                target_obj,
                                items.len()
                            ));
                            return Ok(commands);
                        }
                        Expression::Map(entries) => {
                            commands.push(format!(
                                "scoreboard players set {} {} {}",
                                target,
                                target_obj,
                                entries.len()
                            ));
                            return Ok(commands);
                        }
                        _ => {}
                    }

                    let Some(path_plan) = self.resolve_storage_path_plan(&args[0]) else {
                        return Err(
                            "len() argument must be a string, list, map, or storage-backed value"
                                .to_string()
                                .into(),
                        );
                    };

                    let namespace = &self.data_pack.namespace;
                    if let Some(path) = path_plan.as_static() {
                        commands.push(format!(
                            "execute store result score {} {} run data get storage {}:global {}",
                            target, target_obj, namespace, path
                        ));
                    } else {
                        commands.extend(self.read_dynamic_storage_path_to_target(
                            &path_plan,
                            target,
                            &target_obj,
                        )?);
                    }
                    return Ok(commands);
                }

                // Handle intrinsics like math.sqrt
                match &**func {
                    Expression::Attribute(obj, method) => {
                        if let Expression::Identifier(module) = &**obj {
                            if module == "math" {
                                let op = method.as_str();
                                match op {
                                    "sqrt" | "abs" => {
                                        if args.len() != 1 {
                                            return Err(
                                                format!("math.{} takes 1 argument", op).into()
                                            );
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
                                        commands.push(format!(
                                            "scoreboard players operation {} {} = #math_result math",
                                            target, target_obj
                                        ));
                                        return Ok(commands);
                                    }
                                    "min" | "max" => {
                                        if args.len() != 2 {
                                            return Err(
                                                format!("math.{} takes 2 arguments", op).into()
                                            );
                                        }
                                        let input1 = "#math_input";
                                        let input2 = "#math_input2";
                                        self.data_pack.track_objective("math");

                                        // Evaluating an argument can itself run a math
                                        // helper (a nested math.* call), which stages
                                        // into the shared #math_input scores and would
                                        // clobber an argument staged earlier. Stage each
                                        // argument into a unique scratch and copy into
                                        // the helper inputs right before the call.
                                        let arg1_scratch =
                                            format!("#math_arg_{}", self.next_temp_id());
                                        let arg2_scratch =
                                            format!("#math_arg_{}", self.next_temp_id());
                                        let mut eval_cmds = self.evaluate_expression_to_target(
                                            &args[0],
                                            &arg1_scratch,
                                        )?;
                                        eval_cmds.extend(self.evaluate_expression_to_target(
                                            &args[1],
                                            &arg2_scratch,
                                        )?);
                                        eval_cmds.push(format!(
                                            "scoreboard players operation {} math = {} math",
                                            input1, arg1_scratch
                                        ));
                                        eval_cmds.push(format!(
                                            "scoreboard players operation {} math = {} math",
                                            input2, arg2_scratch
                                        ));
                                        commands.extend(eval_cmds);

                                        self.data_pack.ensure_math_helper(op);
                                        commands.push(format!(
                                            "function {}:_cobble_math_{}",
                                            self.data_pack.namespace, op
                                        ));

                                        self.data_pack.track_objective("temp");
                                        commands.push(format!(
                                            "scoreboard players operation {} {} = #math_result math",
                                            target, target_obj
                                        ));
                                        return Ok(commands);
                                    }
                                    _ => {
                                        return Err(format!("Unknown math function: {}", op).into())
                                    }
                                }
                            }
                        }
                    }
                    // Non-math callees fall through to the shared user-function
                    // dispatch below this match.
                    _ => {}
                }
                if let Some(func_name) = self
                    .resolve_user_function_call_name(func)
                    .map(str::to_string)
                {
                    match self.function_returns.get(&func_name) {
                        Some(FunctionReturnKind::Integer) => {}
                        Some(FunctionReturnKind::NonInteger) => {
                            return Err(format!(
                                "Function '{}' cannot be used as an expression value because it returns non-integer data.",
                                func_name
                            ).into());
                        }
                        _ => {
                            return Err(format!(
                                "Function '{}' cannot be used as an expression value because it has no integer return value.",
                                func_name
                            ).into());
                        }
                    }
                    commands.extend(self.user_function_call_to_target(
                        &func_name,
                        args,
                        target,
                        &target_obj,
                    )?);
                    return Ok(commands);
                }
                return Err(
                    "Only value intrinsics and user functions with `return` values can be used in expressions."
                        .into(),
                );
            }
            Expression::Subscript(base, index) => {
                // Handle array/map access: base[index]
                if let Some(base_path) = self.resolve_storage_path_plan(base) {
                    let base_type = self.storage_expr_type(base);
                    let full_path = match &**index {
                        Expression::Number(n)
                            if self.should_use_dynamic_storage_key(base_type.clone(), index) =>
                        {
                            base_path.append_dynamic_key(Expression::Number(*n))
                        }
                        Expression::Number(n) => base_path.append_static_index(*n as i32),
                        Expression::String(s) => base_path.append_field(s),
                        Expression::Identifier(name) => {
                            if self.should_use_dynamic_storage_key(base_type.clone(), index)
                                && self.compile_time_constants.contains_key(name)
                            {
                                base_path.append_dynamic_key((**index).clone())
                            } else if let Some(val) = self.compile_time_constants.get(name) {
                                base_path.append_static_index(*val as i32)
                            } else if self.should_use_dynamic_storage_key(base_type.clone(), index)
                            {
                                base_path.append_dynamic_key((**index).clone())
                            } else {
                                base_path.append_dynamic_index((**index).clone())
                            }
                        }
                        _ if self.should_use_dynamic_storage_key(base_type, index) => {
                            base_path.append_dynamic_key((**index).clone())
                        }
                        _ => base_path.append_dynamic_index((**index).clone()),
                    };
                    let namespace = &self.data_pack.namespace;

                    if let Some(path) = full_path.as_static() {
                        commands.push(format!(
                            "execute store result score {} {} run data get storage {}:global {}",
                            target, target_obj, namespace, path
                        ));
                    } else {
                        commands.extend(self.read_dynamic_storage_path_to_target(
                            &full_path,
                            target,
                            &target_obj,
                        )?);
                    }
                } else {
                    return Err("Subscript base must resolve to a storage path (list/map)."
                        .to_string()
                        .into());
                }
            }
            Expression::Attribute(obj, attr) => {
                // Handle attribute access as map access: obj.attr
                if let Some(storage_path) = self.resolve_storage_path_plan(obj) {
                    let full_path = storage_path.append_field(attr);
                    let namespace = &self.data_pack.namespace;
                    if let Some(path) = full_path.as_static() {
                        commands.push(format!(
                            "execute store result score {} {} run data get storage {}:global {}",
                            target, target_obj, namespace, path
                        ));
                    } else {
                        commands.extend(self.read_dynamic_storage_path_to_target(
                            &full_path,
                            target,
                            &target_obj,
                        )?);
                    }
                } else {
                    return Err("Attribute base must resolve to a storage path"
                        .to_string()
                        .into());
                }
            }
            Expression::Binary(left, op, right) => {
                if matches!(op, BinaryOp::Pow) && self.static_power_exponent(right).is_none() {
                    let id = self.next_temp_id();
                    let base_temp = format!("#expr_power_base_{}", id);
                    let exponent_temp = format!("#expr_power_exp_{}", id);
                    commands.extend(self.evaluate_expression_to_target(left, &base_temp)?);
                    commands.extend(self.evaluate_expression_to_target(right, &exponent_temp)?);
                    commands.push(format!(
                        "scoreboard players operation {} {} = {} temp",
                        target, target_obj, base_temp
                    ));
                    self.apply_runtime_power_to_target(
                        &mut commands,
                        target,
                        &target_obj,
                        &exponent_temp,
                        "temp",
                    );
                    return Ok(commands);
                }

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
                                    )
                                    .into());
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
                                    )
                                    .into());
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
                                    return Err("Power exponent must be non-negative"
                                        .to_string()
                                        .into());
                                }
                                if value > MAX_POWER_EXPONENT {
                                    return Err(format!(
                                        "Power exponent too large: {} > {}.\n\
                                        \n\
                                        Large exponents generate {} multiplication commands, which is excessive.\n\
                                        Solution: Use a smaller exponent or implement iterative multiplication in a loop.",
                                        value, MAX_POWER_EXPONENT, value - 1
                                    ).into());
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
                            _ => {
                                return Err(format!("Unsupported binary operation: {:?}", op).into())
                            }
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
                                        return Err("Division by zero in expression"
                                            .to_string()
                                            .into());
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
                                        return Err("Modulo by zero in expression"
                                            .to_string()
                                            .into());
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
                                        return Err("Power exponent must be non-negative"
                                            .to_string()
                                            .into());
                                    }
                                    if value > MAX_POWER_EXPONENT {
                                        return Err(format!(
                                            "Power exponent too large: {} > {}.\n\
                                            \n\
                                            Large exponents generate {} multiplication commands, which is excessive.\n\
                                            Solution: Use a smaller exponent or implement iterative multiplication in a loop.",
                                            value, MAX_POWER_EXPONENT, value - 1
                                        ).into());
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
                                _ => {
                                    return Err(
                                        format!("Unsupported binary operation: {:?}", op).into()
                                    )
                                }
                            }
                        } else if self.is_current_param(var) {
                            let namespace = &self.data_pack.namespace;
                            let temp_param = "#expr_param";
                            commands.push(format!(
                                "execute store result score {} temp run data get storage {}:global args.{}",
                                temp_param, namespace, var
                            ));
                            match op {
                                BinaryOp::Add => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} += {} temp",
                                        target, target_obj, temp_param
                                    ));
                                }
                                BinaryOp::Sub => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} -= {} temp",
                                        target, target_obj, temp_param
                                    ));
                                }
                                BinaryOp::Mul => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} *= {} temp",
                                        target, target_obj, temp_param
                                    ));
                                }
                                BinaryOp::Div => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} /= {} temp",
                                        target, target_obj, temp_param
                                    ));
                                }
                                BinaryOp::Mod => {
                                    commands.push(format!(
                                        "scoreboard players operation {} {} %= {} temp",
                                        target, target_obj, temp_param
                                    ));
                                }
                                BinaryOp::Pow => {
                                    self.apply_runtime_power_to_target(
                                        &mut commands,
                                        target,
                                        &target_obj,
                                        temp_param,
                                        "temp",
                                    );
                                }
                                _ => {
                                    return Err(
                                        format!("Unsupported binary operation: {:?}", op).into()
                                    )
                                }
                            }
                        } else {
                            if self.variable_storage_paths.contains_key(var)
                                && matches!(self.variable_types.get(var), Some(CobbleType::String))
                            {
                                return Err(format!(
                                    "Variable '{}' holds a storage-backed string and cannot be used in arithmetic; its scoreboard mirror only stores the string length. Use integer values instead.",
                                    var
                                ).into());
                            }
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
                                    self.apply_runtime_power_to_target(
                                        &mut commands,
                                        target,
                                        &target_obj,
                                        var,
                                        &var_obj,
                                    );
                                }
                                _ => {
                                    return Err(
                                        format!("Unsupported binary operation: {:?}", op).into()
                                    )
                                }
                            }
                        }
                    }
                    Expression::Binary(_, _, _)
                    | Expression::Unary(_, _)
                    | Expression::Call(_, _)
                    | Expression::Attribute(_, _)
                    | Expression::Subscript(_, _) => {
                        // Right side is also a complex expression - evaluate it first into
                        // a scratch score unique to this call, so nested right-hand
                        // evaluations cannot clobber each other's intermediate results.
                        let right_temp = format!("#expr_temp_{}", self.next_temp_id());
                        let right_commands =
                            self.evaluate_expression_to_target(right, &right_temp)?;
                        commands.extend(right_commands);

                        // Now perform the operation
                        match op {
                            BinaryOp::Add => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} += {} temp",
                                    target, target_obj, right_temp
                                ));
                            }
                            BinaryOp::Sub => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} -= {} temp",
                                    target, target_obj, right_temp
                                ));
                            }
                            BinaryOp::Mul => {
                                commands.push(format!(
                                    "scoreboard players operation {} {} *= {} temp",
                                    target, target_obj, right_temp
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
                                    "scoreboard players operation {} {} /= {} temp",
                                    target, target_obj, right_temp
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
                                    "scoreboard players operation {} {} %= {} temp",
                                    target, target_obj, right_temp
                                ));
                            }
                            BinaryOp::Pow => {
                                self.apply_runtime_power_to_target(
                                    &mut commands,
                                    target,
                                    &target_obj,
                                    &right_temp,
                                    "temp",
                                );
                            }
                            _ => {
                                return Err(format!("Unsupported binary operation: {:?}", op).into())
                            }
                        }
                    }
                    _ => {
                        return Err("Unsupported expression type in binary operation"
                            .to_string()
                            .into());
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
                                .to_string()
                                .into(),
                        );
                    }
                }
            }
            _ => {
                return Err("Unsupported expression type".into());
            }
        }

        Ok(commands)
    }
}
