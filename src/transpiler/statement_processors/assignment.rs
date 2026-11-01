use crate::ast::*;
use crate::transpiler::expression_evaluator::{min_constant_commands, ExpressionEvaluator};
use crate::transpiler::function_context::expression_reads;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{escape_quoted, GeneratedCommandKind, ModuleLevelAssignment};
use crate::transpiler::{StoragePathPlan, TranspileError, Transpiler};

/// Where a read whose path is known only at run time lands first, so a
/// missing value can be told apart.
const READ_SCRATCH: &str = "_cobble_read";

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
/// `Transpiler::classify_assignment_value`; the variants are listed in the
/// order they are checked (see `Transpiler::process_assignment`).
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
    /// A character of a string variable, `s[i]`, stored as a string.
    StringChar,
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
            let lines = path_plan.helper_lines(id, |path| {
                vec![format!(
                    "$execute store result score {} temp run data get storage {}:global {}",
                    target_score, namespace, path
                )]
            });
            self.add_generated_helper_function(
                helper_name.clone(),
                lines,
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
            let lines = path_plan.helper_lines(id, |path| {
                vec![format!(
                    "$execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    namespace, path, target_score
                )]
            });
            self.add_generated_helper_function(
                helper_name.clone(),
                lines,
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
            let mut base_reads = Vec::new();
            expression_reads(target_expr, &mut base_reads);
            let base_pending = base_reads
                .first()
                .is_some_and(|base| self.symbols.pending_module_declarations.contains(*base));
            if base_pending || self.resolve_storage_path_plan(target_expr).is_none() {
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

    /// The type a plain assignment gives its variable, from the lowered
    /// value. `register_module_declarations` uses it too, so a module
    /// variable recorded early gets the type its assignment gives it.
    pub(in crate::transpiler) fn assigned_value_type(&self, value: &Expression) -> CobbleType {
        match value {
            Expression::Subscript(base, _) if self.is_string_base(base) => CobbleType::String,
            Expression::Subscript(..) | Expression::Attribute(..) => self.storage_read_type(value),
            value => self.infer_type(value),
        }
    }

    /// Process a plain `target = value` assignment statement.
    ///
    /// The steps run in this order, and reordering them changes the output:
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

        if assign.persist {
            if !matches!(target_kind, AssignTargetKind::ModuleLevel) {
                return Err(format!(
                    "`persist {}` is only allowed at module level. Move it out of the function body.",
                    assign.target
                )
                .into());
            }
            if !self.is_persist_initializer(&assign.value) {
                return Err(format!(
                    "`persist {}` needs a constant initializer: a number, boolean, string, list, or map literal.",
                    assign.target
                )
                .into());
            }
        }

        if self.current_function.is_none() {
            self.check_module_level_reads(&assign.value)?;
        }

        if let AssignTargetKind::StorageExpr(target_expr) = target_kind {
            return self.process_storage_target_assignment(target_expr, &assign.value);
        }

        if matches!(target_kind, AssignTargetKind::LocalVar)
            && self.current_context.is_param(&assign.target)
        {
            return self.process_parameter_assignment(assign);
        }

        // Infer the type of the value being assigned. A value of unknown
        // type, such as a parameter, keeps the variable's type.
        let inferred = self.assigned_value_type(&assign.value);
        let value_type = match (inferred, self.symbols.variable_types.get(&assign.target)) {
            (CobbleType::Unknown, Some(existing)) => existing.clone(),
            (value_type, _) => value_type,
        };
        // A module variable recorded before the function bodies were
        // lowered (see `register_module_declarations`) must keep that type,
        // since the functions above used it.
        if matches!(target_kind, AssignTargetKind::ModuleLevel)
            && self
                .symbols
                .pending_module_declarations
                .contains(&assign.target)
        {
            if let Some(recorded) = self.symbols.variable_types.get(&assign.target) {
                if *recorded != value_type
                    && *recorded != CobbleType::Unknown
                    && value_type != CobbleType::Unknown
                {
                    return Err(format!(
                        "Cobble can't tell the type of the module variable `{}` before this line, and functions above it use it. Move `{} = ...` above those functions.",
                        assign.target, assign.target
                    )
                    .into());
                }
            }
        }

        // Check if this assignment is type-safe
        self.check_type_assignment(&assign.target, &value_type)?;

        // Record the variable's type
        self.symbols
            .variable_types
            .insert(assign.target.clone(), value_type);

        // Store the variable value for later use
        self.symbols
            .variables
            .insert(assign.target.clone(), assign.value.clone());

        // If we're not in a function, store as module-level variable
        // These will be automatically initialized in the _cobble_init function
        if matches!(target_kind, AssignTargetKind::ModuleLevel) {
            self.symbols
                .pending_module_declarations
                .remove(&assign.target);
            self.symbols.late_module_declarations.remove(&assign.target);
            self.defer_module_level_assignment(assign);
            return Ok(());
        }

        if let Some(read) = self.storage_read(&assign.target, &assign.value) {
            return self.lower_storage_read(&assign.target, read);
        }

        // A score value replaces a storage one. A copy of a storage
        // variable and a character of a string stay in storage, and may
        // read the target itself, as in `s = s[0]`.
        let storage_value = match &assign.value {
            Expression::Array(_) | Expression::Map(_) | Expression::String(_) => true,
            Expression::Identifier(name) => {
                self.symbols.variable_storage_paths.contains_key(name)
                    && !self.copies_a_score(&assign.target, name)
            }
            Expression::Subscript(base, _) => self.is_string_base(base),
            _ => false,
        };
        let replaced_copy = if storage_value {
            None
        } else {
            self.symbols.variable_storage_paths.remove(&assign.target)
        };

        match self.classify_assignment_value(&assign.target, &assign.value) {
            AssignValueKind::ConstFoldable(const_value) => {
                self.lower_const_fold_assignment(assign, const_value);
            }
            AssignValueKind::ConditionFlag => self.lower_condition_flag_assignment(assign)?,
            AssignValueKind::ComplexEval | AssignValueKind::ParamCopy => {
                self.lower_expression_eval_assignment(assign)?
            }
            AssignValueKind::StorageLiteral => self.lower_storage_literal_assignment(assign)?,
            AssignValueKind::StringChar => self.lower_string_char_assignment(assign)?,
            AssignValueKind::ScoreSimple => self.lower_simple_score_assignment(assign)?,
        }

        // Code already lowered can read the old copy on a later pass of a
        // loop, as `/say {a}` before `a = a + 1`, so the copy stays current.
        if let Some(path) = replaced_copy {
            let namespace = self.data_pack.namespace.clone();
            self.extend_current_function(vec![format!(
                "execute store result storage {namespace}:global {path} int 1 run scoreboard players get {} temp",
                assign.target
            )]);
        }
        Ok(())
    }

    /// How `target = value` reads `value` from storage, when it copies a
    /// value whose type isn't a number: a parameter, an item of a list, or
    /// a field of a map. A number goes through the score path instead.
    fn storage_read(&self, target: &str, value: &Expression) -> Option<StorageRead> {
        let target_type = self
            .symbols
            .variable_types
            .get(target)
            .cloned()
            .unwrap_or(CobbleType::Unknown);
        if matches!(target_type, CobbleType::Integer | CobbleType::Boolean)
            || self.current_context.score_writes.contains(target)
        {
            return None;
        }
        // A value of unknown type also gets a score, for arithmetic.
        let score = !matches!(
            target_type,
            CobbleType::String | CobbleType::List | CobbleType::Map
        );
        match value {
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Some(StorageRead::Path {
                    plan: StoragePathPlan::static_path(format!("args.{name}")),
                    score,
                })
            }
            // A copy of a value whose type only the running pack knows, as
            // a helper's result for a slice of a parameter.
            Expression::Identifier(name)
                if name != target
                    && matches!(
                        self.symbols.variable_types.get(name),
                        None | Some(CobbleType::Unknown)
                    )
                    && !self.symbols.scoreboard_variables.contains(name)
                    && !self.symbols.variable_objectives.contains_key(name) =>
            {
                self.symbols
                    .variable_storage_paths
                    .get(name)
                    .map(|path| StorageRead::Path {
                        plan: StoragePathPlan::static_path(path.clone()),
                        score,
                    })
            }
            Expression::Subscript(base, _) if self.is_string_base(base) => None,
            Expression::Subscript(base, index)
                if !self.picks_field(base, index) && self.is_unknown_base(base) =>
            {
                Some(StorageRead::Item {
                    base: (**base).clone(),
                    index: (**index).clone(),
                    score,
                })
            }
            Expression::Subscript(..) | Expression::Attribute(..) => self
                .resolve_storage_path_plan(value)
                .map(|plan| StorageRead::Path { plan, score }),
            _ => None,
        }
    }

    /// Copy a value read from storage into `target`'s storage, and into its
    /// score when the value's type is unknown.
    fn lower_storage_read(
        &mut self,
        target: &str,
        read: StorageRead,
    ) -> Result<(), TranspileError> {
        let target_path = self.variable_storage_path(target);
        let namespace = self.data_pack.namespace.clone();
        // A missing item or field gives an empty value of the target's type,
        // or 0, as `pop()` on an empty list does; the target never keeps a
        // value from an earlier call.
        let empty = match self.symbols.variable_types.get(target) {
            Some(CobbleType::String) => "\"\"",
            Some(CobbleType::List) => "[]",
            Some(CobbleType::Map) => "{}",
            _ => "0",
        };
        let (mut commands, score) = match read {
            StorageRead::Path { plan, score } => match plan.as_static() {
                Some(source) => {
                    // A whole parameter is there whenever the call ran.
                    let whole_parameter = source
                        .strip_prefix("args.")
                        .is_some_and(|name| !name.contains(['.', '[']));
                    let mut commands = Vec::new();
                    if !whole_parameter {
                        commands.push(format!(
                            "execute unless data storage {namespace}:global {source} run data modify storage {namespace}:global {target_path} set value {empty}"
                        ));
                    }
                    commands.extend(self.copy_storage_commands(&target_path, &plan)?);
                    (commands, score)
                }
                None => {
                    let commands = self.copy_storage_commands(READ_SCRATCH, &plan)?;
                    (
                        self.read_through_scratch(commands, &target_path, empty),
                        score,
                    )
                }
            },
            StorageRead::Item { base, index, score } => {
                let commands = self.unknown_base_item_commands(READ_SCRATCH, &base, &index)?;
                (
                    self.read_through_scratch(commands, &target_path, empty),
                    score,
                )
            }
        };
        self.symbols
            .variable_storage_paths
            .insert(target.to_string(), target_path.clone());
        if score {
            self.data_pack.track_objective("temp");
            self.symbols
                .variable_objectives
                .insert(target.to_string(), "temp".to_string());
            self.symbols.scoreboard_variables.insert(target.to_string());
            commands.push(format!(
                "execute store result score {target} temp run data get storage {}:global {target_path}",
                self.data_pack.namespace
            ));
        } else {
            self.symbols.scoreboard_variables.remove(target);
            self.symbols.variable_objectives.remove(target);
        }
        self.extend_current_function(commands);
        Ok(())
    }

    /// `read` copies a value to [`READ_SCRATCH`]; these commands then give
    /// `target_path` that value, or `empty` when the read found nothing.
    fn read_through_scratch(
        &self,
        read: Vec<String>,
        target_path: &str,
        empty: &str,
    ) -> Vec<String> {
        let namespace = &self.data_pack.namespace;
        let mut commands = vec![format!(
            "data remove storage {namespace}:global {READ_SCRATCH}"
        )];
        commands.extend(read);
        commands.extend([
            format!(
                "execute unless data storage {namespace}:global {READ_SCRATCH} run data modify storage {namespace}:global {READ_SCRATCH} set value {empty}"
            ),
            format!(
                "data modify storage {namespace}:global {target_path} set from storage {namespace}:global {READ_SCRATCH}"
            ),
        ]);
        commands
    }

    /// Whether `target = source` copies a score: the target holds a number
    /// and the source has a score, as the target of a list loop does next
    /// to its storage copy.
    fn copies_a_score(&self, target: &str, source: &str) -> bool {
        let holds_a_number = matches!(
            self.symbols.variable_types.get(target),
            Some(CobbleType::Integer | CobbleType::Boolean)
        ) || self.current_context.score_writes.contains(target);
        holds_a_number
            && (self.symbols.scoreboard_variables.contains(source)
                || self.symbols.variable_objectives.contains_key(source))
    }

    /// Assign to a parameter of the current function. Parameters live in
    /// the `args` storage the function was called with, so the new value is
    /// written there and every later read of the parameter sees it.
    fn process_parameter_assignment(&mut self, assign: &Assignment) -> Result<(), TranspileError> {
        let path = format!("args.{}", assign.target);
        let namespace = self.data_pack.namespace.clone();
        let storage_source = match &assign.value {
            Expression::Identifier(name) if self.current_context.is_param(name) => {
                Some(format!("args.{name}"))
            }
            Expression::Identifier(name) => self.symbols.variable_storage_paths.get(name).cloned(),
            _ => None,
        };
        let commands = match &assign.value {
            Expression::String(_) | Expression::Array(_) | Expression::Map(_) => {
                self.storage_value_commands(&path, &assign.value)?
            }
            Expression::None => {
                return Err(format!(
                    "Parameter `{}` can't be assigned `None`, which storage can't hold.",
                    assign.target
                )
                .into());
            }
            // A character, an item, or a field: storage keeps a string as a
            // string.
            Expression::Subscript(..) | Expression::Attribute(..) => {
                self.storage_value_commands(&path, &assign.value)?
            }
            // A copy of a string, list, map, or another parameter.
            _ if storage_source.is_some() => vec![format!(
                "data modify storage {namespace}:global {path} set from storage {namespace}:global {}",
                storage_source.unwrap_or_default()
            )],
            _ => {
                let scratch = format!("#param_value_{}", self.next_temp_id());
                self.process_assignment(&Assignment {
                    target: scratch.clone(),
                    target_expr: None,
                    value: assign.value.clone(),
                    persist: false,
                })?;
                vec![format!(
                    "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                    self.data_pack.namespace, path, scratch
                )]
            }
        };
        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }
        Ok(())
    }

    /// Classify where an assignment's *target* routes it. See the
    /// decision-order comment on [`Self::process_assignment`].
    fn classify_assignment_target<'a>(&self, assign: &'a Assignment) -> AssignTargetKind<'a> {
        if let Some(target_expr) = assign
            .target_expr
            .as_ref()
            .filter(|_| !assign.assigns_name())
        {
            return AssignTargetKind::StorageExpr(target_expr);
        }
        if self.current_function.is_none() {
            AssignTargetKind::ModuleLevel
        } else {
            AssignTargetKind::LocalVar
        }
    }

    /// Classify an in-function assignment *value*. The checks run in the
    /// order listed on [`Self::process_assignment`].
    fn classify_assignment_value(&self, target: &str, value: &Expression) -> AssignValueKind {
        // Try to evaluate constant expressions first
        if let Some(const_value) = self.try_eval_const(value) {
            return AssignValueKind::ConstFoldable(const_value);
        }

        if Self::is_runtime_condition_expression(value) {
            return AssignValueKind::ConditionFlag;
        }

        if matches!(value, Expression::Subscript(base, _) if self.is_string_base(base)) {
            return AssignValueKind::StringChar;
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
        self.register_module_variable_backing(assign);
        self.module_level_vars
            .push(ModuleLevelAssignment::Variable {
                target: assign.target.clone(),
                value: assign.value.clone(),
                source: self.current_statement_source.clone(),
                persist: assign.persist,
            });
    }

    /// Records whether the module variable `assign` binds lives in storage,
    /// in a score, or in both. Its type must already be recorded.
    pub(in crate::transpiler) fn register_module_variable_backing(&mut self, assign: &Assignment) {
        // Strings, lists, and maps live in storage: literals, copies of
        // such variables, and characters of strings.
        let storage = matches!(
            &assign.value,
            Expression::Array(_) | Expression::Map(_) | Expression::String(_)
        ) || matches!(
            self.symbols.variable_types.get(&assign.target),
            Some(CobbleType::String | CobbleType::List | CobbleType::Map)
        );
        // An item or field of unknown type keeps a score next to its
        // storage copy, like a list loop target.
        let both = matches!(
            &assign.value,
            Expression::Subscript(..) | Expression::Attribute(..)
        ) && matches!(
            self.symbols.variable_types.get(&assign.target),
            None | Some(CobbleType::Unknown)
        );
        if storage || both {
            let storage_path = format!("vars.{}", assign.target);
            self.symbols
                .variable_storage_paths
                .insert(assign.target.clone(), storage_path);
        }
        if both || !storage {
            self.data_pack.track_objective("temp");
            self.symbols
                .variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            self.symbols
                .scoreboard_variables
                .insert(assign.target.clone());
        }
    }

    /// Lower `AssignValueKind::ConstFoldable`: the value folded to a
    /// compile-time constant, so emit a direct `scoreboard players set`.
    fn lower_const_fold_assignment(&mut self, assign: &Assignment, const_value: f64) {
        // The expression is a constant - fold it at compile time
        if self.current_function.is_some() {
            self.warn_score_value(const_value);
        }
        if let Some(ref mut commands) = self.current_function {
            self.data_pack.track_objective("temp");
            self.symbols
                .variable_objectives
                .insert(assign.target.clone(), "temp".to_string());
            self.symbols
                .scoreboard_variables
                .insert(assign.target.clone());

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
        self.symbols
            .variable_objectives
            .insert(assign.target.clone(), "temp".to_string());
        self.symbols
            .scoreboard_variables
            .insert(assign.target.clone());

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

    /// Lower `AssignValueKind::ComplexEval` and `AssignValueKind::ParamCopy`:
    /// route the value through the recursive expression evaluator.
    fn lower_expression_eval_assignment(
        &mut self,
        assign: &Assignment,
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(assign.target.clone(), "temp".to_string());
        self.symbols
            .scoreboard_variables
            .insert(assign.target.clone());
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
        let storage_path = self.variable_storage_path(&assign.target);
        self.symbols
            .variable_storage_paths
            .insert(assign.target.clone(), storage_path.clone());

        let value_commands = self.storage_value_commands(&storage_path, &assign.value)?;

        if let Some(ref mut commands) = self.current_function {
            commands.extend(value_commands);
        }
        Ok(())
    }

    /// Lower `AssignValueKind::StringChar`: copy one character of a string
    /// variable into the target's storage.
    fn lower_string_char_assignment(&mut self, assign: &Assignment) -> Result<(), TranspileError> {
        let Expression::Subscript(base, index) = &assign.value else {
            unreachable!("classified as a string character");
        };
        let storage_path = self.variable_storage_path(&assign.target);
        let value_commands = self.string_char_commands(&storage_path, base, index)?;
        self.symbols
            .variable_storage_paths
            .insert(assign.target.clone(), storage_path);
        self.symbols.scoreboard_variables.remove(&assign.target);
        self.symbols.variable_objectives.remove(&assign.target);
        if let Some(ref mut commands) = self.current_function {
            commands.extend(value_commands);
        }
        Ok(())
    }

    /// Lower `AssignValueKind::ScoreSimple`: direct scoreboard commands for
    /// literals, identifier copies, and simple atom-op-atom arithmetic.
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

        // Outside a function body there is nothing to emit, and none of the
        // errors below apply at module level.
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
                // `True` and `False` are scores 1 and 0.
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(assign.target.clone(), "temp".to_string());
                self.symbols
                    .scoreboard_variables
                    .insert(assign.target.clone());
                let val = if *b { 1 } else { 0 };
                commands.push(format!(
                    "scoreboard players set {} temp {}",
                    assign.target, val
                ));
            }
            Expression::Number(n) => {
                // Direct number assignment
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(assign.target.clone(), "temp".to_string());
                self.symbols
                    .scoreboard_variables
                    .insert(assign.target.clone());

                self.warn_score_value(*n);
                let score = *n as i32;
                commands.push(format!(
                    "scoreboard players set {} temp {}",
                    assign.target, score
                ));
            }
            Expression::Identifier(var) => {
                // A storage variable is copied in storage, unless a number
                // goes to a number: the target of a list loop has both.
                let src_path_opt = self
                    .symbols
                    .variable_storage_paths
                    .get(var)
                    .filter(|_| !self.copies_a_score(&assign.target, var))
                    .cloned();
                if let Some(src_path) = src_path_opt {
                    // Storage copy: target = src
                    // Mark target as storage variable
                    let target_path = self.variable_storage_path(&assign.target);
                    self.symbols
                        .variable_storage_paths
                        .insert(assign.target.clone(), target_path.clone());

                    let namespace = self.data_pack.namespace.clone();
                    commands.push(format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, target_path, namespace, src_path
                    ));
                    // A list loop target of unknown type holds a score next
                    // to its storage copy; the target keeps both.
                    let source_has_score = self.symbols.scoreboard_variables.contains(var);
                    let target_is_storage_type = matches!(
                        self.symbols.variable_types.get(&assign.target),
                        Some(CobbleType::String | CobbleType::List | CobbleType::Map)
                    );
                    if source_has_score && !target_is_storage_type {
                        self.data_pack.track_objective("temp");
                        self.symbols
                            .variable_objectives
                            .insert(assign.target.clone(), "temp".to_string());
                        self.symbols
                            .scoreboard_variables
                            .insert(assign.target.clone());
                        commands.push(format!(
                            "scoreboard players operation {} temp = {} temp",
                            assign.target, var
                        ));
                    }
                } else {
                    // Variable-to-variable assignment (Scoreboard)
                    self.data_pack.track_objective("temp");
                    self.symbols
                        .variable_objectives
                        .insert(assign.target.clone(), "temp".to_string());
                    self.symbols
                        .scoreboard_variables
                        .insert(assign.target.clone());

                    let var_obj = self
                        .symbols
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
                // `classify_assignment_value` routes nested operands and every
                // `^` (constant or runtime exponent) to the expression
                // evaluator, so only atom-op-atom arithmetic reaches here.
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(assign.target.clone(), "temp".to_string());
                self.symbols
                    .scoreboard_variables
                    .insert(assign.target.clone());

                if !matches!(
                    (&**left, &**right),
                    (
                        Expression::Identifier(_) | Expression::Number(_),
                        Expression::Identifier(_) | Expression::Number(_)
                    )
                ) {
                    return Err(format!(
                        "Unsupported binary operation: {:?} between {:?} and {:?}",
                        op, left, right
                    )
                    .into());
                }
                commands.extend(self.score_binary_commands(assign, left, op, right)?);
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

    /// One side of an atom-op-atom assignment: a score, or a number known at
    /// compile time (a literal or a `const`).
    fn score_operand<'e>(&self, expr: &'e Expression) -> ScoreOperand<'e> {
        match expr {
            Expression::Identifier(name) => match self.symbols.compile_time_constants.get(name) {
                Some(value) => ScoreOperand::Constant {
                    value: *value as i32,
                    literal: false,
                },
                None => ScoreOperand::Score {
                    holder: name,
                    objective: self
                        .symbols
                        .variable_objectives
                        .get(name)
                        .cloned()
                        .unwrap_or_else(|| "temp".to_string()),
                },
            },
            Expression::Number(n) => {
                self.warn_score_value(*n);
                ScoreOperand::Constant {
                    value: *n as i32,
                    literal: true,
                }
            }
            _ => unreachable!("simple score assignments have identifier or number operands"),
        }
    }

    /// `target = left <op> right` with scoreboard commands, where each side
    /// is a score or a compile-time number.
    fn score_binary_commands(
        &mut self,
        assign: &Assignment,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let target = assign.target.as_str();
        let lhs = self.score_operand(left);
        let rhs = self.score_operand(right);

        if matches!(op, BinaryOp::Div | BinaryOp::Mod)
            && matches!(rhs, ScoreOperand::Constant { value: 0, .. })
        {
            let (name, symbol) = if matches!(op, BinaryOp::Div) {
                ("Division", "/")
            } else {
                ("Modulo", "%")
            };
            let place = if matches!(lhs, ScoreOperand::Constant { .. }) {
                "constant expression: ".to_string()
            } else {
                format!("assignment: {} = ", target)
            };
            return Err(format!(
                "{} by zero in {}{} {} {}",
                name,
                place,
                operand_text(left),
                symbol,
                operand_text(right)
            )
            .into());
        }

        let set = |value: i32| format!("scoreboard players set {} temp {}", target, value);
        let copy = |holder: &str, objective: &str| {
            format!(
                "scoreboard players operation {} temp = {} {}",
                target, holder, objective
            )
        };
        let mut commands = Vec::new();
        match (lhs, rhs) {
            (ScoreOperand::Constant { value: a, .. }, ScoreOperand::Constant { value: b, .. }) => {
                let folded = crate::transpiler::const_eval::fold_score_operation(op, a, b)
                    .unwrap_or_default();
                commands.push(set(folded));
            }
            (ScoreOperand::Score { holder, objective }, ScoreOperand::Constant { value, .. }) => {
                // `x = x + 1` needs no copy.
                if target != holder || objective != "temp" {
                    commands.push(copy(holder, &objective));
                }
                commands.extend(Self::apply_constant_to_score(target, op, value));
            }
            (
                ScoreOperand::Constant {
                    value,
                    literal: true,
                },
                ScoreOperand::Score { holder, objective },
            ) if matches!(op, BinaryOp::Add) => {
                commands.push(copy(holder, &objective));
                commands.extend(Self::apply_constant_to_score(target, op, value));
            }
            (ScoreOperand::Constant { value, .. }, ScoreOperand::Score { holder, objective }) => {
                if matches!(op, BinaryOp::Mul) {
                    commands.push(copy(holder, &objective));
                    commands.extend(Self::apply_constant_to_score(target, op, value));
                } else if let Some(operation) = score_operation(op) {
                    commands.push(set(value));
                    commands.push(format!(
                        "scoreboard players operation {} temp {} {} {}",
                        target, operation, holder, objective
                    ));
                }
            }
            (
                ScoreOperand::Score {
                    holder: left_holder,
                    objective: left_objective,
                },
                ScoreOperand::Score { holder, objective },
            ) => {
                if target != left_holder || left_objective != "temp" {
                    commands.push(copy(left_holder, &left_objective));
                }
                if let Some(operation) = score_operation(op) {
                    commands.push(format!(
                        "scoreboard players operation {} temp {} {} {}",
                        target, operation, holder, objective
                    ));
                }
            }
        }
        Ok(commands)
    }

    /// Applies `<op> value` to the score `target`. `+` and `-` use
    /// `add`/`remove`; the other operators go through a constant holder.
    fn apply_constant_to_score(target: &str, op: &BinaryOp, value: i32) -> Vec<String> {
        let value_abs = i64::from(value).abs();
        let add = || format!("scoreboard players add {} temp {}", target, value_abs);
        let remove = || format!("scoreboard players remove {} temp {}", target, value_abs);
        let through_holder = |holder: &str, operation: &str| {
            vec![
                format!("scoreboard players set {} temp {}", holder, value),
                format!(
                    "scoreboard players operation {} temp {} {} temp",
                    target, operation, holder
                ),
            ]
        };
        match op {
            BinaryOp::Add | BinaryOp::Sub if value == i32::MIN => {
                min_constant_commands(target, "temp")
            }
            BinaryOp::Add if value < 0 => vec![remove()],
            BinaryOp::Add => vec![add()],
            BinaryOp::Sub if value < 0 => vec![add()],
            BinaryOp::Sub => vec![remove()],
            BinaryOp::Mul => through_holder("#multiplier", "*="),
            BinaryOp::Div => through_holder("#divisor", "/="),
            BinaryOp::Mod => through_holder("#modulus", "%="),
            _ => Vec::new(),
        }
    }

    pub(in crate::transpiler) fn is_runtime_condition_expression(expr: &Expression) -> bool {
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
        let snbt = serialize_snbt(&self.symbols, expr)?;
        for value in out_of_range_numbers(&self.symbols, expr) {
            self.warn_score_value(value);
        }
        Ok(snbt)
    }

    /// A map literal key as SNBT. Minecraft refuses an empty key.
    pub(in crate::transpiler) fn serialize_snbt_key(key: &str) -> Result<String, TranspileError> {
        if key.is_empty() {
            return Err(crate::transpiler::storage_paths::EMPTY_KEY_ERROR.into());
        }
        if key
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '+')
        {
            return Ok(key.to_string());
        }

        let escaped = escape_quoted(key);
        Ok(format!("\"{}\"", escaped))
    }
}

/// The whole numbers in a list or map literal that a score can't hold.
/// Cobble keeps every whole number as a score does, so each is stored
/// clamped, and gets the warning `x = 3000000000` gets.
pub(in crate::transpiler) fn out_of_range_numbers(
    symbols: &Symbols,
    expr: &Expression,
) -> Vec<f64> {
    fn collect(symbols: &Symbols, expr: &Expression, out: &mut Vec<f64>) {
        let value = match expr {
            Expression::Number(n) => *n,
            Expression::Identifier(name) => match symbols.compile_time_constants.get(name) {
                Some(value) => *value,
                None => return,
            },
            Expression::Array(items) => {
                items.iter().for_each(|item| collect(symbols, item, out));
                return;
            }
            Expression::Map(entries) => {
                entries
                    .iter()
                    .for_each(|(_, value)| collect(symbols, value, out));
                return;
            }
            _ => return,
        };
        if value.fract() == 0.0 && (value > f64::from(i32::MAX) || value < f64::from(i32::MIN)) {
            out.push(value);
        }
    }
    let mut out = Vec::new();
    collect(symbols, expr, &mut out);
    out
}

/// A whole number as SNBT, clamped to a score's range as Cobble keeps every
/// whole number (see [`out_of_range_numbers`]).
fn whole_number_snbt(n: f64) -> String {
    format!("{}", n as i32)
}

/// SNBT text for a literal or constant value.
pub(in crate::transpiler) fn serialize_snbt(
    symbols: &Symbols,
    expr: &Expression,
) -> Result<String, TranspileError> {
    match expr {
        Expression::Number(n) => {
            if n.fract() == 0.0 {
                Ok(whole_number_snbt(*n))
            } else {
                Ok(format!("{}f", n))
            }
        }
        Expression::String(s) => {
            let escaped = escape_quoted(s);
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
                result.push_str(&serialize_snbt(symbols, item)?);
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
                result.push_str(&Transpiler::serialize_snbt_key(key)?);
                result.push(':');
                result.push_str(&serialize_snbt(symbols, value)?);
            }
            result.push('}');
            Ok(result)
        }
        Expression::Identifier(name) => {
            if let Some(val) = symbols.compile_time_constants.get(name) {
                // A const past a score's range would be refused by
                // Minecraft written as is.
                if val.fract() == 0.0 {
                    Ok(whole_number_snbt(*val))
                } else {
                    Ok(format!("{}", val))
                }
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

/// A value an assignment copies from storage.
enum StorageRead {
    /// The value at a path: a parameter, `names[i]`, or `config.title`.
    Path { plan: StoragePathPlan, score: bool },
    /// `base[index]` of a value that can be a list or a string.
    Item {
        base: Expression,
        index: Expression,
        score: bool,
    },
}

/// One side of an atom-op-atom score assignment.
enum ScoreOperand<'a> {
    Score {
        holder: &'a str,
        objective: String,
    },
    /// `literal` tells `5 + x` from `LIMIT + x`; the two compile to
    /// different but equivalent command orders.
    Constant {
        value: i32,
        literal: bool,
    },
}

/// The `scoreboard players operation` operator for an arithmetic operator.
fn score_operation(op: &BinaryOp) -> Option<&'static str> {
    match op {
        BinaryOp::Add => Some("+="),
        BinaryOp::Sub => Some("-="),
        BinaryOp::Mul => Some("*="),
        BinaryOp::Div => Some("/="),
        BinaryOp::Mod => Some("%="),
        _ => None,
    }
}

/// Source text of an atom operand, for error messages.
fn operand_text(expr: &Expression) -> String {
    match expr {
        Expression::Identifier(name) => name.clone(),
        Expression::Number(n) => n.to_string(),
        other => format!("{:?}", other),
    }
}
