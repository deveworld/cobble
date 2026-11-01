use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{TranspileError, Transpiler};
use std::collections::HashMap;

/// Structured result of condition translation.
///
/// Simple conditions (comparisons, boolean checks, AND chains of those) are
/// rendered to execute-condition text immediately. Conditions that need flag
/// variables (`or`, and `and`/`not` around an `or`) cannot be rendered here:
/// each consumer lowers them at its own emission point, so the structure is
/// carried as data instead of being smuggled through marker strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TranslatedCondition {
    /// Directly renderable execute-condition text: a bare condition
    /// ("score x temp matches 1..") or an if/unless-prefixed chain
    /// ("if score x temp matches 1 unless score y temp matches 2").
    Simple(String),
    /// Logical AND whose sides could not be joined into one `Simple` chain
    /// because at least one side contains an OR. `not` never survives
    /// translation: it is pushed down to single clauses with De Morgan's laws,
    /// so negation is always expressed through `Simple`, `AllOf`, and `AnyOf`.
    AllOf(Box<TranslatedCondition>, Box<TranslatedCondition>),
    /// Logical OR of two alternatives; lowered to a flag variable at the
    /// emission site.
    AnyOf(Box<TranslatedCondition>, Box<TranslatedCondition>),
    /// A condition that can never succeed (e.g. `x > i32::MAX`), folded from
    /// the translator's inline always-false spelling for raw execute
    /// conditions.
    AlwaysFalse,
}

impl TranslatedCondition {
    /// Visit every `Simple` fragment, left to right.
    pub fn for_each_simple(&self, visit: &mut impl FnMut(&str)) {
        match self {
            Self::Simple(text) => visit(text),
            Self::AllOf(left, right) | Self::AnyOf(left, right) => {
                left.for_each_simple(visit);
                right.for_each_simple(visit);
            }
            Self::AlwaysFalse => {}
        }
    }

    /// Rewrite every `Simple` fragment in place.
    pub fn map_simple(&mut self, rewrite: &impl Fn(&str) -> String) {
        match self {
            Self::Simple(text) => *text = rewrite(text),
            Self::AllOf(left, right) | Self::AnyOf(left, right) => {
                left.map_simple(rewrite);
                right.map_simple(rewrite);
            }
            Self::AlwaysFalse => {}
        }
    }
}

/// Translates Python-like conditions to Minecraft execute conditions
pub struct ConditionTranslator<'a> {
    pub(in crate::transpiler) symbols: &'a Symbols,
}

impl<'a> ConditionTranslator<'a> {
    pub fn translate(&self, condition: &Expression) -> Result<TranslatedCondition, TranspileError> {
        // Translate Python-like conditions to Minecraft execute conditions
        match condition {
            Expression::Binary(left, op, right) => {
                match op {
                    BinaryOp::And => {
                        Ok(Self::conjoin(self.translate(left)?, self.translate(right)?))
                    }
                    BinaryOp::Or => {
                        // Or: requires setting a flag variable at the
                        // emission site; carry both alternatives as data.
                        Ok(TranslatedCondition::AnyOf(
                            Box::new(self.translate(left)?),
                            Box::new(self.translate(right)?),
                        ))
                    }
                    // Comparison operators
                    _ => self
                        .translate_comparison(left, op, right)
                        .map(TranslatedCondition::Simple),
                }
            }
            Expression::Identifier(var) => {
                // A Minecraft condition masked out of a mixed condition is
                // its own execute clause, so every command that tests it
                // (a short-circuit flag, a helper) tests the real condition.
                if let Some(minecraft) = self.symbols.minecraft_conditions.get(var) {
                    return Ok(TranslatedCondition::Simple(format!("if {minecraft}")));
                }
                self.reject_storage_backed_operand(condition)?;

                // A compile-time constant has no score; fold its truthiness
                // with the same "value != 0" rule a score variable uses.
                if let Some(value) = self.symbols.compile_time_constants.get(var) {
                    return Ok(TranslatedCondition::Simple(Self::constant_condition(
                        (*value as i32) != 0,
                    )));
                }

                // A bare value is true when it is not 0, like Python, so
                // negative numbers count as true.
                let objective = self
                    .symbols
                    .variable_objectives
                    .get(var)
                    .unwrap_or(&"temp".to_string())
                    .clone();
                Ok(TranslatedCondition::Simple(format!(
                    "unless score {} {} matches 0",
                    var, objective
                )))
            }
            Expression::Unary(op, expr) => match op {
                UnaryOp::Not => Ok(Self::negate(self.translate(expr)?)),
                _ => Err("Unsupported unary operator in condition".into()),
            },
            // Boolean literals use internal constants so they evaluate the
            // same regardless of execution context.
            Expression::Boolean(b) => Ok(TranslatedCondition::Simple(Self::constant_condition(*b))),
            _ => Err(format!(
                "Unsupported condition expression. Conditions must be:\n\
                 - Comparisons (x > y, x == 5, etc.)\n\
                 - Boolean variables (x)\n\
                 - Boolean literals (True, False)\n\
                 - Logical operators (and, or, not)\n\
                 Got: `{}`",
                Transpiler::expression_source_key(condition)
            )
            .into()),
        }
    }

    /// Condition text that always (or never) passes, backed by the internal
    /// `#true_const`/`#false_const` scores. Callers that emit it must make
    /// sure the boolean constants are registered.
    fn constant_condition(value: bool) -> String {
        if value {
            "score #true_const temp matches 1".to_string()
        } else {
            "score #false_const temp matches 1".to_string()
        }
    }

    /// Logical AND of two translated conditions: one "if ... if ..." chain
    /// when both sides are simple, otherwise a structured `AllOf` that the
    /// emission site lowers after materializing the OR side(s).
    fn conjoin(left: TranslatedCondition, right: TranslatedCondition) -> TranslatedCondition {
        match (left, right) {
            (TranslatedCondition::Simple(left), TranslatedCondition::Simple(right)) => {
                TranslatedCondition::Simple(format!(
                    "{} {}",
                    Transpiler::condition_execute_args(&left),
                    Transpiler::condition_execute_args(&right)
                ))
            }
            (left, right) => TranslatedCondition::AllOf(Box::new(left), Box::new(right)),
        }
    }

    /// Logical NOT, pushed down to single clauses with De Morgan's laws. A
    /// simple chain "if A if B" means A and B, so its negation is the OR of
    /// the negated clauses, never the chain with every prefix flipped (which
    /// would mean "not A and not B").
    fn negate(condition: TranslatedCondition) -> TranslatedCondition {
        match condition {
            TranslatedCondition::Simple(text) => {
                let mut negated = condition_clauses(&text)
                    .into_iter()
                    .map(|clause| TranslatedCondition::Simple(negate_clause(&clause)));
                let first = negated
                    .next()
                    .expect("translated conditions have at least one clause");
                negated.fold(first, |alternatives, next| {
                    TranslatedCondition::AnyOf(Box::new(alternatives), Box::new(next))
                })
            }
            TranslatedCondition::AnyOf(left, right) => {
                Self::conjoin(Self::negate(*left), Self::negate(*right))
            }
            TranslatedCondition::AllOf(left, right) => TranslatedCondition::AnyOf(
                Box::new(Self::negate(*left)),
                Box::new(Self::negate(*right)),
            ),
            // The translator never produces AlwaysFalse (it is folded from raw
            // execute conditions after translation); negating it always passes.
            TranslatedCondition::AlwaysFalse => {
                TranslatedCondition::Simple(Self::constant_condition(true))
            }
        }
    }

    fn translate_comparison(
        &self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
    ) -> Result<String, TranspileError> {
        self.reject_storage_backed_operand(left)?;
        self.reject_storage_backed_operand(right)?;

        let left_score = self.extract_scoreboard(left);
        let right_score = self.extract_scoreboard(right);
        let left_literal = self.extract_literal(left);
        let right_literal = self.extract_literal(right);

        match (left_score, right_score, left_literal, right_literal) {
            (Some((var_name, objective)), Some((other_var, other_objective)), _, _) => match op {
                BinaryOp::Eq => Ok(format!(
                    "score {} {} = {} {}",
                    var_name, objective, other_var, other_objective
                )),
                BinaryOp::NotEq => Ok(format!(
                    "unless score {} {} = {} {}",
                    var_name, objective, other_var, other_objective
                )),
                BinaryOp::Gt => Ok(format!(
                    "score {} {} > {} {}",
                    var_name, objective, other_var, other_objective
                )),
                BinaryOp::GtEq => Ok(format!(
                    "score {} {} >= {} {}",
                    var_name, objective, other_var, other_objective
                )),
                BinaryOp::Lt => Ok(format!(
                    "score {} {} < {} {}",
                    var_name, objective, other_var, other_objective
                )),
                BinaryOp::LtEq => Ok(format!(
                    "score {} {} <= {} {}",
                    var_name, objective, other_var, other_objective
                )),
                _ => Err("Unsupported operator for variable comparison"
                    .to_string()
                    .into()),
            },
            (Some((var_name, objective)), _, _, Some(_)) => {
                let value = self.extract_literal_value(right).unwrap_or_default();
                self.score_vs_number_condition(&var_name, &objective, op, value)
            }
            (_, Some((_var_name, _objective)), Some(_), _) => {
                if let Some(reversed) = self.reverse_operator(op) {
                    // Translate with swapped sides
                    self.translate_comparison(right, &reversed, left)
                } else {
                    Err("Unsupported operator for literal comparison"
                        .to_string()
                        .into())
                }
            }
            (None, None, Some(left_value), Some(right_value)) => {
                self.literal_vs_literal_condition(left_value, right_value, op)
            }
            _ => Err("Condition must compare scoreboard values or literals"
                .to_string()
                .into()),
        }
    }

    /// The number a literal or a `const` holds, before a score would clamp
    /// or cut it.
    fn extract_literal_value(&self, expr: &Expression) -> Option<f64> {
        match expr {
            Expression::Number(n) => Some(*n),
            Expression::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Expression::Identifier(name) => self.symbols.compile_time_constants.get(name).copied(),
            _ => None,
        }
    }

    /// A score compared with a number, exactly: a score holds a 32-bit
    /// integer, so `x == 9999999999` or `x == 2.5` is never true, and
    /// `x >= 2.5` is `x >= 3`.
    fn score_vs_number_condition(
        &self,
        var_name: &str,
        objective: &str,
        op: &BinaryOp,
        value: f64,
    ) -> Result<String, TranspileError> {
        use BinaryOp::*;
        let never = format!(
            "score {var_name} {objective} matches 0 unless score {var_name} {objective} matches 0"
        );
        let always = format!("score {var_name} {objective} matches ..{}", i32::MAX);
        let (min, max) = (f64::from(i32::MIN), f64::from(i32::MAX));
        // The scores that pass, as `low..=high`.
        let (low, high) = match op {
            Eq | NotEq => {
                let exact = value.fract() == 0.0 && (min..=max).contains(&value);
                return Ok(match (op, exact) {
                    (Eq, true) => format!("score {var_name} {objective} matches {}", value as i32),
                    (NotEq, true) => {
                        format!(
                            "unless score {var_name} {objective} matches {}",
                            value as i32
                        )
                    }
                    (Eq, false) => never,
                    _ => always,
                });
            }
            Gt => (value.floor() + 1.0, max),
            GtEq => (value.ceil(), max),
            Lt => (min, value.ceil() - 1.0),
            LtEq => (min, value.floor()),
            _ => {
                return Err("Unsupported operator for literal comparison"
                    .to_string()
                    .into());
            }
        };
        if low.is_nan() || high.is_nan() || low > high || low > max || high < min {
            return Ok(never);
        }
        let (low, high) = (low.max(min) as i32, high.min(max) as i32);
        Ok(if low == i32::MIN && high == i32::MAX {
            always
        } else if low == i32::MIN {
            format!("score {var_name} {objective} matches ..{high}")
        } else {
            format!("score {var_name} {objective} matches {low}..")
        })
    }

    fn extract_literal(&self, expr: &Expression) -> Option<i32> {
        match expr {
            Expression::Number(n) => Some(*n as i32),
            Expression::Boolean(b) => Some(if *b { 1 } else { 0 }),
            Expression::Identifier(name) => self
                .symbols
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32),
            _ => None,
        }
    }

    fn extract_scoreboard(&self, expr: &Expression) -> Option<(String, String)> {
        if let Expression::Identifier(name) = expr {
            if self.symbols.compile_time_constants.contains_key(name) {
                None
            } else {
                let objective = self
                    .symbols
                    .variable_objectives
                    .get(name)
                    .cloned()
                    .unwrap_or_else(|| "temp".to_string());
                Some((name.clone(), objective))
            }
        } else {
            None
        }
    }

    fn reject_storage_backed_operand(&self, expr: &Expression) -> Result<(), TranspileError> {
        if let Expression::Identifier(name) = expr {
            if let Some(message) = Self::storage_backed_condition_error(
                &self.symbols.variable_storage_paths,
                &self.symbols.variable_types,
                name,
            ) {
                return Err(message.into());
            }
        }
        Ok(())
    }

    /// A storage-backed variable holding a non-integer value (string, list, map)
    /// has no meaningful scoreboard score, so reading it in a scoreboard
    /// condition would silently test a score that is never written. Integer,
    /// boolean, and unknown-typed variables keep compiling: runtime list-loop
    /// targets are registered in both the scoreboard and storage worlds and are
    /// typed Unknown or Integer.
    pub fn storage_backed_condition_error(
        variable_storage_paths: &HashMap<String, String>,
        variable_types: &HashMap<String, CobbleType>,
        name: &str,
    ) -> Option<String> {
        if !variable_storage_paths.contains_key(name) {
            return None;
        }
        let var_type = variable_types
            .get(name)
            .cloned()
            .unwrap_or(CobbleType::Unknown);
        match var_type {
            CobbleType::Integer | CobbleType::Boolean | CobbleType::Unknown => None,
            CobbleType::String | CobbleType::List | CobbleType::Map => Some(format!(
                "Variable '{}' is storage-backed ({}) and cannot be used in a scoreboard condition; compare integers or use a score variable",
                name,
                var_type.name()
            )),
        }
    }

    fn reverse_operator(&self, op: &BinaryOp) -> Option<BinaryOp> {
        use BinaryOp::*;
        match op {
            Eq => Some(Eq),
            NotEq => Some(NotEq),
            Lt => Some(Gt),
            LtEq => Some(GtEq),
            Gt => Some(Lt),
            GtEq => Some(LtEq),
            _ => None,
        }
    }

    fn literal_vs_literal_condition(
        &self,
        left: i32,
        right: i32,
        op: &BinaryOp,
    ) -> Result<String, TranspileError> {
        use BinaryOp::*;

        let result = match op {
            Eq => left == right,
            NotEq => left != right,
            Lt => left < right,
            LtEq => left <= right,
            Gt => left > right,
            GtEq => left >= right,
            _ => {
                return Err("Unsupported operator for literal comparison"
                    .to_string()
                    .into())
            }
        };

        // Use internal scoreboard constants for compile-time boolean results
        if result {
            Ok("score #true_const temp matches 1".to_string())
        } else {
            Ok("score #false_const temp matches 1".to_string())
        }
    }
}

/// Split rendered execute-condition text into its `if`/`unless`-prefixed
/// clauses. A bare leading condition ("score x temp matches 1..") is read as
/// an `if` clause. Clause boundaries inside `[...]`, `{...}`, `(...)`, and
/// quoted strings are ignored, so selector arguments and NBT never split.
pub(in crate::transpiler) fn condition_clauses(text: &str) -> Vec<String> {
    let text = text.trim();
    let text = if text.starts_with("if ") || text.starts_with("unless ") {
        text.to_string()
    } else {
        format!("if {}", text)
    };

    let mut clauses = Vec::new();
    let mut start = 0;
    let mut depth = 0i32;
    let mut quote: Option<char> = None;
    let mut escaped = false;
    for (index, ch) in text.char_indices() {
        if let Some(active) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '[' | '{' | '(' => depth += 1,
            ']' | '}' | ')' => depth -= 1,
            ' ' if depth == 0 && index > start => {
                let rest = &text[index + 1..];
                if rest.starts_with("if ") || rest.starts_with("unless ") {
                    clauses.push(text[start..index].trim().to_string());
                    start = index + 1;
                }
            }
            _ => {}
        }
    }
    let last = text[start..].trim();
    if !last.is_empty() {
        clauses.push(last.to_string());
    }
    clauses
}

/// Flip one `if`/`unless`-prefixed clause (a bare clause counts as `if`).
pub(in crate::transpiler) fn negate_clause(clause: &str) -> String {
    if let Some(rest) = clause.strip_prefix("if ") {
        format!("unless {}", rest)
    } else if let Some(rest) = clause.strip_prefix("unless ") {
        format!("if {}", rest)
    } else {
        format!("unless {}", clause)
    }
}
