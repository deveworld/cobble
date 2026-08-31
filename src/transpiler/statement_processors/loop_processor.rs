use crate::ast::*;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{
    FunctionCapture, GeneratedCommand, GeneratedCommandKind, SourceLocation, StoragePathPlan,
    TranspileError, TranspileErrorKind, Transpiler,
};
use std::collections::HashMap;

const UNROLL_LIMIT: usize = 1024;
const UNROLL_WARNING_THRESHOLD: usize = 256;
const UNROLL_NESTED_ITERATION_LIMIT: usize = 65_536;
const UNROLL_GENERATED_COMMAND_LIMIT: usize = 65_536;

#[derive(Debug, Clone)]
enum UnrollValue {
    Number(f64),
    String(String),
    Boolean(bool),
}

#[derive(Debug, Clone)]
struct RuntimeRangePlan {
    start: Expression,
    stop: Expression,
    step: RuntimeRangeStep,
    reverse_single_arg_on_negative_step: bool,
}

#[derive(Debug, Clone)]
struct RuntimeStorageListPlan {
    path: StoragePathPlan,
    setup_commands: Vec<String>,
    cleanup_commands: Vec<String>,
}

#[derive(Debug, Clone)]
enum RuntimeRangeStep {
    Static(i32),
    Dynamic(Expression),
}

type LoopDriverHook<'a, T> = Box<dyn FnOnce(&mut Transpiler) -> Result<T, TranspileError> + 'a>;

/// Loop body produced by a [`RuntimeLoopDriver`] body hook.
struct RuntimeLoopBody {
    statements: Vec<Statement>,
    /// Commands prepended to the captured body function before it is emitted
    /// (e.g. binding the current list item to the loop target).
    binding_commands: Vec<String>,
}

/// Configuration for [`Transpiler::emit_runtime_loop`], the shared driver
/// behind runtime `for range(...)` loops, storage-list `for` loops, and
/// `while` loops: emit init commands, capture the condition, capture the body
/// into a helper function, then emit the recursive loop helper with return
/// propagation.
struct RuntimeLoopDriver<'a> {
    /// Helper-name prefix: the loop helper is named `{prefix}_temp_{id}` and
    /// the body helper `{prefix}_body_{id}`.
    helper_prefix: &'static str,
    /// `while` historically allocates the loop helper id before lowering its
    /// condition; the runtime `for` drivers allocate it afterwards. Preserved
    /// so generated helper names stay stable.
    loop_helper_id_before_condition: bool,
    /// Commands emitted into the current function before the loop starts.
    init_commands: LoopDriverHook<'a, Vec<String>>,
    /// Builds the per-iteration condition: `execute` args plus the captured
    /// setup commands re-run before every check.
    condition: LoopDriverHook<'a, (String, FunctionCapture)>,
    /// Builds the loop body statements (with any step advancement appended)
    /// and the binding prelude commands.
    body: LoopDriverHook<'a, RuntimeLoopBody>,
    /// Commands appended after the loop call in the current function.
    cleanup_commands: Vec<String>,
}

#[derive(Debug, Clone)]
struct VariableBindingSnapshot {
    value: Option<Expression>,
    objective: Option<String>,
    is_scoreboard: bool,
    storage_path: Option<String>,
    compile_time_constant: Option<f64>,
    value_type: Option<CobbleType>,
}

impl UnrollValue {
    fn as_expression(&self) -> Expression {
        match self {
            Self::Number(value) => Expression::Number(*value),
            Self::String(value) => Expression::String(value.clone()),
            Self::Boolean(value) => Expression::Boolean(*value),
        }
    }

    fn as_command_text(&self) -> String {
        match self {
            Self::Number(value) => format_unroll_number(*value),
            Self::String(value) => value.clone(),
            Self::Boolean(value) => value.to_string(),
        }
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn process_for(
        &mut self,
        for_loop: &ForLoop,
    ) -> Result<(), TranspileError> {
        let loop_source_key = Self::for_loop_source_key(for_loop);
        let values = match self.unroll_values(for_loop) {
            Ok(values) => values,
            Err(error) if error.kind == TranspileErrorKind::UnrollNonLiteral => {
                if Self::is_range_iterable(&for_loop.iter) {
                    return self
                        .process_runtime_range_for(for_loop)
                        .map_err(|runtime_error| {
                            annotate_runtime_for_error(
                                runtime_error,
                                &for_loop.target,
                                &loop_source_key,
                                self.current_statement_source.as_ref(),
                            )
                        });
                }
                if self.runtime_storage_list_candidate(&for_loop.iter) {
                    return self.process_runtime_storage_list_for(for_loop).map_err(
                        |runtime_error| {
                            annotate_runtime_for_error(
                                runtime_error,
                                &for_loop.target,
                                &loop_source_key,
                                self.current_statement_source.as_ref(),
                            )
                        },
                    );
                }
                return Err(annotate_unroll_error(
                    error,
                    &for_loop.target,
                    &loop_source_key,
                    self.current_statement_source.as_ref(),
                ));
            }
            Err(error) => {
                return Err(annotate_unroll_error(
                    error,
                    &for_loop.target,
                    &loop_source_key,
                    self.current_statement_source.as_ref(),
                ));
            }
        };
        let previous_iteration_factor = self.unroll_iteration_factor;
        let projected_iterations = previous_iteration_factor
            .checked_mul(values.len())
            .ok_or_else(|| {
                annotate_unroll_error(
                    TranspileError::new(TranspileErrorKind::UnrollNested, format!(
                        "unroll-limit-exceeded: nested unrolling expands past {} aggregate iterations.",
                        UNROLL_NESTED_ITERATION_LIMIT
                    )),
                    &for_loop.target,
                    &loop_source_key,
                    self.current_statement_source.as_ref(),
                )
            })?;
        if projected_iterations > UNROLL_NESTED_ITERATION_LIMIT {
            return Err(annotate_unroll_error(
                TranspileError::new(TranspileErrorKind::UnrollNested, format!(
                    "unroll-limit-exceeded: nested unrolling expands to {} aggregate iterations (limit {}).",
                    projected_iterations, UNROLL_NESTED_ITERATION_LIMIT
                )),
                &for_loop.target,
                &loop_source_key,
                self.current_statement_source.as_ref(),
            ));
        }
        if values.len() > UNROLL_WARNING_THRESHOLD {
            eprintln!(
                "warning: unroll-large-expansion: for {} expands to {} iterations",
                for_loop.target,
                values.len()
            );
        }

        let is_outermost_unroll = self.unroll_depth == 0;
        if is_outermost_unroll {
            self.unroll_command_baseline = Some(self.generated_command_count());
        }
        self.unroll_depth += 1;
        self.unroll_iteration_factor = projected_iterations;

        let result = (|| {
            self.data_pack.unrolled_loops += 1;
            let body_sources = for_loop
                .body
                .iter()
                .map(|statement| self.take_unrolled_body_statement_source(statement))
                .collect::<Vec<_>>();
            for value in values {
                let substituted = substitute_statements(&for_loop.body, &for_loop.target, &value)?;
                for (statement, body_source) in substituted.into_iter().zip(body_sources.iter()) {
                    let saved_statement_source = self.current_statement_source.clone();
                    if body_source.is_some() {
                        self.current_statement_source = body_source.clone();
                    }
                    let start_index = self.current_function.as_ref().map(Vec::len);
                    let process_result = self.process_statement(&statement);
                    if let Some(start_index) = start_index {
                        self.mark_current_function_commands_as_unrolled(start_index);
                    }
                    self.current_statement_source = saved_statement_source;
                    process_result?;
                    self.ensure_unroll_command_budget()?;
                }
            }
            Ok(())
        })();

        self.unroll_depth -= 1;
        self.unroll_iteration_factor = previous_iteration_factor;
        if is_outermost_unroll {
            self.unroll_command_baseline = None;
        }

        result.map_err(|error| {
            annotate_unroll_error(
                error,
                &for_loop.target,
                &loop_source_key,
                self.current_statement_source.as_ref(),
            )
        })
    }

    fn is_range_iterable(iter: &Expression) -> bool {
        matches!(iter, Expression::Call(func, _) if is_identifier(func, "range"))
    }

    fn process_runtime_range_for(&mut self, for_loop: &ForLoop) -> Result<(), TranspileError> {
        let plan = self.runtime_range_plan(for_loop)?;
        validate_runtime_loop_body_target(&for_loop.body, &for_loop.target)?;

        if self.current_context.is_param(&for_loop.target) {
            return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                "runtime-for-unsupported: dynamic range loop target '{}' cannot shadow a function parameter.",
                for_loop.target
            )));
        }

        let stop_var = format!("#for_range_stop_{}", self.next_temp_id());
        let step_var = if matches!(plan.step, RuntimeRangeStep::Dynamic(_)) {
            let name = format!("#for_range_step_{}", self.next_temp_id());
            Some(name)
        } else {
            None
        };

        let target_snapshot = self.save_variable_binding(&for_loop.target);
        let stop_snapshot = self.save_variable_binding(&stop_var);
        let step_snapshot = step_var
            .as_ref()
            .map(|step_var| (step_var.clone(), self.save_variable_binding(step_var)));
        self.bind_runtime_integer_variable(&for_loop.target);
        self.bind_runtime_integer_variable(&stop_var);
        if let Some(step_var) = &step_var {
            self.bind_runtime_integer_variable(step_var);
        }

        let result = self.emit_runtime_loop(RuntimeLoopDriver {
            helper_prefix: "for_range",
            loop_helper_id_before_condition: false,
            init_commands: Box::new(|transpiler| {
                let mut init_commands =
                    transpiler.evaluate_expression_to_target(&plan.start, &for_loop.target)?;
                init_commands
                    .extend(transpiler.evaluate_expression_to_target(&plan.stop, &stop_var)?);
                if let (RuntimeRangeStep::Dynamic(step), Some(step_var)) = (&plan.step, &step_var) {
                    init_commands.extend(transpiler.evaluate_expression_to_target(step, step_var)?);
                    if plan.reverse_single_arg_on_negative_step {
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players operation {} temp = {} temp",
                            step_var, for_loop.target, stop_var
                        ));
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players remove {} temp 1",
                            step_var, for_loop.target
                        ));
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players set {} temp -1",
                            step_var, stop_var
                        ));
                    }
                }
                Ok(init_commands)
            }),
            condition: Box::new(|transpiler| {
                transpiler.runtime_range_condition(
                    &for_loop.target,
                    &stop_var,
                    &plan.step,
                    &step_var,
                )
            }),
            body: Box::new(|transpiler| {
                let mut statements = for_loop.body.clone();
                statements.push(Statement::Assignment(Assignment {
                    target: for_loop.target.clone(),
                    target_expr: None,
                    value: transpiler.runtime_range_increment_expression(
                        &for_loop.target,
                        &plan.step,
                        &step_var,
                    )?,
                }));
                Ok(RuntimeLoopBody {
                    statements,
                    binding_commands: Vec::new(),
                })
            }),
            cleanup_commands: Vec::new(),
        });

        self.restore_variable_binding(&for_loop.target, target_snapshot);
        self.restore_variable_binding(&stop_var, stop_snapshot);
        if let Some((step_var, snapshot)) = step_snapshot {
            self.restore_variable_binding(&step_var, snapshot);
        }
        result
    }

    fn process_runtime_storage_list_for(
        &mut self,
        for_loop: &ForLoop,
    ) -> Result<(), TranspileError> {
        let RuntimeStorageListPlan {
            path: list_plan,
            setup_commands,
            cleanup_commands,
        } = self.runtime_storage_list_plan(&for_loop.iter)?;
        validate_runtime_loop_body_target(&for_loop.body, &for_loop.target)?;

        if self.current_context.is_param(&for_loop.target) {
            return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                "runtime-for-unsupported: dynamic list loop target '{}' cannot shadow a function parameter.",
                for_loop.target
            )));
        }

        let step = match for_loop.step.as_ref() {
            Some(step) => self.runtime_range_step(step)?,
            None => RuntimeRangeStep::Static(1),
        };
        if matches!(step, RuntimeRangeStep::Static(0)) {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForBadStep,
                "runtime-for-bad-step: list step cannot be zero.",
            ));
        }

        let index_var = format!("#for_list_index_{}", self.next_temp_id());
        let len_var = format!("#for_list_len_{}", self.next_temp_id());
        let step_var = if matches!(step, RuntimeRangeStep::Dynamic(_)) {
            let name = format!("#for_list_step_{}", self.next_temp_id());
            Some(name)
        } else {
            None
        };

        let target_snapshot = self.save_variable_binding(&for_loop.target);
        let index_snapshot = self.save_variable_binding(&index_var);
        let len_snapshot = self.save_variable_binding(&len_var);
        let step_snapshot = step_var
            .as_ref()
            .map(|step_var| (step_var.clone(), self.save_variable_binding(step_var)));

        let element_type = self.runtime_list_element_type(&for_loop.iter);
        self.bind_runtime_storage_loop_value(&for_loop.target, element_type);
        self.bind_runtime_integer_variable(&index_var);
        self.bind_runtime_integer_variable(&len_var);
        if let Some(step_var) = &step_var {
            self.bind_runtime_integer_variable(step_var);
        }

        let result = self.emit_runtime_loop(RuntimeLoopDriver {
            helper_prefix: "for_list",
            loop_helper_id_before_condition: false,
            init_commands: Box::new(|transpiler| {
                let mut init_commands = setup_commands.clone();
                init_commands.push(format!("scoreboard players set {} temp 0", index_var));
                init_commands
                    .extend(transpiler.runtime_storage_list_len_commands(&list_plan, &len_var)?);
                match (&step, &step_var) {
                    (RuntimeRangeStep::Static(step), _) if *step < 0 => {
                        init_commands.push(format!(
                            "scoreboard players operation {} temp = {} temp",
                            index_var, len_var
                        ));
                        init_commands
                            .push(format!("scoreboard players remove {} temp 1", index_var));
                        init_commands.push(format!("scoreboard players set {} temp -1", len_var));
                    }
                    (RuntimeRangeStep::Dynamic(step_expr), Some(step_var)) => {
                        init_commands
                            .extend(transpiler.evaluate_expression_to_target(step_expr, step_var)?);
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players operation {} temp = {} temp",
                            step_var, index_var, len_var
                        ));
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players remove {} temp 1",
                            step_var, index_var
                        ));
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players set {} temp -1",
                            step_var, len_var
                        ));
                    }
                    _ => {}
                }
                Ok(init_commands)
            }),
            condition: Box::new(|transpiler| {
                transpiler.runtime_range_condition(&index_var, &len_var, &step, &step_var)
            }),
            body: Box::new(|transpiler| {
                let item_plan = list_plan
                    .clone()
                    .append_dynamic_index(Expression::Identifier(index_var.clone()));
                let binding_commands =
                    transpiler.runtime_storage_list_item_commands(&item_plan, &for_loop.target)?;

                let mut statements = for_loop.body.clone();
                statements.push(Statement::Assignment(Assignment {
                    target: index_var.clone(),
                    target_expr: None,
                    value: transpiler.runtime_range_increment_expression(
                        &index_var,
                        &step,
                        &step_var,
                    )?,
                }));
                Ok(RuntimeLoopBody {
                    statements,
                    binding_commands,
                })
            }),
            cleanup_commands,
        });

        self.restore_variable_binding(&for_loop.target, target_snapshot);
        self.restore_variable_binding(&index_var, index_snapshot);
        self.restore_variable_binding(&len_var, len_snapshot);
        if let Some((step_var, snapshot)) = step_snapshot {
            self.restore_variable_binding(&step_var, snapshot);
        }
        result
    }

    fn runtime_storage_list_candidate(&self, iter: &Expression) -> bool {
        match iter {
            Expression::Identifier(name) => {
                self.variable_storage_paths.contains_key(name)
                    || matches!(self.variable_types.get(name), Some(CobbleType::List))
            }
            Expression::Attribute(_, _) | Expression::Subscript(_, _) => {
                self.resolve_storage_path_plan(iter).is_some()
            }
            Expression::Array(_) => true,
            _ => false,
        }
    }

    fn runtime_storage_list_plan(
        &mut self,
        iter: &Expression,
    ) -> Result<RuntimeStorageListPlan, TranspileError> {
        match iter {
            Expression::Identifier(name) => {
                let value_type = self.variable_types.get(name);
                if matches!(value_type, Some(CobbleType::Map | CobbleType::String)) {
                    return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                        "runtime-for-unsupported: '{}' is not a storage-backed list.",
                        name
                    )));
                }
                self.variable_storage_paths
                    .get(name)
                    .cloned()
                    .map(|path| RuntimeStorageListPlan {
                        path: StoragePathPlan::static_path(path),
                        setup_commands: Vec::new(),
                        cleanup_commands: Vec::new(),
                    })
                    .ok_or_else(|| {
                        TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                            "runtime-for-unsupported: list '{}' is not backed by storage.",
                            name
                        ))
                    })
            }
            Expression::Attribute(_, _) | Expression::Subscript(_, _) => {
                let Some(path) = self.resolve_storage_path_plan(iter) else {
                    return Err(
                        TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, "runtime-for-unsupported: iterable does not resolve to storage."),
                    );
                };
                if matches!(self.storage_expr_type(iter), Some(CobbleType::Map | CobbleType::String))
                {
                    return Err(
                        TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, "runtime-for-unsupported: storage-backed iterable must be a list."),
                    );
                }
                Ok(RuntimeStorageListPlan {
                    path,
                    setup_commands: Vec::new(),
                    cleanup_commands: Vec::new(),
                })
            }
            Expression::Array(items) => {
                if items.is_empty() {
                    return Err(
                        TranspileError::new(TranspileErrorKind::RuntimeForBadRange, "runtime-for-bad-range: runtime literal arrays cannot be empty."),
                    );
                }
                let id = self.next_temp_id();
                let path = format!("_cobble_for_iter_{}", id);
                let setup_commands = self.storage_value_commands(&path, iter)?;
                Ok(RuntimeStorageListPlan {
                    path: StoragePathPlan::static_path(path.clone()),
                    setup_commands,
                    cleanup_commands: vec![format!(
                        "data remove storage {}:global {}",
                        self.data_pack.namespace, path
                    )],
                })
            }
            _ => Err(
                TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, "runtime-for-unsupported: dynamic for loops only support range(...), storage-backed lists, or runtime literal arrays."),
            ),
        }
    }

    fn runtime_storage_list_len_commands(
        &mut self,
        list_plan: &StoragePathPlan,
        len_var: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        if let Some(list_path) = list_plan.as_static() {
            return Ok(vec![format!(
                "execute store result score {} temp run data get storage {}:global {}",
                len_var, namespace, list_path
            )]);
        }

        self.dynamic_storage_command(list_plan, GeneratedCommandKind::ControlFlow, |path| {
            vec![
                format!("scoreboard players set {} temp 0", len_var),
                format!(
                    "$execute store result score {} temp run data get storage {}:global {}",
                    len_var, namespace, path
                ),
            ]
        })
    }

    fn runtime_storage_list_item_commands(
        &mut self,
        item_plan: &StoragePathPlan,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let target_path = format!("vars.{}", target);
        self.dynamic_storage_command(item_plan, GeneratedCommandKind::ControlFlow, |path| {
            vec![
                format!("data remove storage {}:global {}", namespace, target_path),
                format!(
                    "$data modify storage {}:global {} set from storage {}:global {}",
                    namespace, target_path, namespace, path
                ),
                format!("scoreboard players set {} temp 0", target),
                format!(
                    "$execute store result score {} temp run data get storage {}:global {}",
                    target, namespace, path
                ),
            ]
        })
    }

    fn prepend_runtime_list_item_commands(
        &self,
        capture: FunctionCapture,
        item_commands: Vec<String>,
    ) -> FunctionCapture {
        let prefix_len = item_commands.len();
        let mut commands = item_commands;
        commands.extend(capture.commands);

        let mut metadata = HashMap::new();
        for (index, command) in commands.iter().take(prefix_len).enumerate() {
            metadata.insert(
                index,
                GeneratedCommand::new(
                    command.clone(),
                    self.current_statement_source.clone(),
                    GeneratedCommandKind::ControlFlow,
                ),
            );
        }
        for (index, generated) in capture.metadata {
            metadata.insert(index + prefix_len, generated);
        }

        FunctionCapture { commands, metadata }
    }

    fn runtime_range_condition(
        &mut self,
        target: &str,
        stop_var: &str,
        step: &RuntimeRangeStep,
        step_var: &Option<String>,
    ) -> Result<(String, FunctionCapture), TranspileError> {
        match step {
            RuntimeRangeStep::Static(step) => {
                let condition_op = if *step > 0 {
                    BinaryOp::Lt
                } else {
                    BinaryOp::Gt
                };
                let condition = Expression::Binary(
                    Box::new(Expression::Identifier(target.to_string())),
                    condition_op,
                    Box::new(Expression::Identifier(stop_var.to_string())),
                );

                let (condition_cmd, condition_capture) =
                    self.capture_commands_with_result(|transpiler| {
                        let processed_condition = transpiler.preprocess_condition(&condition)?;
                        let translated = transpiler.translate_condition(&processed_condition)?;
                        transpiler.lower_translated_condition(translated)
                    })?;
                Ok((
                    Self::condition_execute_args(&condition_cmd),
                    condition_capture,
                ))
            }
            RuntimeRangeStep::Dynamic(_) => {
                let Some(step_var) = step_var else {
                    return Err(TranspileError::new(
                        TranspileErrorKind::RuntimeForInternal,
                        "runtime-for-internal: missing dynamic range step variable",
                    ));
                };
                let run_var = format!("#for_range_run_{}", self.next_temp_id());
                self.data_pack.track_objective("temp");
                let commands = vec![
                    format!("scoreboard players set {} temp 0", run_var),
                    format!(
                        "execute if score {} temp matches 1.. if score {} temp < {} temp run scoreboard players set {} temp 1",
                        step_var, target, stop_var, run_var
                    ),
                    format!(
                        "execute if score {} temp matches ..-1 if score {} temp > {} temp run scoreboard players set {} temp 1",
                        step_var, target, stop_var, run_var
                    ),
                ];
                let metadata = commands
                    .iter()
                    .enumerate()
                    .map(|(index, command)| {
                        (
                            index,
                            GeneratedCommand::new(
                                command.clone(),
                                self.current_statement_source.clone(),
                                GeneratedCommandKind::ControlFlow,
                            ),
                        )
                    })
                    .collect();
                Ok((
                    format!("if score {} temp matches 1", run_var),
                    FunctionCapture { commands, metadata },
                ))
            }
        }
    }

    fn runtime_range_increment_expression(
        &self,
        target: &str,
        step: &RuntimeRangeStep,
        step_var: &Option<String>,
    ) -> Result<Expression, TranspileError> {
        let step_expr = match step {
            RuntimeRangeStep::Static(step) => Expression::Number(f64::from(*step)),
            RuntimeRangeStep::Dynamic(_) => Expression::Identifier(
                step_var
                    .as_ref()
                    .ok_or_else(|| {
                        TranspileError::new(
                            TranspileErrorKind::RuntimeForInternal,
                            "runtime-for-internal: missing dynamic range step variable",
                        )
                    })?
                    .clone(),
            ),
        };
        Ok(Expression::Binary(
            Box::new(Expression::Identifier(target.to_string())),
            BinaryOp::Add,
            Box::new(step_expr),
        ))
    }

    fn runtime_range_plan(&self, for_loop: &ForLoop) -> Result<RuntimeRangePlan, TranspileError> {
        let Expression::Call(func, args) = &for_loop.iter else {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForUnsupported,
                "runtime-for-unsupported: dynamic for loops only support range(...).",
            ));
        };
        if !is_identifier(func, "range") {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForUnsupported,
                "runtime-for-unsupported: dynamic for loops only support range(...).",
            ));
        }
        if args.is_empty() || args.len() > 3 {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForUnsupported,
                format!(
                    "runtime-for-unsupported: range(...) expects 1 to 3 arguments, got {}.",
                    args.len()
                ),
            ));
        }

        if args.len() == 3 && for_loop.step.is_some() {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForBadStep,
                "runtime-for-bad-step: use either range(start, stop, step) or 'by step', not both.",
            ));
        }

        let step = match (args.len(), for_loop.step.as_ref()) {
            (3, _) => self.runtime_range_step(&args[2])?,
            (_, Some(step)) => self.runtime_range_step(step)?,
            _ => RuntimeRangeStep::Static(1),
        };
        if matches!(step, RuntimeRangeStep::Static(0)) {
            return Err(TranspileError::new(
                TranspileErrorKind::RuntimeForBadStep,
                "runtime-for-bad-step: range step cannot be zero.",
            ));
        }

        match args.len() {
            1 => {
                self.ensure_runtime_range_bound(&args[0], "count")?;
                let (start, stop, reverse_single_arg_on_negative_step) = match &step {
                    RuntimeRangeStep::Static(step) if *step > 0 => {
                        (Expression::Number(0.0), args[0].clone(), false)
                    }
                    RuntimeRangeStep::Static(_) => (
                        Expression::Binary(
                            Box::new(args[0].clone()),
                            BinaryOp::Sub,
                            Box::new(Expression::Number(1.0)),
                        ),
                        Expression::Number(-1.0),
                        false,
                    ),
                    RuntimeRangeStep::Dynamic(_) => {
                        (Expression::Number(0.0), args[0].clone(), true)
                    }
                };
                Ok(RuntimeRangePlan {
                    start,
                    stop,
                    step,
                    reverse_single_arg_on_negative_step,
                })
            }
            2 | 3 => {
                self.ensure_runtime_range_bound(&args[0], "start")?;
                self.ensure_runtime_range_bound(&args[1], "stop")?;
                Ok(RuntimeRangePlan {
                    start: args[0].clone(),
                    stop: args[1].clone(),
                    step,
                    reverse_single_arg_on_negative_step: false,
                })
            }
            _ => unreachable!(),
        }
    }

    fn runtime_range_step(&self, expr: &Expression) -> Result<RuntimeRangeStep, TranspileError> {
        if let Some(value) = self.try_eval_const(expr) {
            if !value.is_finite() || value.fract() != 0.0 {
                return Err(TranspileError::new(
                    TranspileErrorKind::RuntimeForBadStep,
                    "runtime-for-bad-step: range step must be an integer.",
                ));
            }
            if value < i32::MIN as f64 || value > i32::MAX as f64 {
                return Err(TranspileError::new(
                    TranspileErrorKind::RuntimeForBadRange,
                    format!(
                        "runtime-for-bad-range: range step value {value} is outside the i32 range."
                    ),
                ));
            }
            return Ok(RuntimeRangeStep::Static(value as i32));
        }

        self.ensure_runtime_range_bound(expr, "step")?;
        Ok(RuntimeRangeStep::Dynamic(expr.clone()))
    }

    fn ensure_runtime_range_bound(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<(), TranspileError> {
        match self.infer_type(expr) {
            CobbleType::String | CobbleType::List | CobbleType::Map => Err(TranspileError::new(
                TranspileErrorKind::RuntimeForUnsupported,
                format!("runtime-for-unsupported: range {label} must be an integer expression."),
            )),
            _ => Ok(()),
        }
    }

    fn save_variable_binding(&self, name: &str) -> VariableBindingSnapshot {
        VariableBindingSnapshot {
            value: self.variables.get(name).cloned(),
            objective: self.variable_objectives.get(name).cloned(),
            is_scoreboard: self.scoreboard_variables.contains(name),
            storage_path: self.variable_storage_paths.get(name).cloned(),
            compile_time_constant: self.compile_time_constants.get(name).copied(),
            value_type: self.variable_types.get(name).cloned(),
        }
    }

    fn restore_variable_binding(&mut self, name: &str, snapshot: VariableBindingSnapshot) {
        restore_optional(&mut self.variables, name, snapshot.value);
        restore_optional(&mut self.variable_objectives, name, snapshot.objective);
        if snapshot.is_scoreboard {
            self.scoreboard_variables.insert(name.to_string());
        } else {
            self.scoreboard_variables.remove(name);
        }
        restore_optional(
            &mut self.variable_storage_paths,
            name,
            snapshot.storage_path,
        );
        restore_optional(
            &mut self.compile_time_constants,
            name,
            snapshot.compile_time_constant,
        );
        restore_optional(&mut self.variable_types, name, snapshot.value_type);
    }

    fn bind_runtime_integer_variable(&mut self, name: &str) {
        self.data_pack.track_objective("temp");
        self.variables.remove(name);
        self.variable_objectives
            .insert(name.to_string(), "temp".to_string());
        self.scoreboard_variables.insert(name.to_string());
        self.variable_storage_paths.remove(name);
        self.compile_time_constants.remove(name);
        self.variable_types
            .insert(name.to_string(), CobbleType::Integer);
    }

    fn bind_runtime_storage_loop_value(&mut self, name: &str, element_type: CobbleType) {
        self.data_pack.track_objective("temp");
        self.variables.remove(name);
        self.variable_objectives
            .insert(name.to_string(), "temp".to_string());
        self.scoreboard_variables.insert(name.to_string());
        self.variable_storage_paths
            .insert(name.to_string(), format!("vars.{}", name));
        self.compile_time_constants.remove(name);
        self.variable_types.insert(name.to_string(), element_type);
    }

    /// Best-effort element type for a runtime list loop target. A literal
    /// array (inline, or a variable whose last known value is a literal array)
    /// with a uniform element type yields that type, so misusing string
    /// elements in arithmetic or conditions errors at compile time. Everything
    /// else stays `Unknown` (permissive).
    fn runtime_list_element_type(&self, iter: &Expression) -> CobbleType {
        let items = match iter {
            Expression::Array(items) => items,
            Expression::Identifier(name) => match self.variables.get(name) {
                Some(Expression::Array(items)) => items,
                _ => return CobbleType::Unknown,
            },
            _ => return CobbleType::Unknown,
        };
        if items.is_empty() {
            CobbleType::Unknown
        } else if items
            .iter()
            .all(|item| matches!(item, Expression::Number(_)))
        {
            CobbleType::Integer
        } else if items
            .iter()
            .all(|item| matches!(item, Expression::String(_)))
        {
            CobbleType::String
        } else {
            CobbleType::Unknown
        }
    }

    fn unroll_values(&self, for_loop: &ForLoop) -> Result<Vec<UnrollValue>, TranspileError> {
        match &for_loop.iter {
            Expression::Array(items) => self.unroll_array_values(items),
            Expression::Call(func, args) if is_identifier(func, "range") => {
                self.unroll_range_values(for_loop, args)
            }
            Expression::Call(func, _) => {
                let name = match &**func {
                    Expression::Identifier(name) => name.as_str(),
                    _ => "unknown",
                };
                Err(TranspileError::new(TranspileErrorKind::UnrollNonLiteral, format!(
                    "unroll-non-literal: for loops only support literal range(...) or literal arrays, not {name}(...)."
                )))
            }
            _ => Err(TranspileError::new(
                TranspileErrorKind::UnrollNonLiteral,
                "unroll-non-literal: for loops only support literal range(...) or literal arrays.",
            )),
        }
    }

    fn unroll_array_values(
        &self,
        items: &[Expression],
    ) -> Result<Vec<UnrollValue>, TranspileError> {
        if items.is_empty() {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollBadRange,
                "unroll-bad-range: literal arrays used for unrolling cannot be empty.",
            ));
        }
        if items.len() > UNROLL_LIMIT {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollLimit,
                format!(
                    "unroll-limit-exceeded: literal array expands to {} iterations (limit {}).",
                    items.len(),
                    UNROLL_LIMIT
                ),
            ));
        }

        items
            .iter()
            .map(|item| match item {
                Expression::Number(value) => Ok(UnrollValue::Number(*value)),
                Expression::String(value) => Ok(UnrollValue::String(value.clone())),
                Expression::Boolean(value) => Ok(UnrollValue::Boolean(*value)),
                _ => Err(
                    TranspileError::new(TranspileErrorKind::UnrollNonLiteral, "unroll-non-literal: literal arrays may only contain numbers, strings, or booleans."),
                ),
            })
            .collect()
    }

    fn unroll_range_values(
        &self,
        for_loop: &ForLoop,
        args: &[Expression],
    ) -> Result<Vec<UnrollValue>, TranspileError> {
        if args.is_empty() || args.len() > 3 {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollNonLiteral,
                format!(
                    "unroll-non-literal: range(...) expects 1 to 3 arguments, got {}.",
                    args.len()
                ),
            ));
        }

        let by_step = for_loop
            .step
            .as_ref()
            .map(|step| self.integer_unroll_expr(step, "range step"))
            .transpose()?;
        if args.len() == 3 && by_step.is_some() {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollBadStep,
                "unroll-bad-step: use either range(start, stop, step) or 'by step', not both.",
            ));
        }

        let (start, stop, step) = match args.len() {
            1 => {
                let count = self.integer_unroll_expr(&args[0], "range count")?;
                if count < 0 {
                    return Err(TranspileError::new(
                        TranspileErrorKind::UnrollBadRange,
                        format!("unroll-bad-range: range({count}) cannot expand a negative count."),
                    ));
                }
                let step = by_step.unwrap_or(1);
                if step == 0 {
                    return Err(TranspileError::new(
                        TranspileErrorKind::UnrollBadStep,
                        "unroll-bad-step: range step cannot be zero.",
                    ));
                }
                if step > 0 {
                    (0, count, step)
                } else {
                    (count - 1, -1, step)
                }
            }
            2 => {
                let start = self.integer_unroll_expr(&args[0], "range start")?;
                let stop = self.integer_unroll_expr(&args[1], "range stop")?;
                let step = by_step.unwrap_or(1);
                if step == 0 {
                    return Err(TranspileError::new(
                        TranspileErrorKind::UnrollBadStep,
                        "unroll-bad-step: range step cannot be zero.",
                    ));
                }
                (start, stop, step)
            }
            3 => {
                let start = self.integer_unroll_expr(&args[0], "range start")?;
                let stop = self.integer_unroll_expr(&args[1], "range stop")?;
                let step = self.integer_unroll_expr(&args[2], "range step")?;
                if step == 0 {
                    return Err(TranspileError::new(
                        TranspileErrorKind::UnrollBadStep,
                        "unroll-bad-step: range step cannot be zero.",
                    ));
                }
                (start, stop, step)
            }
            _ => unreachable!(),
        };

        if args.len() != 1 {
            if step > 0 && start > stop {
                return Err(TranspileError::new(TranspileErrorKind::UnrollBadRange, format!(
                    "unroll-bad-range: range start {start} is greater than stop {stop} with a positive step."
                )));
            }
            if step < 0 && start < stop {
                return Err(TranspileError::new(TranspileErrorKind::UnrollBadRange, format!(
                    "unroll-bad-range: range start {start} is less than stop {stop} with a negative step."
                )));
            }
        }

        range_unroll_values(start, stop, step)
    }

    fn integer_unroll_expr(&self, expr: &Expression, label: &str) -> Result<i32, TranspileError> {
        let value = self.try_eval_const(expr).ok_or_else(|| {
            TranspileError::new(
                TranspileErrorKind::UnrollNonLiteral,
                format!(
                    "unroll-non-literal: {label} must be an integer literal or const identifier."
                ),
            )
        })?;
        if !value.is_finite() || value.fract() != 0.0 {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollBadStep,
                format!("unroll-bad-step: {label} must be an integer."),
            ));
        }
        if value < i32::MIN as f64 || value > i32::MAX as f64 {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollBadRange,
                format!("unroll-bad-range: {label} value {value} is outside the i32 range."),
            ));
        }
        Ok(value as i32)
    }

    fn mark_current_function_commands_as_unrolled(&mut self, start_index: usize) {
        let Some(ref commands) = self.current_function else {
            return;
        };
        let Some(ref mut metadata) = self.current_function_metadata else {
            return;
        };
        let source = self.current_statement_source.clone();
        for (index, command) in commands.iter().enumerate().skip(start_index) {
            let command = command.clone();
            metadata
                .entry(index)
                .and_modify(|generated| {
                    generated.text = command.clone();
                    if generated.source.is_none() {
                        generated.source = source.clone();
                    }
                    generated.kind = GeneratedCommandKind::Unrolled;
                })
                .or_insert_with(|| {
                    GeneratedCommand::new(command, source.clone(), GeneratedCommandKind::Unrolled)
                });
        }
    }

    fn take_unrolled_body_statement_source(
        &mut self,
        statement: &Statement,
    ) -> Option<SourceLocation> {
        match statement {
            Statement::MinecraftCommand(command) => {
                let clean_command = Self::strip_command_prefix(command);
                self.take_raw_command_source(clean_command.trim())
                    .or_else(|| self.take_raw_command_statement_source(clean_command.trim()))
            }
            _ => self.take_statement_source(statement),
        }
    }

    fn generated_command_count(&self) -> usize {
        self.data_pack
            .functions
            .values()
            .map(Vec::len)
            .sum::<usize>()
            + self.current_function.as_ref().map_or(0, Vec::len)
    }

    fn ensure_unroll_command_budget(&self) -> Result<(), TranspileError> {
        let Some(baseline) = self.unroll_command_baseline else {
            return Ok(());
        };
        let generated = self.generated_command_count().saturating_sub(baseline);
        if generated > UNROLL_GENERATED_COMMAND_LIMIT {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollCommandLimit,
                format!(
                    "unroll-limit-exceeded: unrolling generated {} commands (limit {}).",
                    generated, UNROLL_GENERATED_COMMAND_LIMIT
                ),
            ));
        }
        Ok(())
    }

    pub(in crate::transpiler) fn process_while(
        &mut self,
        while_loop: &WhileLoop,
    ) -> Result<(), TranspileError> {
        // WARNING: While loops execute all iterations in a single tick
        // This can cause server lag with large iteration counts (>100)
        // Future improvement: Add schedule command support for tick-based iteration

        // Check for obvious infinite loops
        if let Expression::Boolean(true) = &while_loop.condition {
            eprintln!(
                "⚠️  Warning: Infinite loop detected (while True). \n\
                    This will run forever and freeze Minecraft!\n\
                    Consider using a condition that can become false."
            );
        }

        self.emit_runtime_loop(RuntimeLoopDriver {
            helper_prefix: "while",
            loop_helper_id_before_condition: true,
            init_commands: Box::new(|_| Ok(Vec::new())),
            // Capture condition setup so complex expressions and OR lowering
            // are rerun on every recursive iteration instead of once before
            // the loop.
            condition: Box::new(|transpiler| {
                let (condition_cmd, condition_capture) =
                    transpiler.capture_commands_with_result(|transpiler| {
                        let processed_condition =
                            transpiler.preprocess_condition(&while_loop.condition)?;
                        let translated = transpiler.translate_condition(&processed_condition)?;
                        transpiler.lower_translated_condition(translated)
                    })?;
                Ok((
                    Transpiler::condition_execute_args(&condition_cmd),
                    condition_capture,
                ))
            }),
            body: Box::new(|_| {
                Ok(RuntimeLoopBody {
                    statements: while_loop.body.clone(),
                    binding_commands: Vec::new(),
                })
            }),
            cleanup_commands: Vec::new(),
        })
    }

    /// Shared skeleton behind the recursive runtime loops (`for range(...)`,
    /// storage-list `for`, `while`): emit the init commands, capture the
    /// condition, wrap the body in a helper function so the condition is
    /// evaluated once per iteration (not per statement), then emit the
    /// recursive loop helper that re-checks the condition after the body and
    /// propagates early returns.
    fn emit_runtime_loop(&mut self, driver: RuntimeLoopDriver<'_>) -> Result<(), TranspileError> {
        let RuntimeLoopDriver {
            helper_prefix,
            loop_helper_id_before_condition,
            init_commands,
            condition,
            body,
            cleanup_commands,
        } = driver;

        let init_commands = init_commands(self)?;
        if let Some(ref mut commands) = self.current_function {
            commands.extend(init_commands);
        }

        let mut loop_func_name = if loop_helper_id_before_condition {
            Some(format!("{}_temp_{}", helper_prefix, self.next_temp_id()))
        } else {
            None
        };
        let (condition_execute_args, condition_capture) = condition(self)?;
        let loop_func_name = loop_func_name
            .take()
            .unwrap_or_else(|| format!("{}_temp_{}", helper_prefix, self.next_temp_id()));
        let body_func_name = format!("{}_body_{}", helper_prefix, self.next_temp_id());

        let RuntimeLoopBody {
            statements,
            binding_commands,
        } = body(self)?;

        let saved_context = self.current_context.clone();
        let mut capture = self.capture_statements(&statements)?;
        let body_needs_storage = capture.requires_macro_context();
        if !binding_commands.is_empty() {
            capture = self.prepend_runtime_list_item_commands(capture, binding_commands);
        }
        self.add_captured_function(body_func_name.clone(), capture);
        self.current_context = saved_context;

        let body_call = self.function_call_command(&body_func_name, body_needs_storage);
        let mut loop_commands = Vec::new();
        let mut loop_metadata = HashMap::new();
        Self::append_capture_to_buffers(&mut loop_commands, &mut loop_metadata, &condition_capture);
        loop_commands.push(format!(
            "execute {} run {}",
            condition_execute_args, body_call
        ));
        let body_call_index = loop_commands.len() - 1;
        loop_metadata.insert(
            body_call_index,
            GeneratedCommand::new(
                loop_commands[body_call_index].clone(),
                self.current_statement_source.clone(),
                GeneratedCommandKind::ControlFlow,
            ),
        );
        if let Some(return_cmd) = self.return_propagation_command() {
            loop_commands.push(return_cmd);
        }

        // Re-evaluate the condition after the body before deciding whether to
        // recurse. This keeps loops with mutated or computed conditions correct.
        Self::append_capture_to_buffers(&mut loop_commands, &mut loop_metadata, &condition_capture);
        loop_commands.push(format!(
            "execute {} run function {}:{}",
            condition_execute_args, self.data_pack.namespace, loop_func_name
        ));
        let recurse_index = loop_commands.len() - 1;
        loop_metadata.insert(
            recurse_index,
            GeneratedCommand::new(
                loop_commands[recurse_index].clone(),
                self.current_statement_source.clone(),
                GeneratedCommandKind::ControlFlow,
            ),
        );

        self.data_pack.add_function_with_metadata(
            loop_func_name.clone(),
            loop_commands,
            loop_metadata,
        );

        // Start the loop.
        let return_cmd = self.return_propagation_command();
        if let Some(ref mut commands) = self.current_function {
            commands.push(format!(
                "function {}:{}",
                self.data_pack.namespace, loop_func_name
            ));
            commands.extend(cleanup_commands);
            if let Some(return_cmd) = return_cmd {
                commands.push(return_cmd);
            }
        }

        Ok(())
    }

    fn append_capture_to_buffers(
        commands: &mut Vec<String>,
        metadata: &mut HashMap<usize, GeneratedCommand>,
        capture: &crate::transpiler::FunctionCapture,
    ) {
        let start = commands.len();
        commands.extend(capture.commands.clone());
        for (source_index, generated) in &capture.metadata {
            metadata.insert(start + source_index, generated.clone());
        }
    }
}

/// Appends the `loop:`/`source-key:`/`location:` trailer lines to an
/// unroll-family error, leaving other kinds untouched. Trailers already
/// present (from an inner loop) are kept as-is.
fn annotate_unroll_error(
    error: TranspileError,
    target: &str,
    source_key: &str,
    location: Option<&SourceLocation>,
) -> TranspileError {
    if error.kind.is_unroll() {
        annotate_loop_error(error, target, source_key, location)
    } else {
        error
    }
}

/// Appends the `loop:`/`source-key:`/`location:` trailer lines to a
/// runtime-for-family error, leaving other kinds untouched.
fn annotate_runtime_for_error(
    error: TranspileError,
    target: &str,
    source_key: &str,
    location: Option<&SourceLocation>,
) -> TranspileError {
    if error.kind.is_runtime_for() {
        annotate_loop_error(error, target, source_key, location)
    } else {
        error
    }
}

fn annotate_loop_error(
    mut error: TranspileError,
    target: &str,
    source_key: &str,
    location: Option<&SourceLocation>,
) -> TranspileError {
    if !error.message.contains("\n  loop: for ") {
        error.message.push_str(&format!("\n  loop: for {target}"));
    }
    if !error.message.contains("\n  source-key: ") {
        error
            .message
            .push_str(&format!("\n  source-key: {source_key}"));
    }
    if let Some(location) = location.filter(|_| !error.message.contains("\n  location: ")) {
        error.message.push_str(&format!(
            "\n  location: {}:{}",
            location.line, location.column
        ));
    }
    error
}

fn restore_optional<T>(map: &mut HashMap<String, T>, name: &str, value: Option<T>) {
    if let Some(value) = value {
        map.insert(name.to_string(), value);
    } else {
        map.remove(name);
    }
}

fn validate_runtime_loop_body_target(
    statements: &[Statement],
    target: &str,
) -> Result<(), TranspileError> {
    for statement in statements {
        validate_runtime_loop_statement_target(statement, target)?;
    }
    Ok(())
}

fn validate_runtime_loop_statement_target(
    statement: &Statement,
    target: &str,
) -> Result<(), TranspileError> {
    match statement {
        Statement::Assignment(assign) if assign.target == target => Err(TranspileError::new(
            TranspileErrorKind::RuntimeForUnsupported,
            format!(
                "runtime-for-unsupported: cannot assign to dynamic loop target '{}'.",
                target
            ),
        )),
        Statement::ConstAssignment(assign) if assign.target == target => Err(TranspileError::new(
            TranspileErrorKind::RuntimeForUnsupported,
            format!(
                "runtime-for-unsupported: cannot assign to dynamic loop target '{}'.",
                target
            ),
        )),
        Statement::FunctionDef(function) => {
            if function.params.iter().any(|param| param.name == target) {
                return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                    "runtime-for-unsupported: nested function parameter '{}' shadows the dynamic loop target.",
                    target
                )));
            }
            validate_runtime_loop_body_target(&function.body, target)
        }
        Statement::If(if_stmt) => {
            validate_runtime_loop_body_target(&if_stmt.then_branch, target)?;
            for (_, branch) in &if_stmt.elif_branches {
                validate_runtime_loop_body_target(branch, target)?;
            }
            if let Some(branch) = &if_stmt.else_branch {
                validate_runtime_loop_body_target(branch, target)?;
            }
            Ok(())
        }
        Statement::For(for_loop) => {
            if for_loop.target == target {
                return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                    "runtime-for-unsupported: nested loop target '{}' shadows the dynamic loop target.",
                    target
                )));
            }
            validate_runtime_loop_body_target(&for_loop.body, target)
        }
        Statement::While(while_loop) => validate_runtime_loop_body_target(&while_loop.body, target),
        Statement::Match(match_stmt) => {
            for case in &match_stmt.cases {
                validate_runtime_loop_body_target(&case.body, target)?;
            }
            Ok(())
        }
        Statement::Execute(exec_block) => {
            validate_runtime_loop_body_target(&exec_block.body, target)
        }
        _ => Ok(()),
    }
}

fn is_identifier(expr: &Expression, expected: &str) -> bool {
    matches!(expr, Expression::Identifier(name) if name == expected)
}

fn range_unroll_values(
    start: i32,
    stop: i32,
    step: i32,
) -> Result<Vec<UnrollValue>, TranspileError> {
    let mut values = Vec::new();
    let mut current = i64::from(start);
    let stop = i64::from(stop);
    let step = i64::from(step);

    while (step > 0 && current < stop) || (step < 0 && current > stop) {
        if values.len() == UNROLL_LIMIT {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollLimit,
                format!(
                    "unroll-limit-exceeded: range expands past {} iterations.",
                    UNROLL_LIMIT
                ),
            ));
        }
        if current < i64::from(i32::MIN) || current > i64::from(i32::MAX) {
            return Err(TranspileError::new(
                TranspileErrorKind::UnrollBadRange,
                format!("unroll-bad-range: range value {current} is outside the i32 range."),
            ));
        }
        values.push(UnrollValue::Number(current as f64));
        current += step;
    }

    Ok(values)
}

fn format_unroll_number(value: f64) -> String {
    if value == 0.0 {
        "0".to_string()
    } else if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

fn substitute_statements(
    statements: &[Statement],
    target: &str,
    value: &UnrollValue,
) -> Result<Vec<Statement>, TranspileError> {
    statements
        .iter()
        .map(|statement| substitute_statement(statement, target, value))
        .collect()
}

fn substitute_statement(
    statement: &Statement,
    target: &str,
    value: &UnrollValue,
) -> Result<Statement, TranspileError> {
    let substituted = match statement {
        Statement::Import(import) => Statement::Import(import.clone()),
        Statement::FunctionDef(function) => {
            if function.params.iter().any(|param| param.name == target) {
                return Err(TranspileError::new(TranspileErrorKind::UnrollNonLiteral, format!(
                    "unroll-non-literal: nested function parameter '{}' shadows the unrolled loop target.",
                    target
                )));
            }
            Statement::FunctionDef(FunctionDef {
                name: function.name.clone(),
                params: function.params.clone(),
                decorators: function.decorators.clone(),
                body: substitute_statements(&function.body, target, value)?,
            })
        }
        Statement::Assignment(assign) => {
            if assign.target == target {
                return Err(TranspileError::new(
                    TranspileErrorKind::UnrollNonLiteral,
                    format!(
                        "unroll-non-literal: cannot assign to unrolled loop target '{}'.",
                        target
                    ),
                ));
            }
            Statement::Assignment(Assignment {
                target: assign.target.clone(),
                target_expr: assign
                    .target_expr
                    .as_ref()
                    .map(|target_expr| substitute_expression(target_expr, target, value))
                    .transpose()?,
                value: substitute_expression(&assign.value, target, value)?,
            })
        }
        Statement::ConstAssignment(assign) => {
            if assign.target == target {
                return Err(TranspileError::new(
                    TranspileErrorKind::UnrollNonLiteral,
                    format!(
                        "unroll-non-literal: cannot assign to unrolled loop target '{}'.",
                        target
                    ),
                ));
            }
            Statement::ConstAssignment(ConstAssignment {
                target: assign.target.clone(),
                value: substitute_expression(&assign.value, target, value)?,
            })
        }
        Statement::Expression(expr) => {
            Statement::Expression(substitute_expression(expr, target, value)?)
        }
        Statement::If(if_stmt) => Statement::If(IfStatement {
            condition: substitute_expression(&if_stmt.condition, target, value)?,
            then_branch: substitute_statements(&if_stmt.then_branch, target, value)?,
            elif_branches: if_stmt
                .elif_branches
                .iter()
                .map(|(condition, body)| {
                    Ok((
                        substitute_expression(condition, target, value)?,
                        substitute_statements(body, target, value)?,
                    ))
                })
                .collect::<Result<Vec<_>, TranspileError>>()?,
            else_branch: if_stmt
                .else_branch
                .as_ref()
                .map(|body| substitute_statements(body, target, value))
                .transpose()?,
        }),
        Statement::For(for_loop) => {
            if for_loop.target == target {
                return Err(TranspileError::new(
                    TranspileErrorKind::UnrollNonLiteral,
                    format!(
                    "unroll-non-literal: nested loop target '{}' shadows the unrolled loop target.",
                    target
                ),
                ));
            }
            Statement::For(ForLoop {
                target: for_loop.target.clone(),
                iter: substitute_expression(&for_loop.iter, target, value)?,
                step: for_loop
                    .step
                    .as_ref()
                    .map(|step| substitute_expression(step, target, value))
                    .transpose()?,
                body: substitute_statements(&for_loop.body, target, value)?,
            })
        }
        Statement::While(while_loop) => Statement::While(WhileLoop {
            condition: substitute_expression(&while_loop.condition, target, value)?,
            body: substitute_statements(&while_loop.body, target, value)?,
        }),
        Statement::Match(match_stmt) => Statement::Match(MatchStatement {
            value: substitute_expression(&match_stmt.value, target, value)?,
            cases: match_stmt
                .cases
                .iter()
                .map(|case| {
                    Ok(MatchCase {
                        pattern: case.pattern.clone(),
                        body: substitute_statements(&case.body, target, value)?,
                    })
                })
                .collect::<Result<Vec<_>, TranspileError>>()?,
        }),
        Statement::Return(expr) => Statement::Return(
            expr.as_ref()
                .map(|expr| substitute_expression(expr, target, value))
                .transpose()?,
        ),
        Statement::Pass => Statement::Pass,
        Statement::MinecraftCommand(command) => Statement::MinecraftCommand(
            replace_unroll_placeholder(command, target, &value.as_command_text()),
        ),
        Statement::Global(vars) => Statement::Global(vars.clone()),
        Statement::Execute(exec_block) => Statement::Execute(ExecuteBlock {
            modifiers: exec_block
                .modifiers
                .iter()
                .map(|modifier| substitute_execute_modifier(modifier, target, value))
                .collect::<Result<Vec<_>, TranspileError>>()?,
            body: substitute_statements(&exec_block.body, target, value)?,
        }),
        Statement::SelectorDef(selector_def) => Statement::SelectorDef(SelectorDef {
            name: selector_def.name.clone(),
            selector: replace_unroll_placeholder(
                &selector_def.selector,
                target,
                &value.as_command_text(),
            ),
        }),
        Statement::EntityDef(entity_def) => Statement::EntityDef(EntityDef {
            name: entity_def.name.clone(),
            selector: replace_unroll_placeholder(
                &entity_def.selector,
                target,
                &value.as_command_text(),
            ),
            nbt: substitute_expression(&entity_def.nbt, target, value)?,
        }),
        Statement::CreateEntity(name) => Statement::CreateEntity(name.clone()),
    };
    Ok(substituted)
}

fn substitute_execute_modifier(
    modifier: &ExecuteModifier,
    target: &str,
    value: &UnrollValue,
) -> Result<ExecuteModifier, TranspileError> {
    let command_value = value.as_command_text();
    let substituted = match modifier {
        ExecuteModifier::As(raw) => {
            ExecuteModifier::As(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::At(raw) => {
            ExecuteModifier::At(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::IfRaw(raw) => {
            ExecuteModifier::IfRaw(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::UnlessRaw(raw) => {
            ExecuteModifier::UnlessRaw(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Positioned(raw) => {
            ExecuteModifier::Positioned(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Rotated(raw) => {
            ExecuteModifier::Rotated(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::In(raw) => {
            ExecuteModifier::In(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Anchored(raw) => {
            ExecuteModifier::Anchored(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Align(raw) => {
            ExecuteModifier::Align(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Store(raw) => {
            ExecuteModifier::Store(replace_unroll_placeholder(raw, target, &command_value))
        }
    };
    Ok(substituted)
}

fn substitute_expression(
    expression: &Expression,
    target: &str,
    value: &UnrollValue,
) -> Result<Expression, TranspileError> {
    let substituted = match expression {
        Expression::Identifier(name) if name == target => value.as_expression(),
        Expression::Identifier(_) => expression.clone(),
        Expression::String(text) => Expression::String(replace_unroll_placeholder(
            text,
            target,
            &value.as_command_text(),
        )),
        Expression::Array(items) => Expression::Array(
            items
                .iter()
                .map(|item| substitute_expression(item, target, value))
                .collect::<Result<Vec<_>, TranspileError>>()?,
        ),
        Expression::Map(entries) => Expression::Map(
            entries
                .iter()
                .map(|(key, entry_value)| {
                    Ok((
                        replace_unroll_placeholder(key, target, &value.as_command_text()),
                        substitute_expression(entry_value, target, value)?,
                    ))
                })
                .collect::<Result<Vec<_>, TranspileError>>()?,
        ),
        Expression::Attribute(base, attr) => Expression::Attribute(
            Box::new(substitute_expression(base, target, value)?),
            attr.clone(),
        ),
        Expression::Binary(left, op, right) => Expression::Binary(
            Box::new(substitute_expression(left, target, value)?),
            op.clone(),
            Box::new(substitute_expression(right, target, value)?),
        ),
        Expression::Unary(op, expr) => Expression::Unary(
            op.clone(),
            Box::new(substitute_expression(expr, target, value)?),
        ),
        Expression::Call(func, args) => Expression::Call(
            Box::new(substitute_expression(func, target, value)?),
            args.iter()
                .map(|arg| substitute_expression(arg, target, value))
                .collect::<Result<Vec<_>, TranspileError>>()?,
        ),
        Expression::Subscript(base, index) => Expression::Subscript(
            Box::new(substitute_expression(base, target, value)?),
            Box::new(substitute_expression(index, target, value)?),
        ),
        Expression::Number(_) | Expression::Boolean(_) | Expression::None => expression.clone(),
    };
    Ok(substituted)
}

fn replace_unroll_placeholder(text: &str, target: &str, replacement: &str) -> String {
    let mut output = String::new();
    let mut index = 0;

    while index < text.len() {
        let remaining = &text[index..];
        if let Some(stripped) = remaining.strip_prefix("{{") {
            if let Some(end) = stripped.find("}}") {
                let end_index = index + 2 + end + 2;
                output.push_str(&text[index..end_index]);
                index = end_index;
            } else {
                output.push_str(&text[index..]);
                break;
            }
        } else if remaining.starts_with('{') {
            if let Some(end_index) = find_matching_unroll_brace(text, index) {
                let placeholder = text[index + 1..end_index].trim();
                if placeholder == target {
                    output.push_str(replacement);
                } else {
                    output.push('{');
                    output.push_str(&replace_unroll_placeholder(
                        &text[index + 1..end_index],
                        target,
                        replacement,
                    ));
                    output.push('}');
                }
                index = end_index + 1;
            } else {
                output.push_str(&text[index..]);
                break;
            }
        } else {
            let ch = remaining.chars().next().expect("non-empty string slice");
            output.push(ch);
            index += ch.len_utf8();
        }
    }

    output
}

fn find_matching_unroll_brace(text: &str, open_index: usize) -> Option<usize> {
    let mut depth = 0usize;
    let mut index = open_index;

    while index < text.len() {
        let remaining = &text[index..];
        if let Some(stripped) = remaining.strip_prefix("{{") {
            if let Some(end) = stripped.find("}}") {
                index += 2 + end + 2;
                continue;
            }
            return None;
        }

        let ch = remaining.chars().next()?;
        match ch {
            '{' => depth += 1,
            '}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
        index += ch.len_utf8();
    }

    None
}
