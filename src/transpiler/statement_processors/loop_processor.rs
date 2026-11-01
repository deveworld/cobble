use crate::ast::*;
use crate::transpiler::function_context;
use crate::transpiler::placeholders::{self, Piece};
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{
    FunctionCapture, FunctionReturnKind, GeneratedCommand, GeneratedCommandKind, SourceLocation,
    StoragePathPlan, TranspileError, TranspileErrorKind, Transpiler,
};
use std::collections::HashMap;

/// State of the compile-time `for` unrolling in progress.
pub(in crate::transpiler) struct UnrollState {
    /// Current nesting depth.
    pub(in crate::transpiler) depth: usize,
    /// Product of literal iteration counts across the active unroll stack.
    pub(in crate::transpiler) iteration_factor: usize,
    /// Generated command count at the start of the outermost unroll.
    pub(in crate::transpiler) command_baseline: Option<usize>,
}

impl Default for UnrollState {
    fn default() -> Self {
        Self {
            depth: 0,
            iteration_factor: 1,
            command_baseline: None,
        }
    }
}

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
    /// `while` allocates the loop helper id before lowering its condition;
    /// the runtime `for` drivers allocate it afterwards. Generated helper
    /// names depend on this order.
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

/// A runtime `for` over a list whose body is being lowered and may rebind
/// the list. As in Python, the loop keeps iterating the list it started
/// with: a statement that rebinds the list, or moves it, first saves the
/// list's root variable for the loop (see [`Transpiler::rebind_signal_commands`]).
#[derive(Debug, Clone)]
pub(in crate::transpiler) struct ActiveListLoop {
    /// The storage path of the iterable's root variable.
    root_path: String,
    /// The iterable, with the indexes the loop fixed at its start.
    iter: Expression,
}

/// How a runtime `for` keeps walking the list it started with when its
/// body may rebind the list. The loop walks `copy`, which takes the list at
/// `list_plan` before each pass, so changes in place are seen. A statement
/// that rebinds the list first saves the root variable in its rebind slot
/// and sets its rebind holder to 2; the next pass then takes the old list
/// from `prev_plan`, inside that slot, and stops following the variable.
#[derive(Debug, Clone)]
struct ListLoopWatch {
    root_path: String,
    list_plan: StoragePathPlan,
    prev_plan: StoragePathPlan,
    copy: String,
    /// Where the loop keeps an outer loop's saved list while it runs.
    saved_slot: String,
    /// 1 while the copy follows the variable.
    live: String,
    /// The rebind holder's value when the loop started.
    saved: String,
}

/// `commands`, each run only under the `execute` subcommands `condition`.
fn guard_commands(commands: &[String], condition: &str) -> Vec<String> {
    commands
        .iter()
        .map(|command| match command.strip_prefix('$') {
            Some(rest) => format!("$execute {condition} run {rest}"),
            None => format!("execute {condition} run {command}"),
        })
        .collect()
}

/// Methods that move the items of a list, so an item list inside it is no
/// longer at the same index.
const MOVING_METHODS: [&str; 6] = ["pop", "insert", "remove", "clear", "sort", "reverse"];

/// The score that tells a loop over the list at `root_path` that a
/// statement rebound it: 1 while a loop watches it, 2 once a rebind saved
/// the old value in [`rebind_slot`].
fn rebind_holder(root_path: &str) -> String {
    format!("#cobble_rebind.{root_path}")
}

fn rebind_slot(root_path: &str) -> String {
    format!("_cobble_rebind.{root_path}")
}

/// The variable at the root of an iterable such as `g[i].rows`.
fn iterable_root(expr: &Expression) -> Option<&str> {
    match expr {
        Expression::Identifier(name) => Some(name),
        Expression::Attribute(base, _) | Expression::Subscript(base, _) => iterable_root(base),
        _ => None,
    }
}

/// Whether `prefix` names `expr` or a container `expr` lies in.
fn is_path_prefix(prefix: &Expression, expr: &Expression) -> bool {
    prefix == expr
        || match expr {
            Expression::Attribute(base, _) | Expression::Subscript(base, _) => {
                is_path_prefix(prefix, base)
            }
            _ => false,
        }
}

/// Whether `prefix` names a container `expr` lies in, not `expr` itself.
fn is_strict_path_prefix(prefix: &Expression, expr: &Expression) -> bool {
    prefix != expr && is_path_prefix(prefix, expr)
}

/// Calls of moving methods in `expr`, as `(base, method)`.
fn moving_calls<'a>(expr: &'a Expression, out: &mut Vec<&'a Expression>) {
    match expr {
        Expression::Call(func, args) => {
            if let Expression::Attribute(base, method) = &**func {
                if MOVING_METHODS.contains(&method.as_str()) {
                    out.push(base);
                }
                moving_calls(base, out);
            } else {
                moving_calls(func, out);
            }
            for arg in args {
                moving_calls(arg, out);
            }
        }
        Expression::Binary(left, _, right) => {
            moving_calls(left, out);
            moving_calls(right, out);
        }
        Expression::Unary(_, inner) | Expression::Attribute(inner, _) => moving_calls(inner, out),
        Expression::Subscript(base, index) => {
            moving_calls(base, out);
            moving_calls(index, out);
        }
        Expression::Slice(base, start, stop) => {
            moving_calls(base, out);
            for bound in [start, stop].into_iter().flatten() {
                moving_calls(bound, out);
            }
        }
        Expression::Conditional(condition, value, otherwise) => {
            moving_calls(condition, out);
            moving_calls(value, out);
            moving_calls(otherwise, out);
        }
        Expression::Array(items) => items.iter().for_each(|item| moving_calls(item, out)),
        Expression::Map(entries) => entries
            .iter()
            .for_each(|(_, value)| moving_calls(value, out)),
        _ => {}
    }
}

/// The expressions `statement` evaluates itself, without its body.
fn own_expressions(statement: &Statement) -> Vec<&Expression> {
    match statement {
        Statement::Assignment(assign) => {
            let mut out = vec![&assign.value];
            out.extend(assign.target_expr.as_ref());
            out
        }
        Statement::MultiAssign(multi) => multi.targets.iter().chain(&multi.values).collect(),
        Statement::Expression(expr) | Statement::Return(Some(expr)) => vec![expr],
        Statement::If(if_stmt) => std::iter::once(&if_stmt.condition)
            .chain(if_stmt.elif_branches.iter().map(|(condition, _)| condition))
            .collect(),
        Statement::For(for_loop) => std::iter::once(&for_loop.iter)
            .chain(for_loop.step.as_ref())
            .collect(),
        Statement::While(while_loop) => vec![&while_loop.condition],
        Statement::Match(match_stmt) => vec![&match_stmt.value],
        _ => Vec::new(),
    }
}

/// Whether `body` calls a function the pack defines, which may rebind a
/// module list. Builtins and methods of variables don't count; a function
/// of a module, a name `modules` accepts, does.
pub(in crate::transpiler) fn calls_user_function(
    body: &[Statement],
    modules: &dyn Fn(&str) -> bool,
) -> bool {
    let mut statements = Vec::new();
    statements_in_order(body, &mut statements);
    statements.into_iter().any(|statement| {
        own_expressions(statement)
            .into_iter()
            .any(|expr| expression_calls_user_function(expr, modules))
    })
}

fn expression_calls_user_function(expr: &Expression, modules: &dyn Fn(&str) -> bool) -> bool {
    const BUILTINS: [&str; 12] = [
        "abs",
        "enumerate",
        "max",
        "min",
        "sorted",
        "str",
        "sum",
        "len",
        "range",
        "int",
        "list",
        "bool",
    ];
    let calls = |expr: &Expression| expression_calls_user_function(expr, modules);
    match expr {
        Expression::Call(func, args) => {
            let direct = match &**func {
                Expression::Identifier(name) => !BUILTINS.contains(&name.as_str()),
                // `module.f()`; a method such as `a.append(1)` is not one.
                Expression::Attribute(base, _) => match &**base {
                    Expression::Identifier(name) => modules(name),
                    base => calls(base),
                },
                func => calls(func),
            };
            direct || args.iter().any(calls)
        }
        Expression::Binary(left, _, right) => calls(left) || calls(right),
        Expression::Unary(_, inner) | Expression::Attribute(inner, _) => calls(inner),
        Expression::Subscript(base, index) => calls(base) || calls(index),
        Expression::Slice(base, start, stop) => {
            calls(base)
                || [start, stop]
                    .into_iter()
                    .flatten()
                    .any(|bound| calls(bound))
        }
        Expression::Conditional(condition, value, otherwise) => {
            calls(condition) || calls(value) || calls(otherwise)
        }
        Expression::Array(items) => items.iter().any(calls),
        Expression::Map(entries) => entries.iter().any(|(_, value)| calls(value)),
        _ => false,
    }
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

    /// The value placed in command text, where placeholders are read
    /// afterwards: its braces are escaped, so the text is shown as it is,
    /// as a Python format string shows a value.
    fn as_placed_text(&self) -> String {
        placeholders::escape_braces(&self.as_command_text())
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
        let previous_iteration_factor = self.unroll.iteration_factor;
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
            self.warn(
                "unroll-large-expansion",
                format!(
                    "`for {}` expands to {} copies of its body.",
                    for_loop.target,
                    values.len()
                ),
                Some("A `range` whose bounds are known only when the pack runs compiles to a runtime loop instead."),
            );
        }

        let is_outermost_unroll = self.unroll.depth == 0;
        if is_outermost_unroll {
            self.unroll.command_baseline = Some(self.generated_command_count());
        }
        self.unroll.depth += 1;
        self.unroll.iteration_factor = projected_iterations;

        // As in Python, the target is a variable: it holds the last value
        // after the loop, a `global` target writes the module variable, and
        // the body may assign it. When any of that can be seen, the target
        // is a real variable set at the start of each pass. Otherwise each
        // pass reads the value itself.
        let target = &for_loop.target;
        let assigned_in_body = function_context::assigns_name(&for_loop.body, target);
        let read_outside_loop = self.loop_target_read_outside(for_loop);
        let real_target = if self.current_function.is_some() && !self.in_module_code {
            assigned_in_body || read_outside_loop || self.symbols.global_variables.contains(target)
        } else {
            self.module_loop_variables.contains(target)
        };

        let result = (|| {
            self.data_pack.unrolled_loops += 1;
            // The location of every statement of the body, nested ones
            // included, in source order. A body inside an outer expanded
            // loop is a copy whose locations the outer loop recorded.
            let mut original = Vec::new();
            statements_in_order(&for_loop.body, &mut original);
            let nested_sources = original
                .iter()
                .map(|statement| {
                    match self
                        .unrolled_sources
                        .get(&function_context::statement_address(statement))
                    {
                        Some(source) => source.clone(),
                        None => self.take_unrolled_body_statement_source(statement),
                    }
                })
                .collect::<Vec<_>>();
            let body_sources = for_loop
                .body
                .iter()
                .map(|statement| {
                    original
                        .iter()
                        .position(|other| std::ptr::eq(*other, statement))
                        .and_then(|index| nested_sources[index].clone())
                })
                .collect::<Vec<_>>();
            for value in values {
                if real_target {
                    let start_index = self.current_function.as_ref().map(Vec::len);
                    let process_result =
                        self.process_statement(&Statement::Assignment(Assignment {
                            target: target.clone(),
                            target_expr: None,
                            value: value.as_expression(),
                            persist: false,
                        }));
                    if let Some(start_index) = start_index {
                        self.mark_current_function_commands_as_unrolled(start_index);
                    }
                    process_result?;
                }
                let substituted = if assigned_in_body {
                    for_loop.body.clone()
                } else {
                    substitute_statements(&for_loop.body, &for_loop.target, &value)?
                };
                // Each nested statement of this pass reports at its own
                // line, not at the line of the statement around it.
                let mut copies = Vec::new();
                statements_in_order(&substituted, &mut copies);
                let recorded: Vec<usize> = if copies.len() == nested_sources.len() {
                    copies
                        .iter()
                        .zip(&nested_sources)
                        .map(|(copy, source)| {
                            let address = function_context::statement_address(copy);
                            self.unrolled_sources.insert(address, source.clone());
                            address
                        })
                        .collect()
                } else {
                    Vec::new()
                };
                drop(copies);
                let pass = (|| {
                    for (statement, body_source) in substituted.iter().zip(body_sources.iter()) {
                        let saved_statement_source = self.current_statement_source.clone();
                        if body_source.is_some() {
                            self.current_statement_source = body_source.clone();
                        }
                        let start_index = self.current_function.as_ref().map(Vec::len);
                        let process_result = self.process_statement(statement);
                        if let Some(start_index) = start_index {
                            self.mark_current_function_commands_as_unrolled(start_index);
                        }
                        self.current_statement_source = saved_statement_source;
                        process_result?;
                        self.ensure_unroll_command_budget()?;
                    }
                    Ok::<(), TranspileError>(())
                })();
                for address in recorded {
                    self.unrolled_sources.remove(&address);
                }
                pass?;
            }
            Ok(())
        })();

        self.unroll.depth -= 1;
        self.unroll.iteration_factor = previous_iteration_factor;
        if is_outermost_unroll {
            self.unroll.command_baseline = None;
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

    /// Whether the function reads the target of `for_loop` outside the
    /// loop, as after it.
    fn loop_target_read_outside(&self, for_loop: &ForLoop) -> bool {
        let reads_in_loop = function_context::count_reads(
            std::slice::from_ref(&Statement::For(for_loop.clone())),
            &for_loop.target,
        );
        self.current_context
            .for_target_reads
            .get(&for_loop.target)
            .is_some_and(|reads| *reads > reads_in_loop)
    }

    /// Whether a runtime loop over `target` must leave its last value in
    /// it: the function reads `target` outside every loop over it.
    fn runtime_loop_target_read_after(&self, target: &str) -> bool {
        if self.in_module_code {
            // A module-level loop variable a function or later module code
            // reads is a module variable.
            return self.module_loop_variables.contains(target);
        }
        self.current_function.is_some()
            && self.current_context.for_targets_read_after.contains(target)
    }

    /// After a runtime loop whose target is read outside it, the target
    /// keeps the loop's binding, so it holds the last value as in Python.
    /// If it held a value of another type before the loop, which one it
    /// holds after the loop depends on whether the loop ran: a compile
    /// error names the rewrite.
    fn keep_runtime_loop_target(
        &mut self,
        target: &str,
        before: VariableBindingSnapshot,
        mirrored: bool,
    ) -> Result<(), TranspileError> {
        let had_binding = before.is_scoreboard
            || before.storage_path.is_some()
            || before.value.is_some()
            || before.compile_time_constant.is_some();
        let loop_type = self.symbols.variable_types.get(target).cloned();
        let known = |value_type: &Option<CobbleType>| {
            value_type
                .as_ref()
                .is_some_and(|value_type| *value_type != CobbleType::Unknown)
        };
        let number = |value_type: &Option<CobbleType>| {
            matches!(value_type, Some(CobbleType::Integer | CobbleType::Boolean))
        };
        if !had_binding
            || before.value_type == loop_type
            || (number(&before.value_type) && number(&loop_type))
        {
            if number(&before.value_type) && number(&loop_type) {
                restore_optional(
                    &mut self.symbols.variable_storage_paths,
                    target,
                    before.storage_path,
                );
            }
            return Ok(());
        }
        // A value whose type only the running pack knows on one side: the
        // variable holds either value in storage and as a score, mirrored
        // before the loop (see `mirror_loop_target_before`).
        if mirrored && (!known(&before.value_type) || !known(&loop_type)) {
            self.symbols
                .variable_types
                .insert(target.to_string(), CobbleType::Unknown);
            return Ok(());
        }
        let kind = |value_type: &Option<CobbleType>| match value_type {
            Some(CobbleType::Integer | CobbleType::Boolean) => "a number",
            Some(CobbleType::String) => "text",
            Some(CobbleType::List) => "a list",
            Some(CobbleType::Map) => "a map",
            _ => "a value of unknown type",
        };
        Err(TranspileError::new(
            TranspileErrorKind::RuntimeForUnsupported,
            format!(
                "`{target}` holds {} before this loop and {} in it, so after the loop its type depends on whether the loop ran. Use another name for the loop variable, or assign `{target}` before the loop a value of the same type.",
                kind(&before.value_type),
                kind(&loop_type)
            ),
        ))
    }

    /// Commands that copy the value `target` holds before a runtime list
    /// loop into the loop's binding, in storage and as a score, so after a
    /// loop that runs no passes the variable still holds it.
    fn mirror_loop_target_before(
        &mut self,
        target: &str,
        before: &VariableBindingSnapshot,
    ) -> Result<Vec<String>, TranspileError> {
        let had_binding = before.is_scoreboard
            || before.storage_path.is_some()
            || before.value.is_some()
            || before.compile_time_constant.is_some();
        if !had_binding {
            return Ok(Vec::new());
        }
        let ns = self.data_pack.namespace.clone();
        let loop_path = self.variable_storage_path(target);
        let number = matches!(
            before.value_type,
            Some(CobbleType::Integer | CobbleType::Boolean)
        ) || (before.storage_path.is_none() && before.value_type.is_none());
        let value = Expression::Identifier(target.to_string());
        let mut commands = Vec::new();
        if number {
            if !before.is_scoreboard {
                commands.extend(self.evaluate_expression_to_target(&value, target)?);
            }
            commands.push(format!(
                "execute store result storage {ns}:global {loop_path} int 1 run scoreboard players get {target} temp"
            ));
            return Ok(commands);
        }
        if before.storage_path.as_deref() != Some(loop_path.as_str()) {
            commands.extend(self.storage_value_commands(&loop_path, &value)?);
        }
        commands.push(format!(
            "execute store result score {target} temp run data get storage {ns}:global {loop_path}"
        ));
        Ok(commands)
    }

    /// The last value of a module-level loop expanded at compile time, for
    /// recording its variable before function bodies are lowered.
    pub(in crate::transpiler) fn expanded_loop_last_value(
        &self,
        for_loop: &ForLoop,
    ) -> Option<Expression> {
        let values = self.unroll_values(for_loop).ok()?;
        values.last().map(UnrollValue::as_expression)
    }

    /// Whether `for_loop` is expanded at compile time.
    pub(in crate::transpiler) fn expands_at_compile_time(&self, for_loop: &ForLoop) -> bool {
        self.unroll_values(for_loop).is_ok()
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
        // The loop counts in the target itself, which ends one step past
        // the last value. When the target is read after the loop, it counts
        // in a hidden score instead and the target is set from it at the
        // start of each pass, so it keeps the last value, as in Python. So
        // it does when the body assigns the target, as Python sets it again
        // at the start of the next pass.
        let keep_target = self.runtime_loop_target_read_after(&for_loop.target);
        let hidden_counter =
            keep_target || function_context::assigns_name(&for_loop.body, &for_loop.target);
        let counter = if hidden_counter {
            format!("#for_range_value_{}", self.next_temp_id())
        } else {
            for_loop.target.clone()
        };
        let counter_snapshot =
            hidden_counter.then(|| (counter.clone(), self.save_variable_binding(&counter)));

        let target_snapshot = self.save_variable_binding(&for_loop.target);
        let stop_snapshot = self.save_variable_binding(&stop_var);
        let step_snapshot = step_var
            .as_ref()
            .map(|step_var| (step_var.clone(), self.save_variable_binding(step_var)));
        self.bind_runtime_integer_variable(&for_loop.target);
        self.bind_runtime_integer_variable(&counter);
        self.bind_runtime_integer_variable(&stop_var);
        if let Some(step_var) = &step_var {
            self.bind_runtime_integer_variable(step_var);
        }

        let result = self.emit_runtime_loop(RuntimeLoopDriver {
            helper_prefix: "for_range",
            loop_helper_id_before_condition: false,
            init_commands: Box::new(|transpiler| {
                let mut init_commands =
                    transpiler.evaluate_expression_to_target(&plan.start, &counter)?;
                init_commands
                    .extend(transpiler.evaluate_expression_to_target(&plan.stop, &stop_var)?);
                if let (RuntimeRangeStep::Dynamic(step), Some(step_var)) = (&plan.step, &step_var) {
                    init_commands.extend(transpiler.evaluate_expression_to_target(step, step_var)?);
                    if plan.reverse_single_arg_on_negative_step {
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players operation {} temp = {} temp",
                            step_var, counter, stop_var
                        ));
                        init_commands.push(format!(
                            "execute if score {} temp matches ..-1 run scoreboard players remove {} temp 1",
                            step_var, counter
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
                transpiler.runtime_range_condition(&counter, &stop_var, &plan.step, &step_var)
            }),
            body: Box::new(|transpiler| {
                let mut statements = for_loop.body.clone();
                statements.push(Statement::Assignment(Assignment {
                    target: counter.clone(),
                    target_expr: None,
                    persist: false,
                    value: transpiler.runtime_range_increment_expression(
                        &counter,
                        &plan.step,
                        &step_var,
                    )?,
                }));
                let binding_commands = if hidden_counter {
                    vec![format!(
                        "scoreboard players operation {} temp = {} temp",
                        for_loop.target, counter
                    )]
                } else {
                    Vec::new()
                };
                Ok(RuntimeLoopBody {
                    statements,
                    binding_commands,
                })
            }),
            cleanup_commands: Vec::new(),
        });

        let kept = if keep_target && result.is_ok() {
            self.keep_runtime_loop_target(&for_loop.target, target_snapshot, false)
        } else {
            self.restore_variable_binding(&for_loop.target, target_snapshot);
            Ok(())
        };
        if let Some((counter, snapshot)) = counter_snapshot {
            self.restore_variable_binding(&counter, snapshot);
        }
        self.restore_variable_binding(&stop_var, stop_snapshot);
        if let Some((step_var, snapshot)) = step_snapshot {
            self.restore_variable_binding(&step_var, snapshot);
        }
        result.and(kept)
    }

    fn process_runtime_storage_list_for(
        &mut self,
        for_loop: &ForLoop,
    ) -> Result<(), TranspileError> {
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

        // As in Python, the path to the list is worked out once.
        let iter = self.fix_loop_iterable(for_loop)?;
        let RuntimeStorageListPlan {
            path: list_plan,
            setup_commands,
            cleanup_commands,
        } = self.runtime_storage_list_plan(&iter)?;
        let watch = self.list_loop_watch(for_loop, &iter, &list_plan);
        let mut cleanup_commands = cleanup_commands;
        // A loop that may see its list rebound walks a copy, kept equal to
        // the list until a rebind (see `ListLoopWatch`).
        let walked_plan = match &watch {
            Some(watch) => StoragePathPlan::static_path(watch.copy.clone()),
            None => list_plan.clone(),
        };

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

        let element_type = self.runtime_list_element_type(&iter);
        let keep_target = self.runtime_loop_target_read_after(&for_loop.target);
        let before_loop =
            if keep_target && target_snapshot.value_type.as_ref() != Some(&element_type) {
                self.mirror_loop_target_before(&for_loop.target, &target_snapshot)?
            } else {
                Vec::new()
            };
        self.bind_runtime_storage_loop_value(&for_loop.target, element_type);
        self.bind_runtime_integer_variable(&index_var);
        self.bind_runtime_integer_variable(&len_var);
        if let Some(step_var) = &step_var {
            self.bind_runtime_integer_variable(step_var);
        }

        let (watch_start, watch_sync) = match &watch {
            Some(watch) => {
                let (start, end) = self.list_loop_watch_bounds(watch);
                cleanup_commands.extend(end);
                (start, self.list_loop_watch_sync(watch)?)
            }
            None => (Vec::new(), Vec::new()),
        };
        if let Some(watch) = &watch {
            self.list_loops.push(ActiveListLoop {
                root_path: watch.root_path.clone(),
                iter: iter.clone(),
            });
        }

        let result = self.emit_runtime_loop(RuntimeLoopDriver {
            helper_prefix: "for_list",
            loop_helper_id_before_condition: false,
            init_commands: Box::new(|transpiler| {
                let mut init_commands = before_loop.clone();
                init_commands.extend(setup_commands.clone());
                init_commands.extend(watch_start.clone());
                init_commands.extend(watch_sync.clone());
                init_commands.push(format!("scoreboard players set {} temp 0", index_var));
                init_commands
                    .extend(transpiler.runtime_storage_list_len_commands(&walked_plan, &len_var)?);
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
                let (args, capture) =
                    transpiler.runtime_range_condition(&index_var, &len_var, &step, &step_var)?;
                // Walking forward, each pass reads the length again, as
                // Python does, so the body can grow or shrink the list.
                let mut before = watch_sync.clone();
                match (&step, &step_var) {
                    (RuntimeRangeStep::Static(step), _) if *step > 0 => {
                        before.extend(
                            transpiler.runtime_storage_list_len_commands(&walked_plan, &len_var)?,
                        );
                    }
                    (RuntimeRangeStep::Dynamic(_), Some(step_var)) => {
                        let length =
                            transpiler.runtime_storage_list_len_commands(&walked_plan, &len_var)?;
                        before.extend(guard_commands(
                            &length,
                            &format!("if score {step_var} temp matches 1.."),
                        ));
                    }
                    _ => {}
                }
                Ok((args, transpiler.prepend_control_commands(capture, before)))
            }),
            body: Box::new(|transpiler| {
                let unknown_base = transpiler
                    .unknown_iterable_path(&iter)
                    .or_else(|| transpiler.unknown_item_iterable_path(&iter, &list_plan));
                let binding_commands = match unknown_base {
                    // A parameter can hold a string, whose items are its
                    // characters, so the item is read as `p[i]` reads it.
                    Some(base_path) => transpiler.runtime_unknown_item_commands(
                        walked_plan.as_static().unwrap_or(&base_path),
                        &index_var,
                        &for_loop.target,
                    )?,
                    None => {
                        let item_plan = walked_plan
                            .clone()
                            .append_dynamic_index(Expression::Identifier(index_var.clone()));
                        transpiler.runtime_storage_list_item_commands(&item_plan, &for_loop.target)?
                    }
                };

                let mut statements = for_loop.body.clone();
                statements.push(Statement::Assignment(Assignment {
                    target: index_var.clone(),
                    target_expr: None,
                    persist: false,
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
        if watch.is_some() {
            self.list_loops.pop();
        }

        let kept = if keep_target && result.is_ok() {
            self.keep_runtime_loop_target(&for_loop.target, target_snapshot, true)
        } else {
            self.restore_variable_binding(&for_loop.target, target_snapshot);
            Ok(())
        };
        self.restore_variable_binding(&index_var, index_snapshot);
        self.restore_variable_binding(&len_var, len_snapshot);
        if let Some((step_var, snapshot)) = step_snapshot {
            self.restore_variable_binding(&step_var, snapshot);
        }
        result.and(kept)
    }

    /// `capture` with `commands` run first.
    fn prepend_control_commands(
        &self,
        mut capture: FunctionCapture,
        commands: Vec<String>,
    ) -> FunctionCapture {
        if commands.is_empty() {
            return capture;
        }
        let source = self.current_statement_source.clone();
        let shift = commands.len();
        capture.metadata = capture
            .metadata
            .into_iter()
            .map(|(index, command)| (index + shift, command))
            .chain(commands.iter().enumerate().map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(
                        command.clone(),
                        source.clone(),
                        GeneratedCommandKind::ControlFlow,
                    ),
                )
            }))
            .collect();
        capture.commands.splice(0..0, commands);
        capture
    }

    /// The iterable of `for_loop` with each index the body may change
    /// replaced by a hidden variable set here, so the loop keeps the list
    /// it started with, as in Python, where `for v in g[i]:` with `i = 1`
    /// in the body still walks `g[0]`.
    fn fix_loop_iterable(&mut self, for_loop: &ForLoop) -> Result<Expression, TranspileError> {
        fn fix(
            transpiler: &mut Transpiler,
            expr: &Expression,
            body: &[Statement],
        ) -> Result<Expression, TranspileError> {
            match expr {
                Expression::Attribute(base, name) => Ok(Expression::Attribute(
                    Box::new(fix(transpiler, base, body)?),
                    name.clone(),
                )),
                Expression::Subscript(base, index) => {
                    let base = fix(transpiler, base, body)?;
                    let mut reads = Vec::new();
                    function_context::expression_reads(index, &mut reads);
                    let changes = reads
                        .iter()
                        .any(|name| function_context::assigns_name(body, name));
                    if !changes {
                        return Ok(Expression::Subscript(Box::new(base), index.clone()));
                    }
                    let value_type = transpiler.value_type(index);
                    let hidden = transpiler.hidden_local(value_type);
                    transpiler.process_statement(&Statement::Assignment(Assignment {
                        target: hidden.clone(),
                        target_expr: None,
                        value: (**index).clone(),
                        persist: false,
                    }))?;
                    Ok(Expression::Subscript(
                        Box::new(base),
                        Box::new(Expression::Identifier(hidden)),
                    ))
                }
                other => Ok(other.clone()),
            }
        }
        fix(self, &for_loop.iter, &for_loop.body)
    }

    /// How a runtime loop over `iter` follows a rebind of its list, when
    /// its body may rebind the list or move it: `None` when it can't.
    fn list_loop_watch(
        &mut self,
        for_loop: &ForLoop,
        iter: &Expression,
        list_plan: &StoragePathPlan,
    ) -> Option<ListLoopWatch> {
        let root = iterable_root(iter)?;
        let root_path = self.root_storage_path(root)?;
        let (template, args) = match list_plan {
            StoragePathPlan::Static(path) => (path.clone(), None),
            StoragePathPlan::Dynamic { template, args } => (template.clone(), Some(args.clone())),
        };
        let rest = template.strip_prefix(&root_path)?;
        if !(rest.is_empty() || rest.starts_with('.') || rest.starts_with('[')) {
            return None;
        }
        if !self.loop_body_may_rebind(for_loop, iter, root, &root_path) {
            return None;
        }
        let prev_template = format!("{}{rest}", rebind_slot(&root_path));
        let prev_plan = match args {
            None => StoragePathPlan::static_path(prev_template),
            Some(args) => StoragePathPlan::Dynamic {
                template: prev_template,
                args,
            },
        };
        let id = self.next_temp_id();
        Some(ListLoopWatch {
            root_path,
            list_plan: list_plan.clone(),
            prev_plan,
            copy: format!("_cobble_for_iter_{id}"),
            saved_slot: format!("_cobble_for_prev_{id}"),
            live: format!("#for_list_live_{id}"),
            saved: format!("#for_list_watch_{id}"),
        })
    }

    /// The storage path of the variable `name`, a parameter included.
    fn root_storage_path(&self, name: &str) -> Option<String> {
        self.symbols
            .variable_storage_paths
            .get(name)
            .cloned()
            .or_else(|| {
                self.current_context
                    .is_param(name)
                    .then(|| format!("args.{name}"))
            })
    }

    /// Whether the body of `for_loop` may rebind the list `iter` names, or
    /// move it: an assignment to its root or to a container it lies in, a
    /// moving method on such a container, or, for a module list, a call of
    /// a function that may assign it.
    fn loop_body_may_rebind(
        &self,
        for_loop: &ForLoop,
        iter: &Expression,
        root: &str,
        root_path: &str,
    ) -> bool {
        let mut statements = Vec::new();
        statements_in_order(&for_loop.body, &mut statements);
        let rebinds =
            |target: &Expression| is_identifier(target, root) || is_path_prefix(target, iter);
        for statement in statements {
            let direct = match statement {
                Statement::Assignment(assign) => {
                    (assign.assigns_name() && assign.target == root)
                        || assign.target_expr.as_ref().is_some_and(rebinds)
                }
                Statement::MultiAssign(multi) => multi.targets.iter().any(rebinds),
                Statement::For(inner) => inner.target == root,
                _ => false,
            };
            if direct {
                return true;
            }
            let mut moved = Vec::new();
            for expr in own_expressions(statement) {
                moving_calls(expr, &mut moved);
            }
            if moved
                .into_iter()
                .any(|base| is_strict_path_prefix(base, iter))
            {
                return true;
            }
        }
        root_path.starts_with("vars.")
            && self.signalled_list_roots.contains(root)
            && calls_user_function(&for_loop.body, &|name| self.is_module_name(name))
    }

    /// Whether `name` names an imported module rather than a variable.
    fn is_module_name(&self, name: &str) -> bool {
        !self.symbols.variable_storage_paths.contains_key(name)
            && !self.symbols.scoreboard_variables.contains(name)
            && !self.current_context.is_param(name)
    }

    /// The commands that start and end watching for a rebind. A loop
    /// inside another loop over the same list keeps the outer loop's
    /// saved value.
    fn list_loop_watch_bounds(&mut self, watch: &ListLoopWatch) -> (Vec<String>, Vec<String>) {
        self.data_pack.track_objective("temp");
        let ns = self.data_pack.namespace.clone();
        let holder = rebind_holder(&watch.root_path);
        let slot = rebind_slot(&watch.root_path);
        let ListLoopWatch {
            copy,
            saved_slot,
            live,
            saved,
            ..
        } = watch;
        let start = vec![
            format!("execute store result score {saved} temp run scoreboard players get {holder} temp"),
            format!("execute if score {saved} temp matches 2 run data modify storage {ns}:global {saved_slot} set from storage {ns}:global {slot}"),
            format!("scoreboard players set {holder} temp 1"),
            format!("scoreboard players set {live} temp 1"),
            format!("data remove storage {ns}:global {copy}"),
        ];
        let end = vec![
            format!("execute if score {saved} temp matches 0 run data remove storage {ns}:global {slot}"),
            format!("execute if score {saved} temp matches 0 run scoreboard players set {holder} temp 0"),
            format!("execute if score {saved} temp matches 2 run scoreboard players set {holder} temp 2"),
            format!("execute if score {saved} temp matches 2 run data modify storage {ns}:global {slot} set from storage {ns}:global {saved_slot}"),
            format!("data remove storage {ns}:global {copy}"),
            format!("data remove storage {ns}:global {saved_slot}"),
        ];
        (start, end)
    }

    /// The commands run before each pass of a watched loop: after a rebind,
    /// the copy takes the list as it was then, and the loop stops following
    /// the variable; until then the copy takes the list as it is now.
    fn list_loop_watch_sync(
        &mut self,
        watch: &ListLoopWatch,
    ) -> Result<Vec<String>, TranspileError> {
        let holder = rebind_holder(&watch.root_path);
        let live = &watch.live;
        let rebound = format!("if score {live} temp matches 1 if score {holder} temp matches 2");
        let mut commands = guard_commands(
            &self.copy_storage_commands(&watch.copy, &watch.prev_plan)?,
            &rebound,
        );
        commands.push(format!(
            "execute {rebound} run scoreboard players set {live} temp 0"
        ));
        commands.extend(guard_commands(
            &self.copy_storage_commands(&watch.copy, &watch.list_plan)?,
            &format!("if score {live} temp matches 1"),
        ));
        Ok(commands)
    }

    /// Before `statement` rebinds a list a running loop walks, or moves it,
    /// saves the list's root variable for the loop (see `ListLoopWatch`).
    pub(in crate::transpiler) fn rebind_signal_commands(
        &mut self,
        statement: &Statement,
    ) -> Result<(), TranspileError> {
        if self.current_function.is_none()
            || (self.list_loops.is_empty() && self.signalled_list_roots.is_empty())
        {
            return Ok(());
        }
        let mut paths: Vec<String> = Vec::new();
        if let Statement::Assignment(assign) = statement {
            if assign.assigns_name() {
                if let Some(path) = self.root_storage_path(&assign.target) {
                    let watched = self.list_loops.iter().any(|watch| watch.root_path == path)
                        || (path.starts_with("vars.")
                            && self.signalled_list_roots.contains(&assign.target));
                    if watched && !self.extends_in_place(assign) {
                        paths.push(path);
                    }
                }
            } else if let Some(target) = &assign.target_expr {
                let root = iterable_root(target).and_then(|root| self.root_storage_path(root));
                for watch in &self.list_loops {
                    if root.as_deref() == Some(watch.root_path.as_str())
                        && is_path_prefix(target, &watch.iter)
                    {
                        paths.push(watch.root_path.clone());
                    }
                }
            }
        }
        let mut moved = Vec::new();
        for expr in own_expressions(statement) {
            moving_calls(expr, &mut moved);
        }
        for base in moved {
            let root = iterable_root(base).and_then(|root| self.root_storage_path(root));
            for watch in &self.list_loops {
                if root.as_deref() == Some(watch.root_path.as_str())
                    && is_strict_path_prefix(base, &watch.iter)
                {
                    paths.push(watch.root_path.clone());
                }
            }
        }
        paths.sort();
        paths.dedup();
        if paths.is_empty() {
            return Ok(());
        }
        self.data_pack.track_objective("temp");
        let ns = self.data_pack.namespace.clone();
        let mut commands = Vec::new();
        for path in paths {
            let holder = rebind_holder(&path);
            let slot = rebind_slot(&path);
            commands.push(format!("execute if score {holder} temp matches 1 run data modify storage {ns}:global {slot} set from storage {ns}:global {path}"));
            commands.push(format!(
                "execute if score {holder} temp matches 1 run scoreboard players set {holder} temp 2"
            ));
        }
        self.push_current_commands(commands)
    }

    /// Whether `assign` comes from `a += [...]` or `a *= n` on a list, which
    /// changes the list in place, as in Python, rather than rebinding it.
    /// The parser reads both as `a = a + ...`, so the source line tells.
    fn extends_in_place(&self, assign: &Assignment) -> bool {
        if self.symbols.variable_types.get(&assign.target) == Some(&CobbleType::String) {
            return false;
        }
        let Some(location) = &self.current_statement_source else {
            return false;
        };
        let Some(line) = self
            .source_locations
            .sources
            .get(&location.file)
            .and_then(|source| source.lines().nth(location.line.saturating_sub(1)))
        else {
            return false;
        };
        let rest: String = line
            .chars()
            .skip(location.column.saturating_sub(1))
            .collect();
        rest.trim_start()
            .strip_prefix(assign.target.as_str())
            .map(str::trim_start)
            .is_some_and(|rest| rest.starts_with("+=") || rest.starts_with("*="))
    }

    fn runtime_storage_list_candidate(&self, iter: &Expression) -> bool {
        match iter {
            // A parameter is iterated as a list.
            Expression::Identifier(name) => {
                self.symbols.variable_storage_paths.contains_key(name)
                    || self.current_context.is_param(name)
                    || matches!(
                        self.symbols.variable_types.get(name),
                        Some(CobbleType::List)
                    )
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
                let value_type = self.symbols.variable_types.get(name);
                if matches!(value_type, Some(CobbleType::Map | CobbleType::String)) {
                    return Err(TranspileError::new(TranspileErrorKind::RuntimeForUnsupported, format!(
                        "runtime-for-unsupported: '{}' is not a storage-backed list.",
                        name
                    )));
                }
                self.symbols.variable_storage_paths
                    .get(name)
                    .cloned()
                    .or_else(|| self.current_context.is_param(name).then(|| format!("args.{name}")))
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
                // A list or text at a path known only when the pack runs,
                // as `rows[i].s`, is read from a copy, which tells them
                // apart (see `unknown_item_iterable_path`).
                if path.as_static().is_none()
                    && self.storage_read_type(iter) == CobbleType::Unknown
                {
                    let copy = format!("_cobble_for_iter_{}", self.next_temp_id());
                    let namespace = self.data_pack.namespace.clone();
                    let mut setup_commands =
                        vec![format!("data remove storage {namespace}:global {copy}")];
                    setup_commands.extend(self.copy_storage_commands(&copy, &path)?);
                    return Ok(RuntimeStorageListPlan {
                        path: StoragePathPlan::static_path(copy.clone()),
                        setup_commands,
                        cleanup_commands: vec![format!(
                            "data remove storage {namespace}:global {copy}"
                        )],
                    });
                }
                Ok(RuntimeStorageListPlan {
                    path,
                    setup_commands: Vec::new(),
                    cleanup_commands: Vec::new(),
                })
            }
            Expression::Array(_) => {
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

    /// The storage path of a loop's iterable when it is a parameter, or a
    /// copy of one, which can hold a list or a string.
    fn unknown_iterable_path(&self, iter: &Expression) -> Option<String> {
        let Expression::Identifier(name) = iter else {
            return None;
        };
        if self.current_context.is_param(name) {
            return Some(format!("args.{name}"));
        }
        match self.symbols.variable_types.get(name) {
            None | Some(CobbleType::Unknown) => {
                self.symbols.variable_storage_paths.get(name).cloned()
            }
            _ => None,
        }
    }

    /// The storage path of a loop's iterable when it is an item or a field
    /// whose type only the running pack knows, which can hold a list or a
    /// string, as `cache.name` of `cache = {}`.
    fn unknown_item_iterable_path(
        &self,
        iter: &Expression,
        list_plan: &StoragePathPlan,
    ) -> Option<String> {
        if !matches!(iter, Expression::Attribute(..) | Expression::Subscript(..))
            || self.storage_read_type(iter) != CobbleType::Unknown
        {
            return None;
        }
        list_plan.as_static().map(str::to_string)
    }

    /// Copies item `index` of the list or string at `base_path` into the loop
    /// variable `target`, in storage and as a score.
    fn runtime_unknown_item_commands(
        &mut self,
        base_path: &str,
        index_var: &str,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let target_path = self.variable_storage_path(target);
        let mut commands = vec![format!(
            "data remove storage {}:global {}",
            namespace, target_path
        )];
        commands.extend(self.unknown_item_commands(
            &target_path,
            base_path,
            &Expression::Identifier(index_var.to_string()),
        )?);
        commands.push(format!("scoreboard players set {} temp 0", target));
        commands.push(format!(
            "execute store result score {} temp run data get storage {}:global {}",
            target, namespace, target_path
        ));
        Ok(commands)
    }

    fn runtime_storage_list_item_commands(
        &mut self,
        item_plan: &StoragePathPlan,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let target_path = self.variable_storage_path(target);
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
            value: self.symbols.variables.get(name).cloned(),
            objective: self.symbols.variable_objectives.get(name).cloned(),
            is_scoreboard: self.symbols.scoreboard_variables.contains(name),
            storage_path: self.symbols.variable_storage_paths.get(name).cloned(),
            compile_time_constant: self.symbols.compile_time_constants.get(name).copied(),
            value_type: self.symbols.variable_types.get(name).cloned(),
        }
    }

    fn restore_variable_binding(&mut self, name: &str, snapshot: VariableBindingSnapshot) {
        restore_optional(&mut self.symbols.variables, name, snapshot.value);
        restore_optional(
            &mut self.symbols.variable_objectives,
            name,
            snapshot.objective,
        );
        if snapshot.is_scoreboard {
            self.symbols.scoreboard_variables.insert(name.to_string());
        } else {
            self.symbols.scoreboard_variables.remove(name);
        }
        restore_optional(
            &mut self.symbols.variable_storage_paths,
            name,
            snapshot.storage_path,
        );
        restore_optional(
            &mut self.symbols.compile_time_constants,
            name,
            snapshot.compile_time_constant,
        );
        restore_optional(&mut self.symbols.variable_types, name, snapshot.value_type);
    }

    fn bind_runtime_integer_variable(&mut self, name: &str) {
        self.data_pack.track_objective("temp");
        self.symbols.variables.remove(name);
        self.symbols
            .variable_objectives
            .insert(name.to_string(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(name.to_string());
        self.symbols.variable_storage_paths.remove(name);
        self.symbols.compile_time_constants.remove(name);
        self.symbols
            .variable_types
            .insert(name.to_string(), CobbleType::Integer);
    }

    fn bind_runtime_storage_loop_value(&mut self, name: &str, element_type: CobbleType) {
        self.data_pack.track_objective("temp");
        self.symbols.variables.remove(name);
        self.symbols
            .variable_objectives
            .insert(name.to_string(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(name.to_string());
        if matches!(element_type, CobbleType::Integer | CobbleType::Boolean) {
            self.symbols.variable_storage_paths.remove(name);
        } else {
            self.symbols
                .variable_storage_paths
                .insert(name.to_string(), self.variable_storage_path(name));
        }
        self.symbols.compile_time_constants.remove(name);
        self.symbols
            .variable_types
            .insert(name.to_string(), element_type);
    }

    /// Best-effort element type for a runtime list loop target. A literal
    /// array (inline, or a variable whose last known value is a literal array)
    /// with a uniform element type yields that type, so misusing string
    /// elements in arithmetic or conditions errors at compile time. Everything
    /// else stays `Unknown` (permissive).
    fn runtime_list_element_type(&self, iter: &Expression) -> CobbleType {
        let items = match iter {
            Expression::Array(items) => items,
            Expression::Identifier(name) => match self.symbols.variables.get(name) {
                Some(Expression::Array(items)) => items,
                _ => return CobbleType::Unknown,
            },
            _ => return CobbleType::Unknown,
        };
        let is_number = |item: &Expression| match item {
            Expression::Number(_) => true,
            Expression::Unary(UnaryOp::Neg, inner) => {
                matches!(**inner, Expression::Number(_))
            }
            _ => false,
        };
        if items.is_empty() {
            CobbleType::Unknown
        } else if items.iter().all(is_number) {
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
        // An empty array runs no passes, like an empty range.
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

        // A range that is empty, such as `range(3, 1)`, runs no passes, as in
        // Python. It often comes from an outer loop: `range(i, 2)` inside
        // `for i in range(4)`.
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
        let Some(baseline) = self.unroll.command_baseline else {
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
        // All passes of a loop run in the same tick.
        if matches!(while_loop.condition, Expression::Boolean(true))
            && matches!(
                Self::analyze_function_return_kind(&while_loop.body),
                FunctionReturnKind::None
            )
        {
            self.warn(
                "infinite-loop",
                "`while True` has no `return` in its body, so it runs until the game stops it at the command limit, which Cobble raises to 1,000,000,000 commands.",
                Some("Use a condition that becomes false, or `return` from the loop."),
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
                        transpiler.lower_condition_to_execute_args(&while_loop.condition)
                    })?;
                Ok((condition_cmd, condition_capture))
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
    /// storage-list `for`, `while`): emit the init commands, then a loop
    /// helper that evaluates the condition once and runs the body helper.
    /// The body helper ends by calling the loop helper again, so each
    /// iteration evaluates the condition once, and a condition that calls a
    /// function calls it once per iteration. Early returns propagate after
    /// the body call.
    fn emit_runtime_loop(&mut self, driver: RuntimeLoopDriver<'_>) -> Result<(), TranspileError> {
        let RuntimeLoopDriver {
            helper_prefix,
            loop_helper_id_before_condition,
            init_commands,
            condition,
            body,
            cleanup_commands,
        } = driver;

        self.data_pack.track_runtime_loop();
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
        let next_iteration = format!("function {}:{}", self.data_pack.namespace, loop_func_name);
        capture.metadata.insert(
            capture.commands.len(),
            GeneratedCommand::new(
                next_iteration.clone(),
                self.current_statement_source.clone(),
                GeneratedCommandKind::ControlFlow,
            ),
        );
        capture.commands.push(next_iteration);
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

fn restore_optional<T: Clone>(
    map: &mut crate::transpiler::scoped::ScopedMap<String, T>,
    name: &str,
    value: Option<T>,
) {
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

/// The statements of `body` and of the blocks inside it, in source order.
/// Function definitions are not entered.
fn statements_in_order<'a>(body: &'a [Statement], out: &mut Vec<&'a Statement>) {
    for statement in body {
        out.push(statement);
        match statement {
            Statement::If(if_stmt) => {
                statements_in_order(&if_stmt.then_branch, out);
                for (_, branch) in &if_stmt.elif_branches {
                    statements_in_order(branch, out);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    statements_in_order(branch, out);
                }
            }
            Statement::For(for_loop) => statements_in_order(&for_loop.body, out),
            Statement::While(while_loop) => statements_in_order(&while_loop.body, out),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    statements_in_order(&case.body, out);
                }
            }
            Statement::Execute(exec_block) => statements_in_order(&exec_block.body, out),
            _ => {}
        }
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
                persist: assign.persist,
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
                index: for_loop.index.clone(),
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
        Statement::MultiAssign(multi) => {
            let substitute = |expressions: &[Expression]| {
                expressions
                    .iter()
                    .map(|expression| substitute_expression(expression, target, value))
                    .collect::<Result<Vec<_>, TranspileError>>()
            };
            Statement::MultiAssign(MultiAssign {
                targets: substitute(&multi.targets)?,
                values: substitute(&multi.values)?,
            })
        }
        Statement::Pass => Statement::Pass,
        Statement::MinecraftCommand(command) => Statement::MinecraftCommand(
            replace_unroll_placeholder(command, target, &value.as_placed_text()),
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
                &value.as_placed_text(),
            ),
        }),
        Statement::EntityDef(entity_def) => Statement::EntityDef(EntityDef {
            name: entity_def.name.clone(),
            selector: replace_unroll_placeholder(
                &entity_def.selector,
                target,
                &value.as_placed_text(),
            ),
            nbt: substitute_expression(&entity_def.nbt, target, value)?,
        }),
        Statement::CreateEntity(name) => Statement::CreateEntity(name.clone()),
    };
    Ok(substituted)
}

/// An execute header condition with the loop variable `target` replaced in
/// its Python parts, as `i` in `entity @a and x > i`, the way a Python `if`
/// replaces it. Minecraft conditions take the value through `{i}` only.
fn substitute_raw_condition(condition: &str, target: &str, value: &UnrollValue) -> String {
    let python_value = match value {
        UnrollValue::Number(number) => format_unroll_number(*number),
        UnrollValue::String(text) => format!("\"{}\"", crate::transpiler::escape_quoted(text)),
        UnrollValue::Boolean(true) => "True".to_string(),
        UnrollValue::Boolean(false) => "False".to_string(),
    };
    if let Some((masked, minecraft)) = crate::transpiler::mask_minecraft_conditions(condition) {
        let mut text = replace_python_name(&masked, target, &python_value);
        for (identifier, original) in minecraft {
            text = text.replacen(&identifier, &original, 1);
        }
        return text;
    }
    if crate::transpiler::raw_minecraft_condition_verdict(condition.trim_start()) == Some(true) {
        return condition.to_string();
    }
    replace_python_name(condition, target, &python_value)
}

/// `text` with each Python name `name` outside strings, and not after a
/// `.`, replaced by `value`.
fn replace_python_name(text: &str, name: &str, value: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut quote: Option<char> = None;
    let mut escaped = false;
    let mut chars = text.char_indices().peekable();
    while let Some((index, ch)) = chars.next() {
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == open {
                quote = None;
            }
            result.push(ch);
            continue;
        }
        if ch == '"' || ch == '\'' {
            quote = Some(ch);
            result.push(ch);
            continue;
        }
        if ch.is_alphabetic() || ch == '_' {
            let mut end = index + ch.len_utf8();
            while let Some(&(next, next_ch)) = chars.peek() {
                if next_ch.is_alphanumeric() || next_ch == '_' {
                    end = next + next_ch.len_utf8();
                    chars.next();
                } else {
                    break;
                }
            }
            let word = &text[index..end];
            let after_dot = text[..index].trim_end().ends_with('.');
            if word == name && !after_dot {
                result.push_str(value);
            } else {
                result.push_str(word);
            }
            continue;
        }
        result.push(ch);
    }
    result
}

fn substitute_execute_modifier(
    modifier: &ExecuteModifier,
    target: &str,
    value: &UnrollValue,
) -> Result<ExecuteModifier, TranspileError> {
    let command_value = value.as_placed_text();
    let substituted = match modifier {
        ExecuteModifier::As(raw) => {
            ExecuteModifier::As(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::At(raw) => {
            ExecuteModifier::At(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::IfRaw(raw) => ExecuteModifier::IfRaw(substitute_raw_condition(
            &replace_unroll_placeholder(raw, target, &command_value),
            target,
            value,
        )),
        ExecuteModifier::UnlessRaw(raw) => ExecuteModifier::UnlessRaw(substitute_raw_condition(
            &replace_unroll_placeholder(raw, target, &command_value),
            target,
            value,
        )),
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
        ExecuteModifier::Other(raw) => {
            ExecuteModifier::Other(replace_unroll_placeholder(raw, target, &command_value))
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
        Expression::MinecraftCondition(raw) => {
            Expression::MinecraftCondition(substitute_raw_condition(
                &replace_unroll_placeholder(raw, target, &value.as_command_text()),
                target,
                value,
            ))
        }
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
        Expression::Slice(base, start, stop) => {
            let bound = |bound: &Option<Box<Expression>>| {
                bound
                    .as_ref()
                    .map(|bound| substitute_expression(bound, target, value).map(Box::new))
                    .transpose()
            };
            Expression::Slice(
                Box::new(substitute_expression(base, target, value)?),
                bound(start)?,
                bound(stop)?,
            )
        }
        Expression::Conditional(condition, then_value, otherwise) => Expression::Conditional(
            Box::new(substitute_expression(condition, target, value)?),
            Box::new(substitute_expression(then_value, target, value)?),
            Box::new(substitute_expression(otherwise, target, value)?),
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

/// `text` with each `{target}` placeholder replaced, read as the command
/// processor reads placeholders: an escape `{{` or `}}` stays for it, and a
/// placeholder between escapes is replaced too.
fn replace_unroll_placeholder(text: &str, target: &str, replacement: &str) -> String {
    placeholders::pieces(text)
        .into_iter()
        .map(|piece| match piece {
            Piece::Field { name, .. } if name == target => replacement,
            Piece::Field { raw, .. } | Piece::Text { raw, .. } => raw,
        })
        .collect()
}
