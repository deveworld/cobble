use crate::ast::{BinaryOp, Expression, Statement, UnaryOp};
use crate::transpiler::condition_translator::{ConditionTranslator, TranslatedCondition};
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::{FunctionCapture, TranspileError, Transpiler};

/// Execute-condition keywords that start a raw Minecraft condition
/// (`entity @a[...]`, `score x temp matches 5`, `data storage ...`).
pub(in crate::transpiler) const RAW_CONDITION_KEYWORDS: [&str; 11] = [
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
];

/// `Some(true)` for a raw Minecraft condition, `Some(false)` for a Python
/// expression that merely starts with a keyword-named variable
/// (`score >= 10`), and `None` when the condition starts with no keyword.
/// Shared with the diagnostics layer so both agree on which `if` headers
/// carry raw syntax rather than variable references.
pub(crate) fn raw_minecraft_condition_verdict(condition: &str) -> Option<bool> {
    let rest = RAW_CONDITION_KEYWORDS
        .iter()
        .find_map(|keyword| condition.strip_prefix(keyword))?;
    let next_token = rest.split_whitespace().next().unwrap_or("");
    Some(!matches!(
        next_token,
        "==" | "!=" | ">=" | "<=" | ">" | "<" | "and" | "or" | "not" | "+" | "-" | "*" | "/" | "%"
    ))
}

impl Transpiler {
    pub(in crate::transpiler) fn preprocess_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<Expression, TranspileError> {
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
                        BinaryOp::And | BinaryOp::Or
                            if ExpressionEvaluator::calls_user_code(right) =>
                        {
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
                            self.lower_condition_value_to_temp(condition)
                        }
                    }
                }
            }
            Expression::Unary(UnaryOp::Not, expr) => {
                let new_expr = self.preprocess_condition(expr)?;
                Ok(Expression::Unary(UnaryOp::Not, Box::new(new_expr)))
            }
            // `-x` in condition position is a number, true when it is not 0.
            Expression::Unary(..) => self.lower_condition_value_to_temp(condition),
            // A number literal is true when it is not 0, like a variable.
            Expression::Number(value) => {
                self.data_pack.track_boolean_constants();
                Ok(Expression::Boolean(*value as i32 != 0))
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
        let (masked, operand_tokens) = Self::mask_condition_operand_tokens(condition);
        let Some(expr) = Self::parse_raw_condition_expression(&masked) else {
            return Ok(None);
        };

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
                masked.push(ch);
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    masked.push(c as char);
                    i += 1;
                    if c == b'\\' && i < bytes.len() {
                        masked.push(bytes[i] as char);
                        i += 1;
                    } else if c == quote {
                        break;
                    }
                }
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
                masked.push(ch);
                i += 1;
                continue;
            }

            let original = condition[token_start..i].to_string();
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
        Some(if_stmt.condition.clone())
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
