use crate::ast::{BinaryOp, CobbleType, Expression, Statement, UnaryOp};
use crate::transpiler::condition_translator::{ConditionTranslator, TranslatedCondition};
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{FunctionCapture, TranspileError, Transpiler};

/// Execute-condition keywords that start a raw Minecraft condition
/// (`entity @a[...]`, `score x temp matches 5`, `data storage ...`).
pub(in crate::transpiler) const RAW_CONDITION_KEYWORDS: [&str; 13] = [
    "score ",     // "score <target> <objective> matches/</>/>=/<=/= ..."
    "entity ",    // "entity <selector>"
    "block ",     // "block <pos> <block>"
    "blocks ",    // "blocks <start> <end> <dest> <mode>"
    "biome ",     // "biome <pos> <biome>"
    "dimension ", // "dimension <dimension>"
    "predicate ", // "predicate <predicate>"
    "data ",      // "data <source> <path>"
    "items ",     // "items <source> <slots> <item>"
    "function ",  // "function <function/tag>"
    "loaded ",    // "loaded <pos>"
    "slots ",     // "slots <source> <slots> <count>"
    "stopwatch ", // "stopwatch <id> <range>"
];

/// `Some(true)` for a raw Minecraft condition, `Some(false)` for a Python
/// expression that merely starts with a keyword-named variable
/// (`score >= 10`), and `None` when the condition starts with no keyword.
/// Shared with the diagnostics layer so both agree on which `if` headers
/// carry raw syntax rather than variable references.
pub(crate) fn raw_minecraft_condition_verdict(condition: &str) -> Option<bool> {
    if raw_score_condition_end(condition).is_some() {
        return Some(true);
    }
    let rest = RAW_CONDITION_KEYWORDS
        .iter()
        .find_map(|keyword| condition.strip_prefix(keyword))?;
    let next_token = rest.split_whitespace().next().unwrap_or("");
    Some(!matches!(
        next_token,
        "==" | "!=" | ">=" | "<=" | ">" | "<" | "and" | "or" | "not" | "+" | "-" | "*" | "/" | "%"
    ))
}

/// The end of a score condition's required arguments. A holder or objective
/// named `and`, `or`, or `not` is part of those arguments, not a logical
/// operator separating conditions.
pub(in crate::transpiler) fn raw_score_condition_end(condition: &str) -> Option<usize> {
    if !condition.starts_with("score ") {
        return None;
    }
    let words = super::command_processor::split_command_words(condition, usize::MAX);
    let end = match *words.get(3)? {
        "matches" => *words.get(4)?,
        "=" | "<" | ">" | "<=" | ">=" => *words.get(5)?,
        _ => return None,
    };
    Some(end.as_ptr() as usize - condition.as_ptr() as usize + end.len())
}

/// The Python portion of a mixed Minecraft header, for semantic walks.
pub(crate) fn minecraft_condition_expression(raw: &str) -> Option<Expression> {
    let (masked, _) = mask_minecraft_conditions(raw)?;
    Transpiler::parse_raw_condition_expression(&masked)
}

/// A condition that combines Minecraft conditions with Python with `and`,
/// `or`, or `not`, as `entity @a and x > 3` or `not entity @a`, with each
/// Minecraft condition replaced by an identifier, so the whole condition
/// parses as Python. Returns the masked text and (identifier, condition)
/// pairs; while the masked text is translated, each identifier translates to
/// its condition (`Symbols::minecraft_conditions`). A condition that is all
/// Minecraft conditions joined by `and` or `or` gives `None` unless it has
/// a `function` operand, whose side effects need short-circuit lowering.
/// A condition without Minecraft operands also gives `None`.
pub(crate) fn mask_minecraft_conditions(
    condition: &str,
) -> Option<(String, Vec<(String, String)>)> {
    let (masked, conditions, _, mixed) = scan_minecraft_conditions(condition)?;
    mixed.then_some((masked, conditions))
}

pub(crate) fn minecraft_condition_ranges(condition: &str) -> Vec<std::ops::Range<usize>> {
    scan_minecraft_conditions(condition)
        .map(|(_, _, ranges, _)| ranges)
        .unwrap_or_default()
}

type ScannedConditions = (
    String,
    Vec<(String, String)>,
    Vec<std::ops::Range<usize>>,
    bool,
);

fn scan_minecraft_conditions(condition: &str) -> Option<ScannedConditions> {
    let mut masked = String::with_capacity(condition.len());
    let mut conditions: Vec<(String, String)> = Vec::new();
    let mut ranges = Vec::new();
    let mut python_operand = false;
    let mut negated_or_grouped = false;
    let mut rest = condition.trim();
    loop {
        loop {
            if let Some(after) = rest.strip_prefix("not ") {
                masked.push_str("not ");
                negated_or_grouped = true;
                rest = after.trim_start();
            } else if let Some(after) = rest.strip_prefix('(') {
                masked.push('(');
                negated_or_grouped = true;
                rest = after.trim_start();
            } else {
                break;
            }
        }
        let operand = rest[..operand_end(rest)].trim_end();
        if operand.is_empty() {
            return None;
        }
        if raw_minecraft_condition_verdict(operand) == Some(true) {
            let start = condition.trim_end().len() - rest.len();
            ranges.push(start..start + operand.len());
            let identifier = format!("__cobble_mc_condition_{}__", conditions.len());
            masked.push_str(&identifier);
            conditions.push((identifier, operand.to_string()));
        } else {
            python_operand = true;
            masked.push_str(operand);
        }
        rest = rest[operand.len()..].trim_start();
        while let Some(after) = rest.strip_prefix(')') {
            masked.push(')');
            rest = after.trim_start();
        }
        if rest.is_empty() {
            break;
        }
        let (operator, after) = rest
            .strip_prefix("and ")
            .map(|after| ("and", after))
            .or_else(|| rest.strip_prefix("or ").map(|after| ("or", after)))?;
        masked.push_str(&format!(" {operator} "));
        rest = after.trim_start();
    }
    let mixed = !conditions.is_empty()
        && (python_operand
            || negated_or_grouped
            || (conditions.len() > 1
                && conditions
                    .iter()
                    .any(|(_, raw)| raw.starts_with("function "))));
    Some((masked, conditions, ranges, mixed))
}

/// The parser reads `if entity @a and x in items:` as the condition
/// `entity @a and x` and the modifier `in items`. When the condition before
/// `in` is a mixed condition and what follows is no dimension, the `in` is
/// the Python operator: returns the whole condition.
pub(crate) fn merged_execute_in_condition(condition: &str, container: &str) -> Option<String> {
    let container = container.trim();
    let is_path = |part: &str| {
        !part.is_empty()
            && part.bytes().all(|byte| {
                byte.is_ascii_lowercase()
                    || byte.is_ascii_digit()
                    || matches!(byte, b'_' | b'-' | b'.' | b'/')
            })
    };
    let is_dimension = match container.split_once(':') {
        Some((namespace, path)) => is_path(namespace) && is_path(path),
        None => matches!(container, "overworld" | "the_nether" | "the_end"),
    };
    if is_dimension {
        return None;
    }
    let merged = format!("{} in {}", condition.trim(), container);
    mask_minecraft_conditions(&merged).map(|_| merged)
}

/// The byte offset where the operand at the start of `text` ends: at the
/// next ` and ` or ` or `, or a `)` it didn't open, outside brackets and
/// quotes.
fn operand_end(text: &str) -> usize {
    let bytes = text.as_bytes();
    let mut depth = 0usize;
    let mut quote: Option<u8> = None;
    let mut index = raw_score_condition_end(text).unwrap_or(0);
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            if byte == b'\\' {
                index += 1;
            } else if byte == open {
                quote = None;
            }
        } else {
            match byte {
                b'"' | b'\'' => quote = Some(byte),
                b'(' | b'[' | b'{' => depth += 1,
                b')' if depth == 0 => return index,
                b')' | b']' | b'}' => depth = depth.saturating_sub(1),
                b' ' if depth == 0
                    && (text[index..].starts_with(" and ")
                        || text[index..].starts_with(" or ")) =>
                {
                    return index;
                }
                _ => {}
            }
        }
        index += 1;
    }
    text.len()
}

impl Transpiler {
    fn condition_runs_user_code(&self, condition: &Expression) -> bool {
        match condition {
            Expression::Identifier(name) => self
                .symbols
                .minecraft_conditions
                .get(name)
                .is_some_and(|raw| raw.trim_start().starts_with("function ")),
            Expression::Binary(left, _, right) => {
                (self.uses_storage_numbers()
                    && (self.may_hold_float(left) || self.may_hold_float(right)))
                    || self.condition_runs_user_code(left)
                    || self.condition_runs_user_code(right)
            }
            Expression::Unary(_, inner) => self.condition_runs_user_code(inner),
            _ => ExpressionEvaluator::calls_user_code(condition),
        }
    }

    pub(in crate::transpiler) fn preprocess_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<Expression, TranspileError> {
        if self.in_module_code {
            self.check_module_level_reads(condition)?;
        }
        // Boolean literals read the `#true_const`/`#false_const` scores.
        if Self::contains_boolean_literal(condition) {
            self.data_pack.track_boolean_constants();
        }

        // Check if the condition has a complex expression on the left side of a comparison
        // For example: (x % 3) == 1 or (x ^ 2) > 10
        match condition {
            Expression::Binary(left, op, right) => {
                // Check if this is a comparison operator and left is a binary expression
                let is_comparison = matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Lt
                        | BinaryOp::LtEq
                        | BinaryOp::Gt
                        | BinaryOp::GtEq
                );

                if is_comparison
                    && (self.may_hold_float(left) || self.may_hold_float(right))
                    && self.condition_uses_numeric_storage(left)
                    && self.condition_uses_numeric_storage(right)
                {
                    return self.numeric_condition_value(left, Some((op, right)));
                }

                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
                    && self.compares_stored_values(left, right)
                {
                    return self.lower_stored_equality(left, op, right);
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq) {
                    let (left_stored, right_stored) =
                        (self.stored_operand(left), self.stored_operand(right));
                    if left_stored == Some(false) && right_stored.is_none() {
                        return self.lower_number_equality(left, op, right);
                    }
                    if right_stored == Some(false) && left_stored.is_none() {
                        return self.lower_number_equality(right, op, left);
                    }
                }

                if is_comparison {
                    // Evaluate each complex side (arithmetic, calls, storage
                    // reads, macro parameters) into its own unique temp, left
                    // first so side effects keep source order. Both sides are
                    // checked: `actual == expected` with two parameters must
                    // not leave the right one as an unset score name.
                    // Python reads a variable on the left before a call on
                    // the right runs, and the call may change it.
                    let read_left_first = ExpressionEvaluator::calls_user_code(right)
                        && matches!(&**left, Expression::Identifier(name)
                            if self.symbols.scoreboard_variables.contains(name)
                                || self.symbols.variable_objectives.contains_key(name));
                    let new_left = if let Some(folded) = self.fold_condition_operand(left) {
                        folded
                    } else if self.condition_operand_needs_temp(left) || read_left_first {
                        self.lower_condition_value_to_temp(left)?
                    } else {
                        (**left).clone()
                    };
                    let new_right = if let Some(folded) = self.fold_condition_operand(right) {
                        folded
                    } else if self.condition_operand_needs_temp(right) {
                        self.lower_condition_value_to_temp(right)?
                    } else {
                        (**right).clone()
                    };
                    Ok(Expression::Binary(
                        Box::new(new_left),
                        op.clone(),
                        Box::new(new_right),
                    ))
                } else {
                    // Not a comparison, might be And/Or - recursively preprocess
                    match op {
                        BinaryOp::And | BinaryOp::Or if self.condition_runs_user_code(right) => {
                            self.lower_short_circuit_condition(left, op, right)
                        }
                        BinaryOp::And | BinaryOp::Or => {
                            let new_left = self.preprocess_condition(left)?;
                            let new_right = self.preprocess_condition(right)?;
                            Ok(Expression::Binary(
                                Box::new(new_left),
                                op.clone(),
                                Box::new(new_right),
                            ))
                        }
                        _ => {
                            // Arithmetic expressions in condition position use numeric truthiness.
                            if self.condition_uses_numeric_storage(condition) {
                                self.numeric_condition_value(condition, None)
                            } else {
                                self.lower_condition_value_to_temp(condition)
                            }
                        }
                    }
                }
            }
            Expression::Unary(UnaryOp::Not, expr) => {
                let new_expr = self.preprocess_condition(expr)?;
                Ok(Expression::Unary(UnaryOp::Not, Box::new(new_expr)))
            }
            // `-x` in condition position is a number, true when it is not 0.
            Expression::Unary(..) if self.condition_uses_numeric_storage(condition) => {
                self.numeric_condition_value(condition, None)
            }
            Expression::Unary(..) => self.lower_condition_value_to_temp(condition),
            // A number literal is true when it is not 0, like a variable.
            Expression::Number(value) => {
                self.warn_score_value(*value);
                self.data_pack.track_boolean_constants();
                Ok(Expression::Boolean(*value != 0.0))
            }
            expr @ (Expression::Identifier(_)
            | Expression::Call(..)
            | Expression::Subscript(..)
            | Expression::Attribute(..))
                if self.condition_uses_numeric_storage(expr) && self.may_hold_float(expr) =>
            {
                self.numeric_condition_value(expr, None)
            }
            // Text, a list, or a map is true when it isn't empty.
            Expression::Identifier(name)
                if matches!(
                    self.symbols.variable_types.get(name),
                    Some(CobbleType::String | CobbleType::List | CobbleType::Map)
                ) && self.symbols.variable_storage_paths.contains_key(name) =>
            {
                let size = Expression::Call(
                    Box::new(Expression::Identifier("len".to_string())),
                    vec![condition.clone()],
                );
                self.lower_condition_value_to_temp(&size)
            }
            // Literal text is true when it isn't empty.
            Expression::String(text) => {
                self.data_pack.track_boolean_constants();
                Ok(Expression::Boolean(!text.is_empty()))
            }
            // A character of text is true when it isn't empty, and so is an
            // item that may be text; `len` reads a number as itself.
            Expression::Subscript(..) if self.picks_computed_value(condition) => {
                let size = Expression::Call(
                    Box::new(Expression::Identifier("len".to_string())),
                    vec![condition.clone()],
                );
                self.lower_condition_value_to_temp(&size)
            }
            expr if self.condition_operand_needs_temp(expr) => {
                self.lower_condition_value_to_temp(expr)
            }
            _ => Ok(condition.clone()),
        }
    }

    /// `a and f()` / `a or f()` where the right side calls a function: like
    /// Python, the call runs only when the left side doesn't decide the
    /// result. The right side is evaluated in a helper that runs under the
    /// left side's condition and returns 0 or 1 into a flag, which the
    /// caller then tests. The flag is written outside the helper, so under
    /// `as @a` it can be scoped to each entity like the other condition
    /// scores. Right sides without calls keep the plain `if A if B` chain;
    /// evaluating them eagerly has no visible effect.
    pub(in crate::transpiler) fn lower_short_circuit_condition(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<Expression, TranspileError> {
        let is_and = matches!(op, BinaryOp::And);
        let id = self.next_temp_id();
        let flag = format!("cond_{}_{}", if is_and { "and" } else { "or" }, id);
        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(flag.clone(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(flag.clone());

        let left_condition = self.lower_condition_to_execute_args(left)?;
        let (right_condition, mut right_capture) =
            self.capture_commands_with_result(|transpiler| {
                transpiler.lower_condition_to_execute_args(right)
            })?;
        right_capture
            .commands
            .push(format!("execute {} run return 1", right_condition));
        right_capture.commands.push("return 0".to_string());
        let helper_name = format!("cond_rhs_{}", id);
        let needs_storage = right_capture.requires_macro_context();
        self.add_captured_function(helper_name.clone(), right_capture);
        let call = self.function_call_command(&helper_name, needs_storage);

        let mut commands = vec![format!("scoreboard players set {} temp 0", flag)];
        let evaluate_right = if is_and {
            left_condition
        } else {
            commands.push(format!(
                "execute {} run scoreboard players set {} temp 1",
                left_condition, flag
            ));
            format!("if score {} temp matches 0", flag)
        };
        commands.push(format!(
            "execute {} store result score {} temp run {}",
            evaluate_right, flag, call
        ));
        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands.into_iter().map(|command| {
                if command.contains("$(") {
                    format!("${}", command)
                } else {
                    command
                }
            }));
        }
        Ok(Expression::Identifier(flag))
    }

    /// Preprocess, translate, and lower a condition to `if ...`/`unless ...`
    /// execute arguments, emitting any setup commands it needs.
    pub(in crate::transpiler) fn lower_condition_to_execute_args(
        &mut self,
        condition: &Expression,
    ) -> Result<String, TranspileError> {
        if let Expression::MinecraftCondition(raw) = condition {
            let flag = self.hidden_local(CobbleType::Integer);
            self.process_assignment(&crate::ast::Assignment {
                target: flag.clone(),
                target_expr: None,
                value: Expression::Number(0.0),
                persist: false,
            })?;
            self.process_execute_block(&crate::ast::ExecuteBlock {
                modifiers: vec![crate::ast::ExecuteModifier::IfRaw(raw.clone())],
                body: vec![Statement::Assignment(crate::ast::Assignment {
                    target: flag.clone(),
                    target_expr: None,
                    value: Expression::Number(1.0),
                    persist: false,
                })],
            })?;
            return Ok(format!("if score {flag} temp matches 1"));
        }
        let processed = self.preprocess_condition(condition)?;
        let translated = self.translate_condition(&processed)?;
        let lowered = self.lower_translated_condition(translated)?;
        Ok(Self::condition_execute_args(&lowered))
    }

    /// Folds a literal-only comparison operand, such as `step - 1` inside an
    /// unrolled loop, so it needs no scratch score. Integer `/` and `%` fold
    /// with the same floor semantics as the scoreboard.
    pub(in crate::transpiler) fn fold_condition_operand(
        &self,
        expr: &Expression,
    ) -> Option<Expression> {
        // A sign belongs to a literal threshold, including a fractional
        // literal or const; it must not first pass through an integer score.
        fn signed_literal(transpiler: &Transpiler, expr: &Expression) -> Option<f64> {
            match expr {
                Expression::Number(value) => Some(*value),
                Expression::Identifier(name) => {
                    transpiler.symbols.compile_time_constants.get(name).copied()
                }
                Expression::Unary(UnaryOp::Neg, inner) => {
                    signed_literal(transpiler, inner).map(|value| -value)
                }
                Expression::Unary(UnaryOp::Pos, inner) => signed_literal(transpiler, inner),
                _ => None,
            }
        }
        if matches!(expr, Expression::Unary(UnaryOp::Neg | UnaryOp::Pos, _)) {
            if let Some(value) = signed_literal(self, expr) {
                return Some(Expression::Number(value));
            }
        }
        if !matches!(expr, Expression::Binary(..) | Expression::Unary(..)) {
            return None;
        }
        let value = self.try_eval_const(expr)?;
        (value.fract() == 0.0 && value.abs() <= f64::from(i32::MAX))
            .then_some(Expression::Number(value))
    }

    pub(in crate::transpiler) fn lower_condition_value_to_temp(
        &mut self,
        expr: &Expression,
    ) -> Result<Expression, TranspileError> {
        let temp_var = format!("expr_cond_temp_{}", self.next_temp_id());

        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(temp_var.clone(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(temp_var.clone());

        let eval_commands = self.evaluate_expression_to_target(expr, &temp_var)?;
        if let Some(ref mut commands) = self.current_function {
            commands.extend(eval_commands);
        }

        Ok(Expression::Identifier(temp_var))
    }

    /// How an operand of `==` is stored: `Some(true)` for text, a list, or a
    /// map, `Some(false)` for a value that may be one, as a parameter, and
    /// `None` for a number.
    fn stored_operand(&self, expr: &Expression) -> Option<bool> {
        let of_type = |value_type: Option<&CobbleType>, stored: bool| match value_type {
            Some(CobbleType::String | CobbleType::List | CobbleType::Map) => Some(true),
            Some(CobbleType::Integer | CobbleType::Boolean) => None,
            _ => stored.then_some(false),
        };
        match expr {
            Expression::String(_) | Expression::Array(_) | Expression::Map(_) => Some(true),
            Expression::Identifier(name) => {
                if self.symbols.compile_time_constants.contains_key(name) {
                    return None;
                }
                of_type(
                    self.symbols.variable_types.get(name),
                    self.is_current_macro_param(name)
                        || self.symbols.variable_storage_paths.contains_key(name),
                )
            }
            Expression::Subscript(base, _) if self.is_string_base(base) => Some(true),
            Expression::Subscript(..) | Expression::Attribute(..) => of_type(
                Some(&self.storage_read_type(expr)),
                self.resolve_storage_path_plan(expr).is_some(),
            ),
            _ => None,
        }
    }

    /// Whether `left == right` compares stored values, which have no score.
    /// Two values that may be numbers or text are compared as stored too.
    fn compares_stored_values(&self, left: &Expression, right: &Expression) -> bool {
        matches!(
            (self.stored_operand(left), self.stored_operand(right)),
            (Some(true), _) | (_, Some(true)) | (Some(false), Some(false))
        )
    }

    /// `left == right` on stored values: `data modify` fails when the value
    /// it would write is already there, so its success says they differ.
    fn lower_stored_equality(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<Expression, TranspileError> {
        let id = self.next_temp_id();
        let namespace = self.data_pack.namespace.clone();
        let root = format!("_cobble_compare_{id}");
        let (first, second) = (format!("{root}.first"), format!("{root}.second"));
        let differs = format!("expr_cond_temp_{id}");

        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(differs.clone(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(differs.clone());

        let mut commands = self.storage_value_commands(&first, left)?;
        commands.extend(self.storage_value_commands(&second, right)?);
        commands.extend([
            format!(
                "execute store success score {differs} temp run data modify storage {namespace}:global {first} set from storage {namespace}:global {second}"
            ),
            // Nothing to copy also fails, but a value and no value differ.
            format!(
                "execute if data storage {namespace}:global {first} unless data storage {namespace}:global {second} run scoreboard players set {differs} temp 1"
            ),
            format!("data remove storage {namespace}:global {root}"),
        ]);
        self.extend_current_function(commands);

        let expected = if matches!(op, BinaryOp::Eq) { 0.0 } else { 1.0 };
        Ok(Expression::Binary(
            Box::new(Expression::Identifier(differs)),
            BinaryOp::Eq,
            Box::new(Expression::Number(expected)),
        ))
    }

    /// `value == number`, where `value` may hold text, a list, or a map as
    /// well as a number: only a number can be equal, as in Python. Text has
    /// itself as its text, and a list or a map has none, so a value is a
    /// number when `set string` works and changes it.
    fn lower_number_equality(
        &mut self,
        value: &Expression,
        op: &BinaryOp,
        number: &Expression,
    ) -> Result<Expression, TranspileError> {
        let id = self.next_temp_id();
        let namespace = self.data_pack.namespace.clone();
        let root = format!("_cobble_compare_{id}");
        let (copy, text) = (format!("{root}.value"), format!("{root}.text"));
        let is_number = format!("expr_kind_temp_{id}");
        let read = format!("expr_cond_temp_{id}");

        self.data_pack.track_objective("temp");
        for name in [&is_number, &read] {
            self.symbols
                .variable_objectives
                .insert(name.clone(), "temp".to_string());
            self.symbols.scoreboard_variables.insert(name.clone());
        }

        let mut commands = vec![format!("data remove storage {namespace}:global {root}")];
        commands.extend(self.storage_value_commands(&copy, value)?);
        commands.extend([
            format!(
                "execute store success score {is_number} temp run data modify storage {namespace}:global {text} set string storage {namespace}:global {copy}"
            ),
            format!(
                "execute if score {is_number} temp matches 1 store success score {is_number} temp run data modify storage {namespace}:global {text} set from storage {namespace}:global {copy}"
            ),
            format!(
                "execute store result score {read} temp run data get storage {namespace}:global {copy}"
            ),
            format!("data remove storage {namespace}:global {root}"),
        ]);
        self.extend_current_function(commands);
        let number = match self.fold_condition_operand(number) {
            Some(folded) => folded,
            None if self.condition_operand_needs_temp(number) => {
                self.lower_condition_value_to_temp(number)?
            }
            None => number.clone(),
        };

        let flag = |expected: f64| {
            Expression::Binary(
                Box::new(Expression::Identifier(is_number.clone())),
                BinaryOp::Eq,
                Box::new(Expression::Number(expected)),
            )
        };
        let compared = Expression::Binary(
            Box::new(Expression::Identifier(read.clone())),
            op.clone(),
            Box::new(number),
        );
        Ok(if matches!(op, BinaryOp::Eq) {
            Expression::Binary(Box::new(flag(1.0)), BinaryOp::And, Box::new(compared))
        } else {
            Expression::Binary(Box::new(flag(0.0)), BinaryOp::Or, Box::new(compared))
        })
    }

    pub(in crate::transpiler) fn condition_operand_needs_temp(&self, expr: &Expression) -> bool {
        matches!(
            expr,
            Expression::Binary(_, _, _)
                | Expression::Unary(_, _)
                | Expression::Call(_, _)
                | Expression::Attribute(_, _)
                | Expression::Subscript(_, _)
        ) || matches!(expr, Expression::Identifier(name) if self.is_current_macro_param(name))
    }

    /// Check if an expression contains any Boolean literals (True or False)
    pub(in crate::transpiler) fn contains_boolean_literal(expr: &Expression) -> bool {
        match expr {
            Expression::Boolean(_) => true,
            Expression::Binary(left, _, right) => {
                Self::contains_boolean_literal(left) || Self::contains_boolean_literal(right)
            }
            Expression::Unary(_, inner) => Self::contains_boolean_literal(inner),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn capture_preprocessed_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<(TranslatedCondition, FunctionCapture), TranspileError> {
        self.capture_commands_with_result(|transpiler| {
            let processed_condition = transpiler.preprocess_condition(condition)?;
            transpiler.translate_condition(&processed_condition)
        })
    }

    pub(in crate::transpiler) fn capture_raw_python_condition(
        &mut self,
        condition: &str,
    ) -> Result<Option<(TranslatedCondition, FunctionCapture)>, TranspileError> {
        if !self.looks_like_python_expression(condition) {
            return Ok(None);
        }

        // Selector operands (`@s > 5`) and macro-parameter braces
        // (`{count} >= 3`) are valid in raw execute conditions but cannot be
        // parsed as Python expressions; mask them with synthetic identifiers
        // and restore the original spelling after translation.
        let (masked, operand_tokens) = self.mask_condition_operand_tokens(condition);
        let Some(expr) = Self::parse_raw_condition_expression(&masked) else {
            return Ok(None);
        };

        let helpers_mark = self.data_pack.created_functions.len();
        let (translated, mut capture) = self
            .capture_preprocessed_condition(&expr)
            .map_err(|error| Self::restore_condition_operand_tokens(error, &operand_tokens))?;
        let mut translated = Self::fold_always_false_conditions(translated);
        for (placeholder, original) in &operand_tokens {
            translated.map_simple(&|text| text.replace(placeholder.as_str(), original));
            for command in &mut capture.commands {
                *command = command.replace(placeholder, original);
            }
            for generated in capture.metadata.values_mut() {
                generated.text = generated.text.replace(placeholder, original);
            }
            for helper in &self.data_pack.created_functions[helpers_mark..] {
                if let Some(commands) = self.data_pack.functions.get_mut(helper) {
                    for command in commands {
                        *command = command.replace(placeholder, original);
                    }
                }
                if let Some(metadata) = self.data_pack.command_metadata.get_mut(helper) {
                    for generated in metadata.values_mut() {
                        generated.text = generated.text.replace(placeholder, original);
                    }
                }
            }
        }
        Ok(Some((translated, capture)))
    }

    /// Restore the original spelling of masked condition operands in an error
    /// message, so translation failures report `@s`/`{count}` instead of
    /// leaking synthetic `__cobble_cond_operand_N__` placeholder names.
    pub(in crate::transpiler) fn restore_condition_operand_tokens(
        mut error: TranspileError,
        operand_tokens: &[(String, String)],
    ) -> TranspileError {
        for (placeholder, original) in operand_tokens {
            error.message = error.message.replace(placeholder, original);
        }
        error
    }

    /// Replace selector tokens (`@s`, `@a[tag=x]`, ...) and macro-parameter
    /// braces (`{count}`) in a raw condition string with synthetic
    /// identifiers so the condition can be parsed as a Python expression.
    /// Returns the masked string plus (placeholder, original) pairs used to
    /// restore the original spelling in the translated condition and any
    /// captured commands.
    pub(in crate::transpiler) fn mask_condition_operand_tokens(
        &self,
        condition: &str,
    ) -> (String, Vec<(String, String)>) {
        let bytes = condition.as_bytes();
        let mut masked = String::with_capacity(condition.len());
        let mut operand_tokens: Vec<(String, String)> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if ch == '"' || ch == '\'' {
                let quote = bytes[i];
                let start = i;
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    i += 1;
                    if c == b'\\' && i < bytes.len() {
                        i += 1;
                    } else if c == quote {
                        break;
                    }
                }
                masked.push_str(&condition[start..i]);
                continue;
            }

            let token_start = i;
            if ch == '@' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'[' {
                    let Some(end) = Self::selector_bracket_end(condition, i) else {
                        // Unbalanced selector brackets; keep the rest verbatim.
                        masked.push_str(&condition[token_start..]);
                        break;
                    };
                    i = end;
                }
            } else if ch == '{' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j > i + 1 && j < bytes.len() && bytes[j] == b'}' {
                    i = j + 1;
                } else {
                    masked.push(ch);
                    i += 1;
                    continue;
                }
            } else {
                let ch = condition[i..]
                    .chars()
                    .next()
                    .expect("a remaining character");
                masked.push(ch);
                i += ch.len_utf8();
                continue;
            }

            let original = condition[token_start..i].to_string();
            if original.starts_with('{') {
                let name = &original[1..original.len() - 1];
                let word_char = |c: char| {
                    c.is_alphanumeric()
                        || matches!(c, '_' | '.' | ':' | '~' | '^' | '$' | '}' | '{')
                };
                let standalone = !condition[..token_start]
                    .chars()
                    .next_back()
                    .is_some_and(word_char)
                    && !condition[i..]
                        .chars()
                        .next()
                        .is_some_and(|c| c.is_alphanumeric() || matches!(c, '_' | ':' | '{'));
                if standalone
                    && (self.current_context.is_param(name)
                        || self.symbols.compile_time_constants.contains_key(name))
                {
                    masked.push_str(name);
                    continue;
                }
            }
            let placeholder = format!("__cobble_cond_operand_{}__", operand_tokens.len());
            masked.push_str(&placeholder);
            operand_tokens.push((placeholder, original));
        }
        (masked, operand_tokens)
    }

    /// Byte index just past the `]` closing the selector bracket group that
    /// opens at `open`, honoring quoted strings inside the brackets.
    pub(in crate::transpiler) fn selector_bracket_end(
        condition: &str,
        open: usize,
    ) -> Option<usize> {
        let bytes = condition.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'"' | b'\'' => {
                    let quote = bytes[i];
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return None;
                    }
                }
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Map the AST translator's inline always-false output
    /// (`score X O matches 0 unless score X O matches 0`, produced for
    /// comparisons like `x > i32::MAX`) onto `TranslatedCondition::AlwaysFalse`,
    /// including inside OR alternatives. This lets the execute processor keep
    /// its simplifications (dropping impossible branches, `unless`-elision)
    /// for raw execute conditions.
    pub(in crate::transpiler) fn fold_always_false_conditions(
        translated: TranslatedCondition,
    ) -> TranslatedCondition {
        match translated {
            TranslatedCondition::Simple(text) if Self::is_inline_always_false_condition(&text) => {
                TranslatedCondition::AlwaysFalse
            }
            TranslatedCondition::AnyOf(left, right) => TranslatedCondition::AnyOf(
                Box::new(Self::fold_always_false_conditions(*left)),
                Box::new(Self::fold_always_false_conditions(*right)),
            ),
            other => other,
        }
    }

    pub(in crate::transpiler) fn is_inline_always_false_condition(condition: &str) -> bool {
        let Some(rest) = condition.strip_prefix("score ") else {
            return false;
        };
        let Some((head, tail)) = rest.split_once(" matches 0 unless score ") else {
            return false;
        };
        tail.strip_suffix(" matches 0") == Some(head)
    }

    pub(in crate::transpiler) fn parse_raw_condition_expression(
        condition: &str,
    ) -> Option<Expression> {
        let source = format!(
            "def __cobble_condition__():\n    if {}:\n        pass\n",
            condition
        );
        let program = crate::parser::parse(&source).ok()?;
        let Statement::FunctionDef(function) = program.statements.first()? else {
            return None;
        };
        let Statement::If(if_stmt) = function.body.first()? else {
            return None;
        };
        (!matches!(if_stmt.condition, Expression::MinecraftCondition(_)))
            .then(|| if_stmt.condition.clone())
    }

    pub(in crate::transpiler) fn translate_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<TranslatedCondition, TranspileError> {
        let translator = ConditionTranslator {
            symbols: &self.symbols,
        };
        let translated = translator.translate(condition)?;

        // Literal comparisons, boolean literals, constants, and negated
        // always-false conditions fold to the internal `#true_const` /
        // `#false_const` scores; register their objective whenever the
        // translated condition reads them.
        let mut reads_constants = false;
        translated.for_each_simple(&mut |text| {
            reads_constants |= text.contains("#true_const ") || text.contains("#false_const ");
        });
        if reads_constants {
            self.data_pack.track_boolean_constants();
        }
        Ok(translated)
    }

    /// Lower a translated condition to final execute-condition text at this
    /// emission point, materializing OR trees into `or_temp_N` flag variables
    /// in the current function.
    pub(in crate::transpiler) fn lower_translated_condition(
        &mut self,
        condition: TranslatedCondition,
    ) -> Result<String, TranspileError> {
        match condition {
            TranslatedCondition::Simple(text) => Ok(text),
            TranslatedCondition::AnyOf(..) => self.materialize_or_condition(condition),
            TranslatedCondition::AllOf(left, right) => {
                // Lower each side (materializing OR sides first), then chain
                // the results with "if ... if ...".
                let left = self.lower_translated_condition(*left)?;
                let right = self.lower_translated_condition(*right)?;
                Ok(format!(
                    "{} {}",
                    Self::condition_execute_args(&left),
                    Self::condition_execute_args(&right)
                ))
            }
            TranslatedCondition::AlwaysFalse => {
                self.data_pack.track_objective("temp");
                Ok(format!(
                    "score {holder} temp matches 0 unless score {holder} temp matches 0",
                    holder = "#cobble_always_false"
                ))
            }
        }
    }

    /// Lower an OR tree to a fresh flag variable: reset the flag, set it to 1
    /// under each alternative, and return the condition that reads it back.
    pub(in crate::transpiler) fn materialize_or_condition(
        &mut self,
        condition: TranslatedCondition,
    ) -> Result<String, TranspileError> {
        let mut leaves = Vec::new();
        self.flatten_or_leaves(condition, &mut leaves)?;

        // Use unique variable name to prevent race conditions with multiple entities
        let or_var = format!("or_temp_{}", self.next_temp_id());

        // Track the temp objective for or result
        self.data_pack.track_objective("temp");

        // Generate commands for OR logic
        if let Some(ref mut commands) = self.current_function {
            // Initialize or variable to 0
            commands.push(format!("scoreboard players set {} temp 0", or_var));

            // For each condition, if true, set or variable to 1
            for leaf in &leaves {
                let TranslatedCondition::Simple(cond) = leaf else {
                    continue;
                };
                commands.push(format!(
                    "execute {} run scoreboard players set {} temp 1",
                    Self::condition_execute_args(cond),
                    or_var
                ));
            }
        }

        // Return the condition that checks if or variable is 1
        Ok(format!("score {} temp matches 1", or_var))
    }

    /// Flatten an OR tree into its alternatives, in source order. Every
    /// returned leaf is `Simple` or `AlwaysFalse`: structured leaves (an AND
    /// or NOT around a nested OR) are lowered to a `Simple` condition first.
    pub(in crate::transpiler) fn flatten_or_leaves(
        &mut self,
        condition: TranslatedCondition,
        leaves: &mut Vec<TranslatedCondition>,
    ) -> Result<(), TranspileError> {
        match condition {
            TranslatedCondition::AnyOf(left, right) => {
                self.flatten_or_leaves(*left, leaves)?;
                self.flatten_or_leaves(*right, leaves)?;
            }
            leaf @ (TranslatedCondition::Simple(_) | TranslatedCondition::AlwaysFalse) => {
                leaves.push(leaf);
            }
            structured => {
                let rendered = self.lower_translated_condition(structured)?;
                leaves.push(TranslatedCondition::Simple(rendered));
            }
        }
        Ok(())
    }

    /// Check if a condition string looks like a Python expression
    pub(in crate::transpiler) fn looks_like_python_expression(&self, condition: &str) -> bool {
        // The raw-condition whitelist must be checked BEFORE the Python
        // heuristics below: raw conditions may legitimately contain
        // parentheses or operator-like text (selector `name="Bob (Admin)"`,
        // SNBT strings), which would otherwise misroute them into Python
        // translation.
        if let Some(is_raw) = raw_minecraft_condition_verdict(condition) {
            return !is_raw;
        }

        // Check for Python expression indicators
        let python_indicators = [
            " >= ", " <= ", " == ", " != ", " > ", " < ", " and ", " or ", " not ",
        ];
        for indicator in &python_indicators {
            if condition.contains(indicator) {
                return true;
            }
        }

        if condition.starts_with("not ") || condition.contains('(') {
            return true;
        }

        // Check if it's a simple variable name (also a Python expression)
        if condition.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Check if it's a known variable
            return self.symbols.variables.contains_key(condition)
                || self.symbols.scoreboard_variables.contains(condition);
        }

        false
    }
}
