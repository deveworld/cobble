use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};
use crate::transpiler::calls::ArgumentStaging;
use crate::transpiler::data_pack::DataPack;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::symbols::{FunctionTable, Symbols};
use crate::transpiler::{
    escape_quoted, record_warning, score_value_warning, FunctionReturnKind, GeneratedCommandKind,
    SourceLocation, StoragePathPlan, TranspileError, TranspileWarning, Transpiler,
    MAX_POWER_EXPONENT,
};
use std::cell::RefCell;

/// Lowers an expression into commands that compute it into a score.
/// The error for a comparison or `and`/`or` inside an arithmetic expression.
fn unsupported_operator(op: &BinaryOp) -> TranspileError {
    format!(
        "`{}` gives 1 or 0 only as a whole condition or value, as in `flag = a > b`, not inside arithmetic. Store it in a variable first.",
        Transpiler::binary_op_source_key(op)
    )
    .into()
}

pub struct ExpressionEvaluator<'a> {
    pub(in crate::transpiler) data_pack: &'a mut DataPack,
    pub(in crate::transpiler) symbols: &'a Symbols,
    pub(in crate::transpiler) functions: &'a mut FunctionTable,
    pub(in crate::transpiler) current_params: &'a [String],
    pub(in crate::transpiler) temp_counter: &'a mut u32,
    pub(in crate::transpiler) source: Option<SourceLocation>,
    /// Whether the function being lowered is recursive, so its calls stage
    /// arguments on the call stack.
    pub(in crate::transpiler) caller_recursive: bool,
    pub(in crate::transpiler) warnings: &'a RefCell<Vec<TranspileWarning>>,
    pub(in crate::transpiler) numeric_scope: Option<String>,
    pub(in crate::transpiler) numeric_locals: Vec<String>,
    pub(in crate::transpiler) numeric_backend: bool,
}

impl<'a> ExpressionEvaluator<'a> {
    pub(in crate::transpiler) fn warn_score_value(&self, value: f64) {
        if let Some(warning) = score_value_warning(value, self.source.clone()) {
            record_warning(self.warnings, warning);
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

    pub(in crate::transpiler) fn resolve_user_function_call_name<'b>(
        &'b self,
        func: &'b Expression,
    ) -> Option<&'b str> {
        match func {
            Expression::Identifier(name) if self.functions.params.contains_key(name) => {
                Some(name.as_str())
            }
            Expression::Attribute(obj, method) => {
                let module_name = Self::expression_path(obj)?;
                let key = format!("{}.{}", module_name, method);
                self.functions
                    .qualified_aliases
                    .get(&key)
                    .map(String::as_str)
            }
            _ => None,
        }
    }

    pub(in crate::transpiler) fn next_temp_id(&mut self) -> u32 {
        let id = *self.temp_counter;
        *self.temp_counter += 1;
        id
    }

    fn is_current_param(&self, name: &str) -> bool {
        self.current_params.iter().any(|param| param == name)
    }

    /// Whether `target` is read anywhere inside `expr` (as a bare identifier).
    pub(in crate::transpiler) fn expression_reads_target(expr: &Expression, target: &str) -> bool {
        match expr {
            Expression::MinecraftCondition(raw) => super::minecraft_condition_expression(raw)
                .is_some_and(|expr| Self::expression_reads_target(&expr, target)),
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
            Expression::Slice(base, start, stop) => {
                Self::expression_reads_target(base, target)
                    || [start, stop]
                        .into_iter()
                        .flatten()
                        .any(|bound| Self::expression_reads_target(bound, target))
            }
            Expression::Conditional(condition, value, otherwise) => {
                Self::expression_reads_target(condition, target)
                    || Self::expression_reads_target(value, target)
                    || Self::expression_reads_target(otherwise, target)
            }
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => false,
        }
    }

    /// Whether evaluating `expr` can run a user function. `len()` and the
    /// `math` helpers only run Cobble's own helpers, so only their arguments
    /// count.
    pub(in crate::transpiler) fn calls_user_code(expr: &Expression) -> bool {
        match expr {
            // Header lowering emits setup even without a call; an `elif`
            // must keep all of it behind its predecessor's branch flag.
            Expression::MinecraftCondition(_) => true,
            Expression::Call(func, args) => {
                let builtin = matches!(&**func, Expression::Identifier(name)
                    if matches!(name.as_str(), "len" | "abs" | "min" | "max"))
                    || matches!(&**func, Expression::Attribute(obj, _)
                        if matches!(&**obj, Expression::Identifier(module) if module == "math"));
                !builtin || args.iter().any(Self::calls_user_code)
            }
            Expression::Attribute(obj, _) => Self::calls_user_code(obj),
            Expression::Subscript(obj, index) => {
                Self::calls_user_code(obj) || Self::calls_user_code(index)
            }
            Expression::Binary(left, _, right) => {
                Self::calls_user_code(left) || Self::calls_user_code(right)
            }
            Expression::Unary(_, inner) => Self::calls_user_code(inner),
            Expression::Array(items) => items.iter().any(Self::calls_user_code),
            Expression::Map(entries) => entries
                .iter()
                .any(|(_, value)| Self::calls_user_code(value)),
            Expression::Slice(base, start, stop) => {
                Self::calls_user_code(base)
                    || [start, stop]
                        .into_iter()
                        .flatten()
                        .any(|bound| Self::calls_user_code(bound))
            }
            Expression::Conditional(condition, value, otherwise) => {
                Self::calls_user_code(condition)
                    || Self::calls_user_code(value)
                    || Self::calls_user_code(otherwise)
            }
            Expression::Identifier(_)
            | Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => false,
        }
    }

    /// Whether lowering `expr` into a score writes that score before a user
    /// function in `expr` runs. A binary operation puts its left operand
    /// into the target first, so a call anywhere on a right side runs after
    /// that write. A call's own result is stored after the call returns.
    fn writes_target_before_call(expr: &Expression) -> bool {
        match expr {
            Expression::Call(..) => false,
            Expression::Binary(left, _, right) => {
                Self::writes_target_before_call(left) || Self::calls_user_code(right)
            }
            Expression::Unary(_, inner) => Self::writes_target_before_call(inner),
            _ => Self::calls_user_code(expr),
        }
    }

    fn target_objective(&self, target: &str) -> &str {
        self.symbols
            .variable_objectives
            .get(target)
            .map(String::as_str)
            .unwrap_or("temp")
    }

    /// A string's scoreboard mirror only holds its length, so evaluating it
    /// as a score value is always a bug.
    pub(in crate::transpiler) fn reject_string_score_operand(
        &self,
        var: &str,
    ) -> Result<(), TranspileError> {
        if self.symbols.variable_storage_paths.contains_key(var)
            && matches!(
                self.symbols.variable_types.get(var),
                Some(CobbleType::String)
            )
        {
            return Err(format!(
                "Variable '{}' holds a storage-backed string and cannot be used in arithmetic; its scoreboard mirror only stores the string length. Use integer values instead.",
                var
            ).into());
        }
        Ok(())
    }

    fn static_power_exponent(&self, expr: &Expression) -> Option<i32> {
        match expr {
            Expression::Number(value) => Some(*value as i32),
            Expression::Identifier(name) => self
                .symbols
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
        // The helper calls itself once per exponent step, like a loop.
        self.data_pack.track_runtime_loop();

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

    fn read_storage_path_to_target(
        &mut self,
        plan: &StoragePathPlan,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(path) = plan.as_static() {
            return Ok(vec![format!(
                "execute store result score {} {} run data get storage {}:global {}",
                target, target_obj, self.data_pack.namespace, path
            )]);
        }
        self.read_dynamic_storage_path_to_target(plan, target, target_obj)
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
        let helper_commands = plan.helper_lines(id, |path| {
            vec![format!(
                "$execute store result score {} {} run data get storage {}:global {}",
                target, target_obj, namespace, path
            )]
        });
        self.add_generated_helper_function(
            helper_name.clone(),
            helper_commands,
            GeneratedCommandKind::ControlFlow,
        );
        // A missing value reads as 0. The helper doesn't run at all when its
        // path is one Minecraft refuses, as for an empty key, so the target
        // is cleared before the call.
        commands.push(format!("scoreboard players set {target} {target_obj} 0"));
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
        let Some(param_names) = self.functions.params.get(func_name).cloned() else {
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
        let call_target = self.functions.call_target(func_name).to_string();
        let plain_call = format!(
            "execute store result score {} {} run function {}:{}",
            target, target_obj, namespace, call_target
        );
        if param_names.is_empty() && self.current_params.is_empty() {
            return Ok(vec![plain_call]);
        }
        let staging = if self.caller_recursive {
            ArgumentStaging::Stack
        } else {
            ArgumentStaging::Site(self.next_temp_id())
        };
        if param_names.is_empty() {
            // The callee may call functions with parameters, which replace
            // `args`; see `Transpiler::preserving_args`.
            return Ok(staging.preserve(&namespace, plain_call));
        }

        let mut argument_commands = Vec::new();
        for (param_name, arg) in param_names.iter().zip(args.iter()) {
            let target_path = staging.argument_path(param_name);
            argument_commands.extend(self.function_argument_to_storage_commands(
                func_name,
                param_name,
                &target_path,
                arg,
            )?);
            let integers = self.functions.integer_argument_commands(
                func_name,
                param_name,
                &namespace,
                &target_path,
                arg,
            );
            if !integers.is_empty() {
                self.data_pack.track_objective("temp");
                argument_commands.extend(integers);
            }
        }
        Ok(staging.wrap(
            &namespace,
            !self.current_params.is_empty(),
            argument_commands,
            format!(
                "execute store result score {} {} run function {}:{} with storage {}:global args",
                target, target_obj, namespace, call_target, namespace
            ),
        ))
    }

    fn function_argument_to_storage_commands(
        &mut self,
        func_name: &str,
        param_name: &str,
        target_path: &str,
        arg: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        if self.numeric_backend
            && matches!(arg, Expression::Identifier(name) if self.symbols.compile_time_constants.contains_key(name) && matches!(self.symbols.variable_types.get(name), Some(CobbleType::Float | CobbleType::Double)) && !self.functions.params_used_as_values.get(func_name).is_some_and(|used| !used.contains(param_name)))
        {
            return self.numeric_value_commands(target_path, arg);
        }
        if self.numeric_backend
            && matches!(
                arg,
                Expression::Binary(..) | Expression::Call(..) | Expression::Unary(..)
            )
            && !matches!(
                arg,
                Expression::Binary(
                    _,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Lt
                        | BinaryOp::LtEq
                        | BinaryOp::Gt
                        | BinaryOp::GtEq
                        | BinaryOp::And
                        | BinaryOp::Or
                        | BinaryOp::In
                        | BinaryOp::NotIn,
                    _
                )
            )
        {
            return self.numeric_value_commands(target_path, arg);
        }
        // As a call statement passes it (see `call_argument_commands`).
        let number = match arg {
            Expression::Number(n) => Some(*n),
            Expression::Identifier(name)
                if !self.is_current_param(name)
                    && !self.symbols.scoreboard_variables.contains(name)
                    && !self.symbols.variable_storage_paths.contains_key(name) =>
            {
                self.symbols.compile_time_constants.get(name).copied()
            }
            Expression::Unary(..) => Transpiler::signed_number(arg),
            _ => None,
        };
        if let Some(n) = number {
            let (text, lossy) = self.functions.number_argument(func_name, param_name, n);
            if lossy {
                self.warn_score_value(n);
            }
            return Ok(vec![format!(
                "data modify storage {namespace}:global {target_path} set value {text}"
            )]);
        }
        match arg {
            Expression::String(value) => {
                let escaped = escape_quoted(value);
                Ok(vec![format!(
                    "data modify storage {}:global {} set value \"{}\"",
                    namespace, target_path, escaped
                )])
            }
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
                if let Some(source_path) = self.symbols.variable_storage_paths.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, source_path
                    )]);
                }
                if let Some(value) = self.symbols.compile_time_constants.get(name) {
                    return Ok(vec![format!(
                        "data modify storage {}:global {} set value {}",
                        namespace, target_path, *value as i32
                    )]);
                }
                let objective = self
                    .symbols
                    .variable_objectives
                    .get(name)
                    .map(String::as_str)
                    .unwrap_or("temp");
                Ok(vec![format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                    namespace, target_path, name, objective
                )])
            }
            Expression::Array(_) | Expression::Map(_) => {
                self.storage_value_commands(target_path, arg)
            }
            Expression::Subscript(base, index) if self.is_string_base(base) => {
                self.string_char_commands(target_path, base, index)
            }
            // A value whose type isn't a number keeps its storage value.
            Expression::Subscript(base, index)
                if !self.picks_field(base, index) && self.is_unknown_base(base) =>
            {
                self.unknown_base_item_commands(target_path, base, index)
            }
            Expression::Subscript(..) | Expression::Attribute(..)
                if (self.numeric_backend || self.storage_read_type(arg) != CobbleType::Integer)
                    && self.resolve_storage_path_plan(arg).is_some() =>
            {
                let plan = self.resolve_storage_path_plan(arg).expect("checked above");
                self.copy_storage_commands(target_path, &plan)
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
    pub fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
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
            return self.evaluate_through_scratch(expr, target, &target_obj);
        }

        // Every variable is a global score, so a function called inside the
        // expression may change `target`. Writing the first operand into it
        // before the call would let the call overwrite it
        // (`result = 1 + square(3)` where `square` sets `result`). Python
        // assigns after the whole right side runs; do the same through a
        // scratch score. Compiler scratch scores start with `#` and are never
        // written by a call, which also ends this recursion.
        if !target.starts_with('#') && Self::writes_target_before_call(expr) {
            return self.evaluate_through_scratch(expr, target, &target_obj);
        }

        match expr {
            Expression::Number(n) => {
                self.warn_score_value(*n);
                Ok(Self::number_to_target(*n, target, &target_obj))
            }
            Expression::Identifier(var) => self.identifier_to_target(var, target, &target_obj),
            Expression::Call(func, args) => self.call_to_target(func, args, target, &target_obj),
            // Array/map access `base[index]` and attribute access `obj.attr`
            // both read the storage path the expression resolves to.
            Expression::Subscript(..) | Expression::Attribute(..) => {
                self.storage_access_to_target(expr, target, &target_obj)
            }
            Expression::Binary(left, op, right) => {
                self.binary_to_target(left, op, right, target, &target_obj)
            }
            Expression::Unary(op, inner) => self.unary_to_target(op, inner, target, &target_obj),
            _ => Err("Unsupported expression type".into()),
        }
    }

    fn evaluate_through_scratch(
        &mut self,
        expr: &Expression,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let scratch = format!("#expr_result_{}", self.next_temp_id());
        let mut commands = self.evaluate_expression_to_target(expr, &scratch)?;
        commands.push(format!(
            "scoreboard players operation {} {} = {} temp",
            target, target_obj, scratch
        ));
        Ok(commands)
    }

    fn number_to_target(n: f64, target: &str, target_obj: &str) -> Vec<String> {
        vec![format!(
            "scoreboard players set {} {} {}",
            target, target_obj, n as i32
        )]
    }

    fn identifier_to_target(
        &self,
        var: &str,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(const_value) = self.symbols.compile_time_constants.get(var) {
            return Ok(vec![format!(
                "scoreboard players set {} {} {}",
                target, target_obj, *const_value as i32
            )]);
        }
        if let Some(storage_path) = self.symbols.variable_storage_paths.get(var) {
            self.reject_string_score_operand(var)?;
            // Load from storage to scoreboard
            return Ok(vec![format!(
                "execute store result score {} {} run data get storage {}:global {}",
                target, target_obj, self.data_pack.namespace, storage_path
            )]);
        }
        let var_obj = match self.symbols.variable_objectives.get(var) {
            Some(var_obj) => var_obj.as_str(),
            None if self.is_current_param(var) => {
                return Ok(vec![format!(
                    "execute store result score {} {} run data get storage {}:global args.{}",
                    target, target_obj, self.data_pack.namespace, var
                )]);
            }
            None => "temp",
        };
        // Skip no-op self-assignment (e.g. target temp = target temp)
        if var == target && var_obj == target_obj {
            return Ok(Vec::new());
        }
        Ok(vec![format!(
            "scoreboard players operation {} {} = {} {}",
            target, target_obj, var, var_obj
        )])
    }

    pub(in crate::transpiler) fn call_to_target(
        &mut self,
        func: &Expression,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if matches!(func, Expression::Identifier(name) if name == "len") {
            return self.len_to_target(args, target, target_obj);
        }
        if let Expression::Identifier(name) = func {
            if !self.functions.params.contains_key(name) {
                match name.as_str() {
                    "abs" => return self.abs_to_target(args, target, target_obj),
                    "min" | "max" => return self.extreme_to_target(name, args, target, target_obj),
                    _ => {}
                }
            }
        }

        // Handle intrinsics like math.sqrt
        if let Expression::Attribute(obj, method) = func {
            if matches!(&**obj, Expression::Identifier(module) if module == "math") {
                return self.math_call_to_target(method, args, target, target_obj);
            }
        }

        // Every other callee must be a user function with an integer return.
        let Some(func_name) = self
            .resolve_user_function_call_name(func)
            .map(str::to_string)
        else {
            return Err(
                "Only value intrinsics and user functions with `return` values can be used in expressions."
                    .into(),
            );
        };
        match self.functions.returns.get(&func_name) {
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
        self.user_function_call_to_target(&func_name, args, target, target_obj)
    }

    fn abs_to_target(
        &mut self,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let [value] = args else {
            return Err(format!("abs() takes 1 argument, got {}", args.len()).into());
        };
        let scratch = format!("#abs_value_{}", self.next_temp_id());
        let mut commands = self.evaluate_expression_to_target(value, &scratch)?;
        commands.extend([
            "scoreboard players set #abs_negate temp -1".to_string(),
            format!(
                "execute if score {scratch} temp matches ..-1 run scoreboard players operation {scratch} temp *= #abs_negate temp"
            ),
            format!("scoreboard players operation {target} {target_obj} = {scratch} temp"),
        ]);
        Ok(commands)
    }

    /// `min(a, b, ...)` or `max(a, b, ...)`. A single list argument is
    /// lowered to a prelude helper before this point.
    fn extreme_to_target(
        &mut self,
        name: &str,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let Some((first, rest)) = args.split_first() else {
            return Err(format!("{name}() takes at least 1 argument").into());
        };
        let operation = if name == "min" { "<" } else { ">" };
        let best = format!("#{name}_value_{}", self.next_temp_id());
        let mut commands = self.evaluate_expression_to_target(first, &best)?;
        for arg in rest {
            let scratch = format!("#{name}_arg_{}", self.next_temp_id());
            commands.extend(self.evaluate_expression_to_target(arg, &scratch)?);
            commands.push(format!(
                "scoreboard players operation {best} temp {operation} {scratch} temp"
            ));
        }
        commands.push(format!(
            "scoreboard players operation {target} {target_obj} = {best} temp"
        ));
        Ok(commands)
    }

    fn len_to_target(
        &mut self,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if args.len() != 1 {
            return Err(format!("len() takes 1 argument, got {}", args.len()).into());
        }
        let literal_len = match &args[0] {
            Expression::String(value) => Some(value.chars().count()),
            Expression::Array(items) => Some(items.len()),
            Expression::Map(entries) => Some(entries.len()),
            _ => None,
        };
        if let Some(len) = literal_len {
            return Ok(vec![format!(
                "scoreboard players set {} {} {}",
                target, target_obj, len
            )]);
        }
        // A character of text, or an item of a value only the running pack
        // knows, is copied out first: a path through it names nothing.
        if self.picks_computed_value(&args[0]) {
            let namespace = self.data_pack.namespace.clone();
            let copy = format!("_cobble_len_{}", self.next_temp_id());
            let mut commands = vec![format!("data remove storage {namespace}:global {copy}")];
            commands.extend(self.storage_leaf_set_commands(&copy, &args[0])?);
            commands.extend([
                format!(
                    "execute store result score {target} {target_obj} run data get storage {namespace}:global {copy}"
                ),
                format!("data remove storage {namespace}:global {copy}"),
            ]);
            return Ok(commands);
        }

        let Some(path_plan) = self.resolve_storage_path_plan(&args[0]) else {
            return Err(
                "len() argument must be a string, list, map, or storage-backed value"
                    .to_string()
                    .into(),
            );
        };
        self.read_storage_path_to_target(&path_plan, target, target_obj)
    }

    fn math_call_to_target(
        &mut self,
        op: &str,
        args: &[Expression],
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let mut commands = match op {
            "sqrt" | "abs" => {
                if args.len() != 1 {
                    return Err(format!("math.{} takes 1 argument", op).into());
                }
                self.data_pack.track_objective("temp");
                // A call inside a larger argument may run another math
                // helper, which overwrites `#math_input` after the rest of
                // the argument is written there, so such an argument is
                // computed into its own scratch first.
                if !matches!(args[0], Expression::Call(..)) && contains_call(&args[0]) {
                    let scratch = format!("#math_arg_{}", self.next_temp_id());
                    let mut commands = self.evaluate_expression_to_target(&args[0], &scratch)?;
                    commands.push(format!(
                        "scoreboard players operation #math_input temp = {} temp",
                        scratch
                    ));
                    commands
                } else {
                    self.evaluate_expression_to_target(&args[0], "#math_input")?
                }
            }
            "min" | "max" => {
                if args.len() != 2 {
                    return Err(format!("math.{} takes 2 arguments", op).into());
                }
                self.data_pack.track_objective("temp");

                // Evaluating an argument can itself run a math
                // helper (a nested math.* call), which stages
                // into the shared #math_input scores and would
                // clobber an argument staged earlier. Stage each
                // argument into a unique scratch and copy into
                // the helper inputs right before the call.
                let arg1_scratch = format!("#math_arg_{}", self.next_temp_id());
                let arg2_scratch = format!("#math_arg_{}", self.next_temp_id());
                let mut commands = self.evaluate_expression_to_target(&args[0], &arg1_scratch)?;
                commands.extend(self.evaluate_expression_to_target(&args[1], &arg2_scratch)?);
                commands.push(format!(
                    "scoreboard players operation #math_input temp = {} temp",
                    arg1_scratch
                ));
                commands.push(format!(
                    "scoreboard players operation #math_input2 temp = {} temp",
                    arg2_scratch
                ));
                commands
            }
            _ => return Err(format!("Unknown math function: {}", op).into()),
        };

        self.data_pack.ensure_math_helper(op);
        commands.push(format!(
            "function {}:_cobble_math_{}",
            self.data_pack.namespace, op
        ));
        self.data_pack.track_objective("temp");
        commands.push(format!(
            "scoreboard players operation {} {} = #math_result temp",
            target, target_obj
        ));
        Ok(commands)
    }

    fn storage_access_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let Some(path_plan) = self.resolve_storage_path_plan(expr) else {
            if let Expression::Attribute(base, field) = expr {
                if let Expression::Identifier(module) = &**base {
                    if !self.symbols.variable_storage_paths.contains_key(module)
                        && self.symbols.module_variables.contains(field)
                    {
                        return Err(format!(
                            "`{module}.{field}` names a variable of the module `{module}`, which can't be read through the module name. Import it with `from {module} import {field}` and use `{field}`."
                        )
                        .into());
                    }
                }
            }
            let message = if matches!(expr, Expression::Subscript(..)) {
                "Subscript base must resolve to a storage path (list/map)."
            } else {
                "Attribute base must resolve to a storage path"
            };
            return Err(message.to_string().into());
        };
        self.warn_fractional_item(expr);
        self.read_storage_path_to_target(&path_plan, target, target_obj)
    }

    /// Warns when an item or a field read into a score can hold a fraction
    /// in its literal, as `y[0]` after `y = [0.6, 2.5]`: the score keeps
    /// the integer part, as `x = 1.5` does.
    fn warn_fractional_item(&self, expr: &Expression) {
        // A `const` index picks one item, as a literal does.
        fn fold_indexes(expr: &Expression, constants: &dyn Fn(&str) -> Option<f64>) -> Expression {
            match expr {
                Expression::Attribute(base, field) => {
                    Expression::Attribute(Box::new(fold_indexes(base, constants)), field.clone())
                }
                Expression::Subscript(base, index) => {
                    let index = match &**index {
                        Expression::Identifier(name) => {
                            constants(name).map_or_else(|| (**index).clone(), Expression::Number)
                        }
                        _ => (**index).clone(),
                    };
                    Expression::Subscript(Box::new(fold_indexes(base, constants)), Box::new(index))
                }
                other => other.clone(),
            }
        }
        let folded = fold_indexes(expr, &|name| {
            self.symbols.compile_time_constants.get(name).copied()
        });
        let Some((name, steps)) = crate::transpiler::container_writes::access_path(&folded) else {
            return;
        };
        if self.is_storage_current_param(name) {
            return;
        }
        let Some(value) = self.symbols.variables.get(name).and_then(|literal| {
            crate::transpiler::container_writes::fractional_literal_at(literal, &steps)
        }) else {
            return;
        };
        // An index known only when the pack runs can pick any item.
        let picked = steps.iter().all(|step| match step {
            crate::transpiler::container_writes::Step::Item(index) => {
                matches!(index, Expression::Number(_))
            }
            _ => true,
        });
        let read = Transpiler::expression_source_key(expr);
        // `data get` rounds a fraction down.
        let message = if picked {
            format!(
                "Scores hold integers, so {read}, which holds {value}, is read as {}.",
                value.floor() as i32
            )
        } else {
            format!(
                "Scores hold integers, so {read}, which can hold a fraction such as {value}, is read rounded down."
            )
        };
        record_warning(
            self.warnings,
            TranspileWarning {
                kind: "fractional-number",
                message,
                help: Some(format!(
                    "Store the number as text, as \"{value}\", to show it as written, or scale it to an integer."
                )),
                location: self.source.clone(),
            },
        );
    }

    fn binary_to_target(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        if matches!(op, BinaryOp::Pow) && self.static_power_exponent(right).is_none() {
            return self.runtime_power_to_target(left, right, target, target_obj);
        }

        // For nested binary expressions like (a + b) + c:
        // First evaluate left side into target, then apply operation with right side
        let mut commands = self.evaluate_expression_to_target(left, target)?;

        // Now apply the operation with the right side
        match right {
            Expression::Number(n) => {
                self.warn_score_value(*n);
                let value = *n as i32;
                // Check for division and modulo by zero at compile time
                match op {
                    BinaryOp::Div | BinaryOp::FloorDiv if value == 0 => {
                        return Err(format!(
                            "Division by zero in expression: dividing by {}",
                            value
                        )
                        .into());
                    }
                    BinaryOp::Mod if value == 0 => {
                        return Err(
                            format!("Modulo by zero in expression: modulo by {}", value).into()
                        );
                    }
                    _ => {}
                }
                Self::apply_constant_operand(&mut commands, op, value, target, target_obj)?;
            }
            Expression::Identifier(var) => {
                self.apply_identifier_operand(&mut commands, op, var, target, target_obj)?;
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
                commands.extend(self.evaluate_expression_to_target(right, &right_temp)?);
                self.apply_score_operand(
                    &mut commands,
                    op,
                    &right_temp,
                    "temp",
                    target,
                    target_obj,
                )?;
            }
            _ => {
                return Err("Unsupported expression type in binary operation"
                    .to_string()
                    .into());
            }
        }
        Ok(commands)
    }

    fn runtime_power_to_target(
        &mut self,
        base: &Expression,
        exponent: &Expression,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let id = self.next_temp_id();
        let base_temp = format!("#expr_power_base_{}", id);
        let exponent_temp = format!("#expr_power_exp_{}", id);
        let mut commands = self.evaluate_expression_to_target(base, &base_temp)?;
        commands.extend(self.evaluate_expression_to_target(exponent, &exponent_temp)?);
        commands.push(format!(
            "scoreboard players operation {} {} = {} temp",
            target, target_obj, base_temp
        ));
        self.apply_runtime_power_to_target(
            &mut commands,
            target,
            target_obj,
            &exponent_temp,
            "temp",
        );
        Ok(commands)
    }

    /// Right operand that names a compile-time constant, a parameter of the
    /// current function, or a score variable.
    fn apply_identifier_operand(
        &mut self,
        commands: &mut Vec<String>,
        op: &BinaryOp,
        var: &str,
        target: &str,
        target_obj: &str,
    ) -> Result<(), TranspileError> {
        if let Some(const_value) = self.symbols.compile_time_constants.get(var) {
            let value = *const_value as i32;
            match op {
                BinaryOp::Div | BinaryOp::FloorDiv if value == 0 => {
                    return Err("Division by zero in expression".to_string().into());
                }
                BinaryOp::Mod if value == 0 => {
                    return Err("Modulo by zero in expression".to_string().into());
                }
                _ => {}
            }
            return Self::apply_constant_operand(commands, op, value, target, target_obj);
        }

        if self.is_current_param(var) {
            let temp_param = "#expr_param";
            commands.push(format!(
                "execute store result score {} temp run data get storage {}:global args.{}",
                temp_param, self.data_pack.namespace, var
            ));
            return self.apply_score_operand(commands, op, temp_param, "temp", target, target_obj);
        }

        self.reject_string_score_operand(var)?;
        // A list or map reads as its size, like on the left of an operator.
        if let Some(path) = self.symbols.variable_storage_paths.get(var).cloned() {
            let scratch = format!("#expr_temp_{}", self.next_temp_id());
            commands.push(format!(
                "execute store result score {} temp run data get storage {}:global {}",
                scratch, self.data_pack.namespace, path
            ));
            return self.apply_score_operand(commands, op, &scratch, "temp", target, target_obj);
        }
        let var_obj = self.target_objective(var).to_string();
        self.apply_score_operand(commands, op, var, &var_obj, target, target_obj)
    }

    /// Applies `op` with a right operand known at compile time to `target`,
    /// which already holds the left operand. Callers reject a zero divisor
    /// first, each with its own message.
    fn apply_constant_operand(
        commands: &mut Vec<String>,
        op: &BinaryOp,
        value: i32,
        target: &str,
        target_obj: &str,
    ) -> Result<(), TranspileError> {
        match op {
            BinaryOp::Add | BinaryOp::Sub if value == i32::MIN => {
                commands.extend(min_constant_commands(target, target_obj));
            }
            BinaryOp::Add | BinaryOp::Sub => {
                // `add` and `remove` only take amounts of 0 or more, so a
                // negative constant switches to the other one.
                let adds = matches!(op, BinaryOp::Add) == (value >= 0);
                commands.push(format!(
                    "scoreboard players {} {} {} {}",
                    if adds { "add" } else { "remove" },
                    target,
                    target_obj,
                    if value < 0 { -value } else { value }
                ));
            }
            BinaryOp::Mul | BinaryOp::Div | BinaryOp::FloorDiv | BinaryOp::Mod => {
                let (holder, operation) = match op {
                    BinaryOp::Mul => ("#multiplier", "*="),
                    BinaryOp::Div | BinaryOp::FloorDiv => ("#divisor", "/="),
                    _ => ("#modulus", "%="),
                };
                commands.push(format!("scoreboard players set {} temp {}", holder, value));
                commands.push(format!(
                    "scoreboard players operation {} {} {} {} temp",
                    target, target_obj, operation, holder
                ));
            }
            BinaryOp::Pow => {
                // Compile-time expansion: x^n becomes x*x*...*x (n times)
                if value < 0 {
                    return Err("Power exponent must be non-negative".to_string().into());
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
                    // x^1 = x, nothing to do (target already holds left side)
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
            _ => return Err(unsupported_operator(op)),
        }
        Ok(())
    }

    /// Applies `op` with the score `operand operand_obj` to `target`, which
    /// already holds the left operand.
    fn apply_score_operand(
        &mut self,
        commands: &mut Vec<String>,
        op: &BinaryOp,
        operand: &str,
        operand_obj: &str,
        target: &str,
        target_obj: &str,
    ) -> Result<(), TranspileError> {
        let operation = match op {
            BinaryOp::Add => "+=",
            BinaryOp::Sub => "-=",
            BinaryOp::Mul => "*=",
            BinaryOp::Div | BinaryOp::FloorDiv => "/=",
            BinaryOp::Mod => "%=",
            BinaryOp::Pow => {
                self.apply_runtime_power_to_target(
                    commands,
                    target,
                    target_obj,
                    operand,
                    operand_obj,
                );
                return Ok(());
            }
            _ => return Err(unsupported_operator(op)),
        };
        commands.push(format!(
            "scoreboard players operation {} {} {} {} {}",
            target, target_obj, operation, operand, operand_obj
        ));
        Ok(())
    }

    fn unary_to_target(
        &mut self,
        op: &UnaryOp,
        inner: &Expression,
        target: &str,
        target_obj: &str,
    ) -> Result<Vec<String>, TranspileError> {
        match op {
            UnaryOp::Neg => {
                // Unary negation: -expr
                // Evaluate the expression first, then multiply by -1
                let mut commands = self.evaluate_expression_to_target(inner, target)?;

                // Multiply by -1 using temp objective fake player
                commands.push("scoreboard players set #neg_const temp -1".to_string());
                commands.push(format!(
                    "scoreboard players operation {} {} *= #neg_const temp",
                    target, target_obj
                ));
                Ok(commands)
            }
            // Unary plus: +expr (no-op, just evaluate the expression)
            UnaryOp::Pos => self.evaluate_expression_to_target(inner, target),
            UnaryOp::Not => Err(
                "Logical NOT operator cannot be used in arithmetic expressions"
                    .to_string()
                    .into(),
            ),
        }
    }
}

fn contains_call(expr: &Expression) -> bool {
    match expr {
        Expression::Call(..) => true,
        Expression::Binary(left, _, right) => contains_call(left) || contains_call(right),
        Expression::Unary(_, inner) => contains_call(inner),
        Expression::Attribute(obj, _) => contains_call(obj),
        Expression::Subscript(obj, index) => contains_call(obj) || contains_call(index),
        _ => false,
    }
}

/// `+ i32::MIN` or `- i32::MIN` on `target`. 2^31 doesn't fit `add` or
/// `remove`; with 32-bit wrapping, adding and subtracting it give the same
/// score.
pub(in crate::transpiler) fn min_constant_commands(target: &str, target_obj: &str) -> Vec<String> {
    vec![
        format!("scoreboard players set #addend temp {}", i32::MIN),
        format!(
            "scoreboard players operation {} {} += #addend temp",
            target, target_obj
        ),
    ]
}
