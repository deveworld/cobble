use crate::ast::{BinaryOp, CobbleType, Expression, UnaryOp};
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
    /// because at least one side contains an OR.
    AllOf(Box<TranslatedCondition>, Box<TranslatedCondition>),
    /// Logical OR of two alternatives; lowered to a flag variable at the
    /// emission site.
    AnyOf(Box<TranslatedCondition>, Box<TranslatedCondition>),
    /// Logical negation of a structured (OR-carrying) condition. Simple
    /// conditions are negated inline by the translator and never appear here.
    Negated(Box<TranslatedCondition>),
    /// A condition that can never succeed (e.g. `x > i32::MAX`), folded from
    /// the translator's inline always-false spelling for raw execute
    /// conditions.
    AlwaysFalse,
}

impl TranslatedCondition {
    /// True for shapes that need deferred lowering and therefore cannot be
    /// joined into a single `Simple` chain.
    fn is_structured(&self) -> bool {
        matches!(
            self,
            Self::AllOf(..) | Self::AnyOf(..) | Self::Negated(..) | Self::AlwaysFalse
        )
    }

    /// Visit every `Simple` fragment, left to right.
    pub fn for_each_simple(&self, visit: &mut impl FnMut(&str)) {
        match self {
            Self::Simple(text) => visit(text),
            Self::AllOf(left, right) | Self::AnyOf(left, right) => {
                left.for_each_simple(visit);
                right.for_each_simple(visit);
            }
            Self::Negated(inner) => inner.for_each_simple(visit),
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
            Self::Negated(inner) => inner.map_simple(rewrite),
            Self::AlwaysFalse => {}
        }
    }
}

/// Translates Python-like conditions to Minecraft execute conditions
pub struct ConditionTranslator<'a> {
    pub variable_objectives: &'a HashMap<String, String>,
    pub compile_time_constants: &'a HashMap<String, f64>,
    pub variable_storage_paths: &'a HashMap<String, String>,
    pub variable_types: &'a HashMap<String, CobbleType>,
}

impl<'a> ConditionTranslator<'a> {
    pub fn new(
        variable_objectives: &'a HashMap<String, String>,
        compile_time_constants: &'a HashMap<String, f64>,
        variable_storage_paths: &'a HashMap<String, String>,
        variable_types: &'a HashMap<String, CobbleType>,
    ) -> Self {
        Self {
            variable_objectives,
            compile_time_constants,
            variable_storage_paths,
            variable_types,
        }
    }

    pub fn translate(&self, condition: &Expression) -> Result<TranslatedCondition, TranspileError> {
        // Translate Python-like conditions to Minecraft execute conditions
        match condition {
            Expression::Binary(left, op, right) => {
                match op {
                    BinaryOp::And => {
                        let left_cond = self.translate(left)?;
                        let right_cond = self.translate(right)?;

                        // If either side contains an OR the AND cannot be
                        // rendered as one "if ... if ..." chain here; the
                        // emission site lowers the OR side(s) first.
                        if left_cond.is_structured() || right_cond.is_structured() {
                            return Ok(TranslatedCondition::AllOf(
                                Box::new(left_cond),
                                Box::new(right_cond),
                            ));
                        }

                        let (
                            TranslatedCondition::Simple(left_cond),
                            TranslatedCondition::Simple(right_cond),
                        ) = (left_cond, right_cond)
                        else {
                            unreachable!("non-OR conditions are always Simple");
                        };

                        // And: chain the conditions with "if ... if ..."
                        Ok(TranslatedCondition::Simple(format!(
                            "{} {}",
                            Transpiler::condition_execute_args(&left_cond),
                            Transpiler::condition_execute_args(&right_cond)
                        )))
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
                self.reject_storage_backed_operand(condition)?;

                // Simple boolean check: treat as "var > 0"
                let objective = self
                    .variable_objectives
                    .get(var)
                    .unwrap_or(&"temp".to_string())
                    .clone();
                Ok(TranslatedCondition::Simple(format!(
                    "score {} {} matches 1..",
                    var, objective
                )))
            }
            Expression::Unary(op, expr) => {
                match op {
                    UnaryOp::Not => {
                        let inner_cond = self.translate(expr)?;
                        match inner_cond {
                            TranslatedCondition::Simple(inner) => Ok(TranslatedCondition::Simple(
                                Self::negate_simple_condition(inner),
                            )),
                            // Double negation cancels out.
                            TranslatedCondition::Negated(inner) => Ok(*inner),
                            // NOT around an OR-carrying condition is negated
                            // at the emission site, after the OR is lowered.
                            structured => Ok(TranslatedCondition::Negated(Box::new(structured))),
                        }
                    }
                    _ => Err("Unsupported unary operator in condition".into()),
                }
            }
            Expression::Boolean(b) => {
                // Boolean literals - use a condition that always evaluates correctly
                // regardless of execution context
                if *b {
                    // True: use a condition that always succeeds
                    Ok(TranslatedCondition::Simple(
                        "score #true_const __internal__ matches 1".to_string(),
                    ))
                } else {
                    // False: use a condition that always fails
                    Ok(TranslatedCondition::Simple(
                        "score #false_const __internal__ matches 1".to_string(),
                    ))
                }
            }
            _ => Err(format!(
                "Unsupported condition expression. Conditions must be:\n\
                 - Comparisons (x > y, x == 5, etc.)\n\
                 - Boolean variables (x)\n\
                 - Boolean literals (True, False)\n\
                 - Logical operators (and, or, not)\n\
                 Got: {:?}",
                condition
            )
            .into()),
        }
    }

    /// Negate a simple (marker-free) condition inline by flipping its
    /// if/unless spelling.
    fn negate_simple_condition(inner_cond: String) -> String {
        if inner_cond.starts_with("unless ") {
            // Double negative: "unless" becomes "if"
            inner_cond.replace("unless ", "if ")
        } else if inner_cond.starts_with("score ")
            || inner_cond.starts_with("entity ")
            || inner_cond.starts_with("block ")
        {
            // Add "unless" prefix
            format!("unless {}", inner_cond)
        } else {
            // Already has "if", convert to "unless"
            inner_cond.replace("if ", "unless ")
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
            (Some((var_name, objective)), _, _, Some(value)) => {
                self.score_vs_literal_condition(&var_name, &objective, op, value)
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

    fn extract_literal(&self, expr: &Expression) -> Option<i32> {
        match expr {
            Expression::Number(n) => Some(*n as i32),
            Expression::Boolean(b) => Some(if *b { 1 } else { 0 }),
            Expression::Identifier(name) => self
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32),
            _ => None,
        }
    }

    fn extract_scoreboard(&self, expr: &Expression) -> Option<(String, String)> {
        if let Expression::Identifier(name) = expr {
            if self.compile_time_constants.contains_key(name) {
                None
            } else {
                let objective = self
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
                self.variable_storage_paths,
                self.variable_types,
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

    fn score_vs_literal_condition(
        &self,
        var_name: &str,
        objective: &str,
        op: &BinaryOp,
        value: i32,
    ) -> Result<String, TranspileError> {
        use BinaryOp::*;

        Ok(match op {
            Eq => format!("score {} {} matches {}", var_name, objective, value),
            NotEq => format!("unless score {} {} matches {}", var_name, objective, value),
            Gt => {
                // Handle boundary case: x > i32::MAX is always false
                if value == i32::MAX {
                    // Use a condition that's always false: check if score matches a value AND doesn't match it
                    return Ok(format!(
                        "score {} {} matches 0 unless score {} {} matches 0",
                        var_name, objective, var_name, objective
                    ));
                }
                format!(
                    "score {} {} matches {}..",
                    var_name,
                    objective,
                    value + 1 // Safe because we checked value != i32::MAX
                )
            }
            GtEq => format!("score {} {} matches {}..", var_name, objective, value),
            Lt => {
                // Handle boundary case: x < i32::MIN is always false
                if value == i32::MIN {
                    // Use a condition that's always false
                    return Ok(format!(
                        "score {} {} matches 0 unless score {} {} matches 0",
                        var_name, objective, var_name, objective
                    ));
                }
                format!(
                    "score {} {} matches ..{}",
                    var_name,
                    objective,
                    value - 1 // Safe because we checked value != i32::MIN
                )
            }
            LtEq => format!("score {} {} matches ..{}", var_name, objective, value),
            _ => {
                return Err("Unsupported operator for literal comparison"
                    .to_string()
                    .into());
            }
        })
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
            Ok("score #true_const __internal__ matches 1".to_string())
        } else {
            Ok("score #false_const __internal__ matches 1".to_string())
        }
    }
}
