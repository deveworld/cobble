use crate::ast::*;
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{GeneratedCommandKind, ModuleLevelAssignment};
use crate::transpiler::{TranspileError, Transpiler};

/// How an assignment's *target* routes it. Produced by
/// `Transpiler::classify_assignment_target`; the variants are checked in
/// this order.
#[derive(Clone, Copy)]
enum AssignTargetKind<'a> {
    /// `target_expr` is present and is not the bare identifier `target`: a
    /// storage-backed write such as `config.count = ...` or `items[i] = ...`.
    StorageExpr(&'a Expression),
    /// Plain-variable assignment outside any function body; deferred into
    /// the `_cobble_init` replay as a `ModuleLevelAssignment::Variable`.
    ModuleLevel,
    /// Plain-variable assignment inside a function body.
    LocalVar,
}

/// How an in-function assignment's *value* is lowered. Produced by
/// `Transpiler::classify_assignment_value`; the variants are listed in
/// classification order, which is load-bearing (see the decision-order
/// comment on `Transpiler::process_assignment`).
enum AssignValueKind {
    /// The whole value folds to a compile-time number.
    ConstFoldable(f64),
    /// Runtime comparison/boolean expression lowered to a 0/1 flag.
    ConditionFlag,
    /// Needs the recursive expression evaluator: Pow, macro-param operands,
    /// nested operands, unary ops, calls, attribute/subscript reads.
    ComplexEval,
    /// Bare identifier naming a macro parameter; also routed through the
    /// expression evaluator.
    ParamCopy,
    /// Array/Map/String literal materialized into NBT storage.
    StorageLiteral,
    /// Everything else: the simple scoreboard ladder.
    ScoreSimple,
}

impl Transpiler {
    pub(in crate::transpiler) fn storage_target_assignment_commands(
        &mut self,
        target_expr: &Expression,
        value: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let Some(path_plan) = self.resolve_storage_path_plan(target_expr) else {
            return Err(format!(
                "Assignment target `{target_expr:?}` must resolve to a storage-backed value. \
                 Assign to a storage-backed list/map/string field such as `config.count` or `items[i]`."
            ).into());
        };

        if let Some((op, right)) = Self::storage_compound_assignment_parts(target_expr, value) {
            return self.storage_compound_assignment_commands(&path_plan, op, right);
        }

        Ok(
            if let Some(static_path) = path_plan.as_static().map(str::to_string) {
                self.storage_value_commands(&static_path, value)?
            } else {
                let (mut commands, value_path) = self.materialize_storage_value(value)?;
                let namespace = self.data_pack.namespace.clone();
                commands.extend(self.dynamic_storage_command(
                    &path_plan,
                    GeneratedCommandKind::ControlFlow,
                    |path| {
                        vec![format!(
                            "$data modify storage {}:global {} set from storage {}:global {}",
                            namespace, path, namespace, value_path
                        )]
                    },
                )?);
                commands.push(format!(
                    "data remove storage {}:global {}",
                    self.data_pack.namespace, value_path
                ));
                commands
            },
        )
    }

    fn storage_compound_assignment_parts<'a>(
        target_expr: &Expression,
        value: &'a Expression,
    ) -> Option<(&'a BinaryOp, &'a Expression)> {
        let Expression::Binary(left, op, right) = value else {
            return None;
        };
        if left.as_ref() != target_expr || matches!(op, BinaryOp::Pow) {
            return None;
        }
        Some((op, right))
    }

    fn storage_compound_assignment_commands(
        &mut self,
        path_plan: &crate::transpiler::StoragePathPlan,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let id = self.next_temp_id();
        let namespace = self.data_pack.namespace.clone();
        let target_score = format!("#storage_target_{}", id);
        let rhs_score = format!("#storage_rhs_{}", id);
        self.data_pack.track_objective("temp");

        let mut commands = Vec::new();
        let dynamic_args_path = if let Some(static_path) = path_plan.as_static() {
            commands.push(format!(
                "execute store result score {} temp run data get storage {}:global {}",
                target_score, namespace, static_path
            ));
            None
        } else {
            let (prepare_commands, args_path) = self.prepare_dynamic_storage_path(id, path_plan)?;
            commands.extend(prepare_commands);
            let helper_name = format!("_cobble_storage_dyn_read_{}", id);
            self.add_generated_helper_function(
                helper_name.clone(),
                vec![format!(
                    "$execute store result score {} temp run data get storage {}:global {}",
                    target_score,
                    namespace,
                    path_plan.template()
                )],
                GeneratedCommandKind::ControlFlow,
            );
            commands.push(format!(
                "function {}:{} with storage {}:global {}",
                namespace, helper_name, namespace, args_path
            ));
            Some(args_path)
        };

        commands.extend(self.evaluate_expression_to_target(right, &rhs_score)?);
        let operation = match op {
            BinaryOp::Add => "+=",
            BinaryOp::Sub => "-=",
            BinaryOp::Mul => "*=",
            BinaryOp::Div => "/=",
            BinaryOp::Mod => "%=",
            _ => {
                return Err(
                    format!("Unsupported compound storage assignment operator: {:?}", op).into(),
                )
            }
        };
        commands.push(format!(
            "scoreboard players operation {} temp {} {} temp",
            target_score, operation, rhs_score
        ));

        if let Some(static_path) = path_plan.as_static() {
            commands.push(format!(
                "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                namespace, static_path, target_score
            ));
        } else if let Some(args_path) = dynamic_args_path {
            let helper_name = format!("_cobble_storage_dyn_write_{}", id);
            self.add_generated_helper_function(
                helper_name.clone(),
                vec![format!(
                    "$execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    namespace,
                    path_plan.template(),
                    target_score
                )],
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
        }

        Ok(commands)
    }

    fn process_storage_target_assignment(
        &mut self,
        target_expr: &Expression,
        value: &Expression,
    ) -> Result<(), TranspileError> {
        if self.current_function.is_none() {
            if self.resolve_storage_path_plan(target_expr).is_none() {
                return Err(format!(
                    "Assignment target `{target_expr:?}` must resolve to an already-defined storage-backed value. \
                     Define the base list, map, or string before assigning to one of its fields."
                ).into());
            }
            self.module_level_vars
                .push(ModuleLevelAssignment::StorageTarget {
                    target_expr: target_expr.clone(),
                    value: value.clone(),
                    source: self.current_statement_source.clone(),
                });
            return Ok(());
        }

        let commands = self.storage_target_assignment_commands(target_expr, value)?;

        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }

        Ok(())
    }

    /// Process a plain `target = value` assignment statement.
    ///
    /// DECISION ORDER (load-bearing -- this mirrors the historical if/else
    /// ladder exactly; reordering any step changes user-visible behavior):
    ///
    /// 1. Storage-expression targets (`config.x = ...`, `items[i] = ...`)
    ///    short-circuit to the storage-target path before any type
    ///    bookkeeping happens.
    /// 2. The type check runs before module-level deferral, so ill-typed
    ///    module-level assignments still error at their own statement.
    /// 3. Module-level assignments defer to the `_cobble_init` replay before
    ///    any value classification or codegen.
    /// 4. A non-storage value clears the target's stale storage-path binding
    ///    before the value is classified.
    /// 5. Const folding runs before condition-flag lowering: a constant
    ///    comparison (`f = 2 > 1`) folds to 0/1 instead of emitting
    ///    `execute`-based flag commands.
    /// 6. Condition-flag lowering runs before the complex-eval check --
    ///    comparisons are `Binary` expressions too and would otherwise be
    ///    routed to the expression evaluator.
    /// 7. The complex-eval check runs before the manual scoreboard ladder;
    ///    only simple atom-op-atom arithmetic stays on the ladder.
    /// 8. A bare macro-parameter identifier routes through the expression
    ///    evaluator instead of the ladder's identifier copy.
    /// 9. Storage literals (Array/Map/String) materialize into NBT storage
    ///    before the ladder, which reads its operands as scores.
    pub(in crate::transpiler) fn process_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), TranspileError> {
        let target_kind = self.classify_assignment_target(assign);

        if let AssignTargetKind::StorageExpr(target_expr) = target_kind {
            return self.process_storage_target_assignment(target_expr, &assign.value);
        }

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
        if matches!(target_kind, AssignTargetKind::ModuleLevel) {
            self.defer_module_level_assignment(assign);
            return Ok(());
        }

        if !matches!(
            &assign.value,
            Expression::Array(_) | Expression::Map(_) | Expression::String(_)
        ) {
            self.variable_storage_paths.remove(&assign.target);
        }

        match self.classify_assignment_value(&assign.target, &assign.value) {
            AssignValueKind::ConstFoldable(const_value) => {
                self.lower_const_fold_assignment(assign, const_value);
                Ok(())
            }
            AssignValueKind::ConditionFlag => self.lower_condition_flag_assignment(assign),
            AssignValueKind::ComplexEval | AssignValueKind::ParamCopy => {
                self.lower_expression_eval_assignment(assign)
            }
            AssignValueKind::StorageLiteral => self.lower_storage_literal_assignment(assign),
            AssignValueKind::ScoreSimple => self.lower_simple_score_assignment(assign),
        }
    }

    /// Classify where an assignment's *target* routes it. See the
    /// decision-order comment on [`Self::process_assignment`].
    fn classify_assignment_target<'a>(&self, assign: &'a Assignment) -> AssignTargetKind<'a> {
        if let Some(target_expr) = assign.target_expr.as_ref() {
            if !matches!(target_expr, Expression::Identifier(name) if name == &assign.target) {
                return AssignTargetKind::StorageExpr(target_expr);
            }
        }
        if self.current_function.is_none() {
            AssignTargetKind::ModuleLevel
        } else {
            AssignTargetKind::LocalVar
        }
    }

    /// Classify an in-function assignment *value*. The check order is
    /// load-bearing; see the decision-order comment on
    /// [`Self::process_assignment`].
    fn classify_assignment_value(&self, target: &str, value: &Expression) -> AssignValueKind {
        // Try to evaluate constant expressions first
        if let Some(const_value) = self.try_eval_const(value) {
            return AssignValueKind::ConstFoldable(const_value);
        }

        if Self::is_runtime_condition_expression(value) {
            return AssignValueKind::ConditionFlag;
        }

        let needs_complex_eval = match value {
            Expression::Binary(left, op, right) => {
                // Pow always goes through the expression evaluator (both
                // constant and runtime exponents), as do expressions touching
                // macro params. Simple binary (atom op atom) stays on the
                // optimized ladder below; nested expressions need the
                // expression evaluator.
                matches!(op, BinaryOp::Pow)
                    // The ladder writes the left operand into the target
                    // before applying the right one, so `x = 10 - x` or
                    // `x = y - x` would read the clobbered target. The
                    // evaluator routes self-referential right operands
                    // through a scratch score.
                    || matches!(&**right, Expression::Identifier(name) if name == target)
                    || Self::expr_contains_param(left, &self.current_context.params)
                    || Self::expr_contains_param(right, &self.current_context.params)
                    || !matches!(
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
            return AssignValueKind::ComplexEval;
        }

        if matches!(value, Expression::Identifier(var) if self.current_context.params.iter().any(|param| param == var))
        {
            return AssignValueKind::ParamCopy;
        }

        if matches!(
            value,
            Expression::Array(_) | Expression::Map(_) | Expression::String(_)
        ) {
            return AssignValueKind::StorageLiteral;
        }

        AssignValueKind::ScoreSimple
    }

    /// Defer a module-level `target = value` into the `_cobble_init`
    /// replay (`ModuleLevelAssignment::Variable`), recording the target's
    /// storage or scoreboard backing up front.
    fn defer_module_level_assignment(&mut self, assign: &Assignment) {
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
        self.module_level_vars
            .push(ModuleLevelAssignment::Variable {
                target: assign.target.clone(),
                value: assign.value.clone(),
                source: self.current_statement_source.clone(),
            });
    }

    /// Lower `AssignValueKind::ConstFoldable`: the value folded to a
    /// compile-time constant, so emit a direct `scoreboard players set`.
    fn lower_const_fold_assignment(&mut self, assign: &Assignment, const_value: f64) {
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
    }

    /// Lower `AssignValueKind::ConditionFlag`: evaluate a runtime
    /// comparison or boolean expression into a 0/1 scoreboard flag.
    fn lower_condition_flag_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        self.variable_objectives
            .insert(assign.target.clone(), "temp".to_string());
        self.scoreboard_variables.insert(assign.target.clone());

        let processed_condition = self.preprocess_condition(&assign.value)?;
        let translated = self.translate_condition(&processed_condition)?;
        let condition_cmd = self.lower_translated_condition(translated)?;
        let condition_execute_args = Self::condition_execute_args(&condition_cmd);

        // The flag is reset before the condition is tested, so a condition
        // that reads the target itself (`flag = not flag`) is computed into a
        // scratch score first and copied over afterwards.
        let reads_target =
            ExpressionEvaluator::expression_reads_target(&assign.value, &assign.target);
        let flag = if reads_target {
            format!("#cond_flag_{}", self.next_temp_id())
        } else {
            assign.target.clone()
        };

        if let Some(ref mut commands) = self.current_function {
            commands.push(format!("scoreboard players set {} temp 0", flag));
            commands.push(format!(
                "execute {} run scoreboard players set {} temp 1",
                condition_execute_args, flag
            ));
            if reads_target {
                commands.push(format!(
                    "scoreboard players operation {} temp = {} temp",
                    assign.target, flag
                ));
            }
        }
        Ok(())
    }

    /// Lower `AssignValueKind::ComplexEval` and `AssignValueKind::ParamCopy`
    /// (their historical lowerings were identical): route the value through
    /// the recursive expression evaluator.
    fn lower_expression_eval_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        self.variable_objectives
            .insert(assign.target.clone(), "temp".to_string());
        self.scoreboard_variables.insert(assign.target.clone());
        let expr_commands = self.evaluate_expression_to_target(&assign.value, &assign.target)?;

        if let Some(ref mut commands) = self.current_function {
            commands.extend(expr_commands);
        }
        Ok(())
    }

    /// Lower `AssignValueKind::StorageLiteral`: materialize an
    /// Array/Map/String literal into NBT storage and record the target's
    /// storage binding.
    fn lower_storage_literal_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), TranspileError> {
        let storage_path = format!("vars.{}", assign.target);
        self.variable_storage_paths
            .insert(assign.target.clone(), storage_path.clone());

        let value_commands = self.storage_value_commands(&storage_path, &assign.value)?;

        if let Some(ref mut commands) = self.current_function {
            commands.extend(value_commands);
        }
        Ok(())
    }

    /// Lower `AssignValueKind::ScoreSimple`: the historical manual
    /// scoreboard ladder for literals, identifier copies, and simple
    /// atom-op-atom binary arithmetic.
    fn lower_simple_score_assignment(&mut self, assign: &Assignment) -> Result<(), TranspileError> {
        // Scoreboard arithmetic below reads identifier operands as scores; a
        // storage-backed string's score mirror only holds the string length.
        if let Expression::Binary(left, _, right) = &assign.value {
            for operand in [&**left, &**right] {
                if let Expression::Identifier(name) = operand {
                    self.reject_string_score_operand(name)?;
                }
            }
        }

        // The historical ladder emitted nothing outside a function body (its
        // command block was guarded on `current_function`), and none of the
        // ladder's errors fired at module level either.
        if self.current_function.is_none() {
            return Ok(());
        }

        let commands = self.simple_score_commands(assign)?;
        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }
        Ok(())
    }

    /// Build the scoreboard-ladder commands for one simple assignment.
    fn simple_score_commands(
        &mut self,
        assign: &Assignment,
    ) -> Result<Vec<String>, TranspileError> {
        let mut commands = Vec::new();
        match &assign.value {
            Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                // Storage literals are routed to
                // `lower_storage_literal_assignment` by
                // `classify_assignment_value`; nothing to emit here.
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
                // Binary operation (`classify_assignment_value` already routed
                // complex shapes to the expression evaluator, so this is simple).
                // NOTE: BinaryOp::Pow never reaches this ladder — `needs_complex_eval`
                // routes every Pow assignment to the expression evaluator, which
                // handles both constant and runtime exponents.
                self.data_pack.track_objective("temp");
                self.variable_objectives
                    .insert(assign.target.clone(), "temp".to_string());
                self.scoreboard_variables.insert(assign.target.clone());

                match (&**left, &**right) {
                    (Expression::Identifier(var), Expression::Number(n)) => {
                        self.score_binary_var_num_commands(&mut commands, assign, var, op, n)?
                    }
                    (Expression::Number(n1), Expression::Number(n2)) => {
                        Self::score_binary_num_num_commands(&mut commands, assign, n1, op, n2)?
                    }
                    (Expression::Number(n), Expression::Identifier(var)) => {
                        self.score_binary_num_var_commands(&mut commands, assign, n, op, var)?
                    }
                    (Expression::Identifier(var1), Expression::Identifier(var2)) => {
                        self.score_binary_var_var_commands(&mut commands, assign, var1, op, var2)?
                    }
                    _ => {
                        // Unsupported combination
                        return Err(format!(
                            "Unsupported binary operation: {:?} between {:?} and {:?}",
                            op, left, right
                        )
                        .into());
                    }
                }
            }
            Expression::Call(func, _args) => {
                // Value-returning user calls are handled by the expression
                // evaluator (`ComplexEval`).
                // Reaching this branch means the call is not assignable as a score value.
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
                        "Cannot assign this function call result to a variable.\n\n\
                        Attempted: {} = {}()\n\n\
                        Only Cobble user functions with integer `return` values can be assigned in expressions.\n\n\
                        Solutions:\n\
                        1. If this is a side-effect function, call it separately:\n\
                           {}()\n\
                           # Then read any state it updates explicitly\n\n\
                        2. If you need an expression value, return an integer score:\n\
                           def {}():\n\
                               return 42\n\n\
                        3. For storage/string/list/map data, write to storage and read it through a storage-backed variable.",
                        assign.target, func_name, func_name, func_name
                    ).into());
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
                )
                .into());
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
                )
                .into());
            }
        }

        Ok(commands)
    }

    /// Scoreboard ladder: `target = var <op> number`.
    fn score_binary_var_num_commands(
        &mut self,
        commands: &mut Vec<String>,
        assign: &Assignment,
        var: &str,
        op: &BinaryOp,
        n: &f64,
    ) -> Result<(), TranspileError> {
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
                commands.push(format!("scoreboard players set #multiplier temp {}", value));
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
                    )
                    .into());
                }
                // Optimization: Skip self-assignment if target == var
                if assign.target != *var || var_obj != "temp" {
                    commands.push(format!(
                        "scoreboard players operation {} temp = {} {}",
                        assign.target, var, var_obj
                    ));
                }
                commands.push(format!("scoreboard players set #divisor temp {}", value));
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
                    )
                    .into());
                }
                // Optimization: Skip self-assignment if target == var
                if assign.target != *var || var_obj != "temp" {
                    commands.push(format!(
                        "scoreboard players operation {} temp = {} {}",
                        assign.target, var, var_obj
                    ));
                }
                commands.push(format!("scoreboard players set #modulus temp {}", value));
                commands.push(format!(
                    "scoreboard players operation {} temp %= #modulus temp",
                    assign.target
                ));
            }
            _ => {}
        }
        Ok(())
    }

    /// Scoreboard ladder: `target = number <op> number` that escaped const
    /// folding (folding declines division/modulo by zero, which error here).
    fn score_binary_num_num_commands(
        commands: &mut Vec<String>,
        assign: &Assignment,
        n1: &f64,
        op: &BinaryOp,
        n2: &f64,
    ) -> Result<(), TranspileError> {
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
                    )
                    .into());
                }
                *n1 / *n2
            }
            BinaryOp::Mod => {
                if *n2 == 0.0 {
                    return Err(
                        format!("Modulo by zero in constant expression: {} % {}", n1, n2).into(),
                    );
                }
                (*n1 as i32 % *n2 as i32) as f64
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
        Ok(())
    }

    /// Scoreboard ladder: `target = number <op> var`.
    fn score_binary_num_var_commands(
        &mut self,
        commands: &mut Vec<String>,
        assign: &Assignment,
        n: &f64,
        op: &BinaryOp,
        var: &str,
    ) -> Result<(), TranspileError> {
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
                commands.push(format!("scoreboard players set #multiplier temp {}", value));
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
                                            ).into());
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
                                            ).into());
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
            _ => {}
        }
        Ok(())
    }

    /// Scoreboard ladder: `target = var <op> var`, with either side possibly
    /// a compile-time constant.
    fn score_binary_var_var_commands(
        &mut self,
        commands: &mut Vec<String>,
        assign: &Assignment,
        var1: &str,
        op: &BinaryOp,
        var2: &str,
    ) -> Result<(), TranspileError> {
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
                    commands.push(format!("scoreboard players set #multiplier temp {}", num));
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
                    commands.push(format!("scoreboard players set #multiplier temp {}", num));
                    commands.push(format!(
                        "scoreboard players operation {} temp *= #multiplier temp",
                        assign.target
                    ));
                }
                BinaryOp::Div => {
                    if num == 0 {
                        return Err("Division by zero in assignment".into());
                    }
                    commands.push(format!("scoreboard players set #divisor temp {}", num));
                    commands.push(format!(
                        "scoreboard players operation {} temp /= #divisor temp",
                        assign.target
                    ));
                }
                BinaryOp::Mod => {
                    if num == 0 {
                        return Err("Modulo by zero in assignment".into());
                    }
                    commands.push(format!("scoreboard players set #modulus temp {}", num));
                    commands.push(format!(
                        "scoreboard players operation {} temp %= #modulus temp",
                        assign.target
                    ));
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
                            return Err(format!(
                                                    "Division by zero: Variable '{}' has constant value 0.\n\
                                                    \n\
                                                    Division by zero causes undefined behavior in Minecraft.\n\
                                                    Solution: Check the divisor before division:\n\
                                                    \n\
                                                    if {} != 0:\n\
                                                        {} = {} / {}",
                                                    var2, var2, assign.target, assign.target, var2
                                                ).into());
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
                            return Err(format!(
                                                    "Modulo by zero: Variable '{}' has constant value 0.\n\
                                                    \n\
                                                    Modulo by zero causes undefined behavior in Minecraft.\n\
                                                    Solution: Check the divisor before modulo:\n\
                                                    \n\
                                                    if {} != 0:\n\
                                                        {} = {} % {}",
                                                    var2, var2, assign.target, assign.target, var2
                                                ).into());
                        }
                    }
                    commands.push(format!(
                        "scoreboard players operation {} temp %= {} {}",
                        assign.target, var2, var2_obj
                    ));
                }
                _ => {}
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

    fn expr_contains_param(expr: &Expression, params: &[String]) -> bool {
        match expr {
            Expression::Identifier(name) => params.iter().any(|param| param == name),
            Expression::Binary(left, _, right) => {
                Self::expr_contains_param(left, params) || Self::expr_contains_param(right, params)
            }
            Expression::Unary(_, inner) => Self::expr_contains_param(inner, params),
            Expression::Call(func, args) => {
                Self::expr_contains_param(func, params)
                    || args
                        .iter()
                        .any(|arg| Self::expr_contains_param(arg, params))
            }
            Expression::Attribute(obj, _) => Self::expr_contains_param(obj, params),
            Expression::Subscript(obj, index) => {
                Self::expr_contains_param(obj, params) || Self::expr_contains_param(index, params)
            }
            Expression::Array(items) => items
                .iter()
                .any(|item| Self::expr_contains_param(item, params)),
            Expression::Map(entries) => entries
                .iter()
                .any(|(_, value)| Self::expr_contains_param(value, params)),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn serialize_to_snbt(
        &self,
        expr: &Expression,
    ) -> Result<String, TranspileError> {
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
                "None/null values are not supported in Minecraft SNBT storage literals because NBT has no null type. Use a data pack JSON resource helper when you need JSON null, or choose an explicit sentinel value for storage.".into(),
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
                    Err(format!("Runtime variable '{}' cannot be serialized as static SNBT. Use the storage runtime materialization path or a compile-time constant.", name).into())
                }
            }
            _ => Err(format!(
                "Unsupported expression in SNBT serialization: {:?}",
                expr
            ).into()),
        }
    }

    pub(in crate::transpiler) fn serialize_snbt_key(key: &str) -> String {
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
