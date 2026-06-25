use crate::ast::*;
use crate::transpiler::{GeneratedCommand, GeneratedCommandKind, SourceLocation, Transpiler};
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
    pub(in crate::transpiler) fn process_for(&mut self, for_loop: &ForLoop) -> Result<(), String> {
        let loop_source_key = Self::for_loop_source_key(for_loop);
        let values = self.unroll_values(for_loop).map_err(|error| {
            annotate_unroll_error(
                error,
                &for_loop.target,
                &loop_source_key,
                self.current_statement_source.as_ref(),
            )
        })?;
        let previous_iteration_factor = self.unroll_iteration_factor;
        let projected_iterations = previous_iteration_factor
            .checked_mul(values.len())
            .ok_or_else(|| {
                annotate_unroll_error(
                    format!(
                        "unroll-limit-exceeded: nested unrolling expands past {} aggregate iterations.",
                        UNROLL_NESTED_ITERATION_LIMIT
                    ),
                    &for_loop.target,
                    &loop_source_key,
                    self.current_statement_source.as_ref(),
                )
            })?;
        if projected_iterations > UNROLL_NESTED_ITERATION_LIMIT {
            return Err(annotate_unroll_error(
                format!(
                    "unroll-limit-exceeded: nested unrolling expands to {} aggregate iterations (limit {}).",
                    projected_iterations, UNROLL_NESTED_ITERATION_LIMIT
                ),
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
            for value in values {
                let substituted = substitute_statements(&for_loop.body, &for_loop.target, &value)?;
                for statement in substituted {
                    let start_index = self.current_function.as_ref().map(Vec::len);
                    self.process_statement(&statement)?;
                    if let Some(start_index) = start_index {
                        self.mark_current_function_commands_as_unrolled(start_index);
                    }
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

    fn unroll_values(&self, for_loop: &ForLoop) -> Result<Vec<UnrollValue>, String> {
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
                Err(format!(
                    "unroll-non-literal: for loops only support literal range(...) or literal arrays, not {name}(...)."
                ))
            }
            _ => Err(
                "unroll-non-literal: for loops only support literal range(...) or literal arrays."
                    .to_string(),
            ),
        }
    }

    fn unroll_array_values(&self, items: &[Expression]) -> Result<Vec<UnrollValue>, String> {
        if items.is_empty() {
            return Err(
                "unroll-bad-range: literal arrays used for unrolling cannot be empty.".to_string(),
            );
        }
        if items.len() > UNROLL_LIMIT {
            return Err(format!(
                "unroll-limit-exceeded: literal array expands to {} iterations (limit {}).",
                items.len(),
                UNROLL_LIMIT
            ));
        }

        items
            .iter()
            .map(|item| match item {
                Expression::Number(value) => Ok(UnrollValue::Number(*value)),
                Expression::String(value) => Ok(UnrollValue::String(value.clone())),
                Expression::Boolean(value) => Ok(UnrollValue::Boolean(*value)),
                _ => Err(
                    "unroll-non-literal: literal arrays may only contain numbers, strings, or booleans."
                        .to_string(),
                ),
            })
            .collect()
    }

    fn unroll_range_values(
        &self,
        for_loop: &ForLoop,
        args: &[Expression],
    ) -> Result<Vec<UnrollValue>, String> {
        if args.is_empty() || args.len() > 3 {
            return Err(format!(
                "unroll-non-literal: range(...) expects 1 to 3 arguments, got {}.",
                args.len()
            ));
        }

        let by_step = for_loop
            .step
            .as_ref()
            .map(|step| self.integer_unroll_expr(step, "range step"))
            .transpose()?;
        if args.len() == 3 && by_step.is_some() {
            return Err(
                "unroll-bad-step: use either range(start, stop, step) or 'by step', not both."
                    .to_string(),
            );
        }

        let (start, stop, step) = match args.len() {
            1 => {
                let count = self.integer_unroll_expr(&args[0], "range count")?;
                if count < 0 {
                    return Err(format!(
                        "unroll-bad-range: range({count}) cannot expand a negative count."
                    ));
                }
                let step = by_step.unwrap_or(1);
                if step == 0 {
                    return Err("unroll-bad-step: range step cannot be zero.".to_string());
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
                    return Err("unroll-bad-step: range step cannot be zero.".to_string());
                }
                (start, stop, step)
            }
            3 => {
                let start = self.integer_unroll_expr(&args[0], "range start")?;
                let stop = self.integer_unroll_expr(&args[1], "range stop")?;
                let step = self.integer_unroll_expr(&args[2], "range step")?;
                if step == 0 {
                    return Err("unroll-bad-step: range step cannot be zero.".to_string());
                }
                (start, stop, step)
            }
            _ => unreachable!(),
        };

        if args.len() != 1 {
            if step > 0 && start > stop {
                return Err(format!(
                    "unroll-bad-range: range start {start} is greater than stop {stop} with a positive step."
                ));
            }
            if step < 0 && start < stop {
                return Err(format!(
                    "unroll-bad-range: range start {start} is less than stop {stop} with a negative step."
                ));
            }
        }

        range_unroll_values(start, stop, step)
    }

    fn integer_unroll_expr(&self, expr: &Expression, label: &str) -> Result<i32, String> {
        let value = self.try_eval_const(expr).ok_or_else(|| {
            format!("unroll-non-literal: {label} must be an integer literal or const identifier.")
        })?;
        if !value.is_finite() || value.fract() != 0.0 {
            return Err(format!("unroll-bad-step: {label} must be an integer."));
        }
        if value < i32::MIN as f64 || value > i32::MAX as f64 {
            return Err(format!(
                "unroll-bad-range: {label} value {value} is outside the i32 range."
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
            metadata.insert(
                index,
                GeneratedCommand::new(
                    command.clone(),
                    source.clone(),
                    GeneratedCommandKind::Unrolled,
                ),
            );
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

    fn ensure_unroll_command_budget(&self) -> Result<(), String> {
        let Some(baseline) = self.unroll_command_baseline else {
            return Ok(());
        };
        let generated = self.generated_command_count().saturating_sub(baseline);
        if generated > UNROLL_GENERATED_COMMAND_LIMIT {
            return Err(format!(
                "unroll-limit-exceeded: unrolling generated {} commands (limit {}).",
                generated, UNROLL_GENERATED_COMMAND_LIMIT
            ));
        }
        Ok(())
    }

    pub(in crate::transpiler) fn process_while(
        &mut self,
        while_loop: &WhileLoop,
    ) -> Result<(), String> {
        let mut while_commands = Vec::new();

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

        // Generate a recursive function for the while loop
        let loop_func_name = format!("while_temp_{}", self.temp_counter);
        self.temp_counter += 1;

        // Capture condition setup so complex expressions and OR lowering are
        // rerun on every recursive iteration instead of once before the loop.
        let (condition_cmd, condition_capture) =
            self.capture_commands_with_result(|transpiler| {
                let processed_condition = transpiler.preprocess_condition(&while_loop.condition)?;
                let translated = transpiler.translate_condition(&processed_condition)?;
                transpiler.normalize_if_condition(translated)
            })?;
        let condition_execute_args = Self::condition_execute_args(&condition_cmd);

        // IMPORTANT: We need to wrap the body in a conditional function call
        // to prevent bugs where body statements modify condition variables.
        // The condition should be evaluated ONCE per iteration, not per statement.

        // Create inner body function that executes unconditionally
        let body_func_name = format!("while_body_{}", self.temp_counter);
        self.temp_counter += 1;

        // Process loop body into the body function
        let saved_context = self.current_context.clone();

        let capture = self.capture_statements(&while_loop.body)?;
        let body_needs_storage = capture.requires_macro_context();
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

        // Add the loop function to the data pack
        self.data_pack.add_function_with_metadata(
            loop_func_name.clone(),
            loop_commands,
            loop_metadata,
        );

        // Start the loop
        while_commands.push(format!(
            "function {}:{}",
            self.data_pack.namespace, loop_func_name
        ));

        if let Some(ref mut commands) = self.current_function {
            commands.extend(while_commands);
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

fn annotate_unroll_error(
    error: String,
    target: &str,
    source_key: &str,
    location: Option<&SourceLocation>,
) -> String {
    if error.starts_with("unroll-") {
        let mut annotated = if error.contains("\n  loop: for ") {
            error
        } else {
            format!("{error}\n  loop: for {target}")
        };
        if !annotated.contains("\n  source-key: ") {
            annotated.push_str(&format!("\n  source-key: {source_key}"));
        }
        if let Some(location) = location.filter(|_| !annotated.contains("\n  location: ")) {
            annotated.push_str(&format!(
                "\n  location: {}:{}",
                location.line, location.column
            ));
        }
        annotated
    } else {
        error
    }
}

fn is_identifier(expr: &Expression, expected: &str) -> bool {
    matches!(expr, Expression::Identifier(name) if name == expected)
}

fn range_unroll_values(start: i32, stop: i32, step: i32) -> Result<Vec<UnrollValue>, String> {
    let mut values = Vec::new();
    let mut current = i64::from(start);
    let stop = i64::from(stop);
    let step = i64::from(step);

    while (step > 0 && current < stop) || (step < 0 && current > stop) {
        if values.len() == UNROLL_LIMIT {
            return Err(format!(
                "unroll-limit-exceeded: range expands past {} iterations.",
                UNROLL_LIMIT
            ));
        }
        if current < i64::from(i32::MIN) || current > i64::from(i32::MAX) {
            return Err(format!(
                "unroll-bad-range: range value {current} is outside the i32 range."
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
) -> Result<Vec<Statement>, String> {
    statements
        .iter()
        .map(|statement| substitute_statement(statement, target, value))
        .collect()
}

fn substitute_statement(
    statement: &Statement,
    target: &str,
    value: &UnrollValue,
) -> Result<Statement, String> {
    let substituted = match statement {
        Statement::Import(import) => Statement::Import(import.clone()),
        Statement::FunctionDef(function) => {
            if function.params.iter().any(|param| param.name == target) {
                return Err(format!(
                    "unroll-non-literal: nested function parameter '{}' shadows the unrolled loop target.",
                    target
                ));
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
                return Err(format!(
                    "unroll-non-literal: cannot assign to unrolled loop target '{}'.",
                    target
                ));
            }
            Statement::Assignment(Assignment {
                target: assign.target.clone(),
                value: substitute_expression(&assign.value, target, value)?,
            })
        }
        Statement::ConstAssignment(assign) => {
            if assign.target == target {
                return Err(format!(
                    "unroll-non-literal: cannot assign to unrolled loop target '{}'.",
                    target
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
                .collect::<Result<Vec<_>, String>>()?,
            else_branch: if_stmt
                .else_branch
                .as_ref()
                .map(|body| substitute_statements(body, target, value))
                .transpose()?,
        }),
        Statement::For(for_loop) => {
            if for_loop.target == target {
                return Err(format!(
                    "unroll-non-literal: nested loop target '{}' shadows the unrolled loop target.",
                    target
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
                .collect::<Result<Vec<_>, String>>()?,
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
                .collect::<Result<Vec<_>, String>>()?,
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
) -> Result<ExecuteModifier, String> {
    let command_value = value.as_command_text();
    let substituted = match modifier {
        ExecuteModifier::As(raw) => {
            ExecuteModifier::As(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::At(raw) => {
            ExecuteModifier::At(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::If(expr) => {
            ExecuteModifier::If(substitute_expression(expr, target, value)?)
        }
        ExecuteModifier::IfRaw(raw) => {
            ExecuteModifier::IfRaw(replace_unroll_placeholder(raw, target, &command_value))
        }
        ExecuteModifier::Unless(expr) => {
            ExecuteModifier::Unless(substitute_expression(expr, target, value)?)
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
) -> Result<Expression, String> {
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
                .collect::<Result<Vec<_>, String>>()?,
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
                .collect::<Result<Vec<_>, String>>()?,
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
                .collect::<Result<Vec<_>, String>>()?,
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
