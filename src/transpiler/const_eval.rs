use crate::ast::{BinaryOp, Expression};
use crate::transpiler::Transpiler;

/// Exact integer value of a compile-time number, if it has no fractional part.
pub(in crate::transpiler) fn exact_integer(value: f64) -> Option<i64> {
    (value.is_finite() && value.fract() == 0.0 && value.abs() < 9.0e15).then_some(value as i64)
}

/// `Mth.floorDiv`: integer division rounding toward negative infinity.
pub(in crate::transpiler) fn floor_div_i64(left: i64, right: i64) -> i64 {
    let quotient = left / right;
    if left % right != 0 && ((left < 0) != (right < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

/// Compile-time `/` with Minecraft semantics. Integer operands use
/// `Mth.floorDiv`, exactly like scoreboard `/=` at runtime (`-7 / 2` is -4,
/// `1 / 3 * 3` is 0). Fractional constants can never exist at runtime, so
/// they keep real division and are truncated where the result is used.
/// Returns `None` for a zero divisor.
pub(in crate::transpiler) fn const_div(left: f64, right: f64) -> Option<f64> {
    if right == 0.0 {
        return None;
    }
    match (exact_integer(left), exact_integer(right)) {
        (Some(left), Some(right)) => Some(floor_div_i64(left, right) as f64),
        _ => Some(left / right),
    }
}

/// Compile-time `%` with Minecraft semantics: `Mth.positiveModulo`, a floor
/// modulus whose sign follows the divisor, like scoreboard `%=` at runtime
/// (`-7 % 3` is 2). Fractional operands use the same floor-modulus rule.
/// Returns `None` for a zero divisor.
pub(in crate::transpiler) fn const_mod(left: f64, right: f64) -> Option<f64> {
    if right == 0.0 {
        return None;
    }
    match (exact_integer(left), exact_integer(right)) {
        (Some(left), Some(right)) => Some((left - floor_div_i64(left, right) * right) as f64),
        _ => Some(left - (left / right).floor() * right),
    }
}

/// A compile-time number that a score can hold exactly.
pub(in crate::transpiler) fn score_integer(value: f64) -> Option<i32> {
    (value.fract() == 0.0 && value >= f64::from(i32::MIN) && value <= f64::from(i32::MAX))
        .then_some(value as i32)
}

/// `Mth.floorDiv` on scores. `i32::MIN / -1` wraps to `i32::MIN`, like Java.
pub(in crate::transpiler) fn floor_div_i32(left: i32, right: i32) -> i32 {
    let quotient = left.wrapping_div(right);
    if left.wrapping_rem(right) != 0 && ((left < 0) != (right < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

/// Folds one arithmetic step on two scores exactly as `scoreboard players
/// operation` computes it: 32-bit wrapping, floor division, floor modulus,
/// and a power that multiplies step by step. `None` means the step can't be
/// folded (a zero divisor or a negative exponent).
pub(in crate::transpiler) fn fold_score_operation(
    op: &BinaryOp,
    left: i32,
    right: i32,
) -> Option<i32> {
    match op {
        BinaryOp::Add => Some(left.wrapping_add(right)),
        BinaryOp::Sub => Some(left.wrapping_sub(right)),
        BinaryOp::Mul => Some(left.wrapping_mul(right)),
        BinaryOp::Div | BinaryOp::FloorDiv => (right != 0).then(|| floor_div_i32(left, right)),
        BinaryOp::Mod => {
            (right != 0).then(|| left.wrapping_sub(floor_div_i32(left, right).wrapping_mul(right)))
        }
        BinaryOp::Pow => u32::try_from(right).ok().map(|exp| left.wrapping_pow(exp)),
        _ => None,
    }
}

impl Transpiler {
    /// Infer the type of an expression
    /// Evaluate a constant expression to a number if possible
    pub(in crate::transpiler) fn try_eval_const(&self, expr: &Expression) -> Option<f64> {
        match expr {
            Expression::Number(n) => Some(*n),
            Expression::Identifier(name) => self.symbols.compile_time_constants.get(name).copied(),
            Expression::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Expression::Binary(left, op, right) => {
                let left_val = self.try_eval_const(left)?;
                let right_val = self.try_eval_const(right)?;

                use crate::ast::BinaryOp;
                // Integer operands fold with the scoreboard's own arithmetic,
                // so a folded expression and the same expression computed
                // from variables agree, overflow included.
                if matches!(
                    op,
                    BinaryOp::Add
                        | BinaryOp::Sub
                        | BinaryOp::Mul
                        | BinaryOp::Div
                        | BinaryOp::FloorDiv
                        | BinaryOp::Mod
                        | BinaryOp::Pow
                ) && !(self.uses_storage_numbers() && matches!(op, BinaryOp::Div))
                {
                    if let (Some(left), Some(right)) =
                        (score_integer(left_val), score_integer(right_val))
                    {
                        return fold_score_operation(op, left, right).map(f64::from);
                    }
                }
                match op {
                    BinaryOp::Add => Some(left_val + right_val),
                    BinaryOp::Sub => Some(left_val - right_val),
                    BinaryOp::Mul => Some(left_val * right_val),
                    BinaryOp::Div if self.uses_storage_numbers() => {
                        (right_val != 0.0).then_some(left_val / right_val)
                    }
                    BinaryOp::Div => const_div(left_val, right_val),
                    BinaryOp::FloorDiv => {
                        (right_val != 0.0).then_some((left_val / right_val).floor())
                    }
                    BinaryOp::Mod => const_mod(left_val, right_val),
                    BinaryOp::Pow => {
                        let base = left_val as i32;
                        let exp = right_val as i32;
                        if exp < 0 {
                            None
                        } else {
                            // Real arithmetic, like `*` on these values. A score
                            // that stores the result warns when it can't hold it.
                            Some(f64::from(base).powi(exp))
                        }
                    }
                    BinaryOp::Eq => Some((left_val == right_val) as i32 as f64),
                    BinaryOp::NotEq => Some((left_val != right_val) as i32 as f64),
                    BinaryOp::Lt => Some((left_val < right_val) as i32 as f64),
                    BinaryOp::LtEq => Some((left_val <= right_val) as i32 as f64),
                    BinaryOp::Gt => Some((left_val > right_val) as i32 as f64),
                    BinaryOp::GtEq => Some((left_val >= right_val) as i32 as f64),
                    BinaryOp::In | BinaryOp::NotIn => None,
                    BinaryOp::And => Some(((left_val != 0.0) && (right_val != 0.0)) as i32 as f64),
                    BinaryOp::Or => Some(((left_val != 0.0) || (right_val != 0.0)) as i32 as f64),
                }
            }
            _ => None,
        }
    }
}
