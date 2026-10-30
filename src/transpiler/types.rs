use crate::ast::{Expression, Statement};
use crate::transpiler::{TranspileError, Transpiler};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum FunctionReturnKind {
    None,
    Integer,
    NonInteger,
}

impl Transpiler {
    pub(in crate::transpiler) fn infer_type(&self, expr: &Expression) -> crate::ast::CobbleType {
        use crate::ast::CobbleType;

        match expr {
            Expression::Number(_) => CobbleType::Integer,
            Expression::Boolean(_) => CobbleType::Boolean,
            Expression::String(_) => CobbleType::String,
            Expression::Array(_) => CobbleType::List,
            Expression::Map(_) => CobbleType::Map,
            Expression::Identifier(name) => {
                // Look up variable type
                self.symbols
                    .variable_types
                    .get(name)
                    .cloned()
                    .unwrap_or(CobbleType::Unknown)
            }
            Expression::Binary(left, op, right) => {
                use crate::ast::BinaryOp;

                let _left_type = self.infer_type(left);
                let _right_type = self.infer_type(right);

                match op {
                    // Arithmetic operations return integers
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow => CobbleType::Integer,

                    // Comparison operations return booleans
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq => CobbleType::Boolean,

                    // Logical operations return booleans
                    BinaryOp::And | BinaryOp::Or => CobbleType::Boolean,
                }
            }
            Expression::Unary(op, _) => {
                use crate::ast::UnaryOp;

                match op {
                    UnaryOp::Not => CobbleType::Boolean,
                    UnaryOp::Neg | UnaryOp::Pos => CobbleType::Integer,
                }
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if name == "len") => {
                CobbleType::Integer
            }
            Expression::Call(func, _)
                if self
                    .resolve_user_function_call_name(func)
                    .is_some_and(|name| {
                        matches!(
                            self.functions.returns.get(name),
                            Some(FunctionReturnKind::Integer)
                        )
                    }) =>
            {
                CobbleType::Integer
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Attribute(obj, _) if matches!(&**obj, Expression::Identifier(module) if module == "math")) => {
                CobbleType::Integer
            }
            Expression::Call(_, _) => CobbleType::Unknown,
            // A character of a string is a string.
            Expression::Subscript(base, _)
                if matches!(&**base, Expression::String(_))
                    || matches!(&**base, Expression::Identifier(name)
                        if self.symbols.variable_types.get(name) == Some(&CobbleType::String)) =>
            {
                CobbleType::String
            }
            _ => CobbleType::Unknown,
        }
    }

    pub(in crate::transpiler) fn analyze_function_return_kind(
        body: &[Statement],
    ) -> FunctionReturnKind {
        body.iter()
            .fold(FunctionReturnKind::None, |acc, statement| {
                Self::merge_return_kind(acc, Self::statement_return_kind(statement))
            })
    }

    pub(in crate::transpiler) fn statement_return_kind(
        statement: &Statement,
    ) -> FunctionReturnKind {
        match statement {
            Statement::Return(expr) => expr
                .as_ref()
                .map(Self::expression_return_kind)
                .unwrap_or(FunctionReturnKind::Integer),
            Statement::If(if_stmt) => {
                let mut kind = Self::analyze_function_return_kind(&if_stmt.then_branch);
                for (_, branch) in &if_stmt.elif_branches {
                    kind =
                        Self::merge_return_kind(kind, Self::analyze_function_return_kind(branch));
                }
                if let Some(branch) = &if_stmt.else_branch {
                    kind =
                        Self::merge_return_kind(kind, Self::analyze_function_return_kind(branch));
                }
                kind
            }
            Statement::For(for_loop) => Self::analyze_function_return_kind(&for_loop.body),
            Statement::While(while_loop) => Self::analyze_function_return_kind(&while_loop.body),
            Statement::Match(match_stmt) => {
                match_stmt
                    .cases
                    .iter()
                    .fold(FunctionReturnKind::None, |acc, case| {
                        Self::merge_return_kind(acc, Self::analyze_function_return_kind(&case.body))
                    })
            }
            Statement::Execute(exec_block) => Self::analyze_function_return_kind(&exec_block.body),
            _ => FunctionReturnKind::None,
        }
    }

    pub(in crate::transpiler) fn expression_return_kind(expr: &Expression) -> FunctionReturnKind {
        match expr {
            Expression::String(_)
            | Expression::Array(_)
            | Expression::Map(_)
            | Expression::None => FunctionReturnKind::NonInteger,
            _ => FunctionReturnKind::Integer,
        }
    }

    pub(in crate::transpiler) fn merge_return_kind(
        left: FunctionReturnKind,
        right: FunctionReturnKind,
    ) -> FunctionReturnKind {
        match (left, right) {
            (FunctionReturnKind::NonInteger, _) | (_, FunctionReturnKind::NonInteger) => {
                FunctionReturnKind::NonInteger
            }
            (FunctionReturnKind::Integer, _) | (_, FunctionReturnKind::Integer) => {
                FunctionReturnKind::Integer
            }
            _ => FunctionReturnKind::None,
        }
    }

    /// Check if a type assignment is valid
    pub(in crate::transpiler) fn check_type_assignment(
        &self,
        var_name: &str,
        new_type: &crate::ast::CobbleType,
    ) -> Result<(), TranspileError> {
        use crate::ast::CobbleType;

        if let Some(existing_type) = self.symbols.variable_types.get(var_name) {
            if existing_type != new_type
                && *existing_type != CobbleType::Unknown
                && *new_type != CobbleType::Unknown
            {
                return Err(format!(
                    "Type mismatch for variable '{}'.\n\n\
                    Variable was previously defined as type: {}\n\
                    Cannot reassign to type: {}\n\n\
                    In Cobble, all variables have immutable types.\n\
                    Once a variable is assigned a value, its type cannot change.\n\n\
                    Solutions:\n\
                    1. Use a different variable name for the different type\n\
                    2. Ensure all assignments to '{}' use the same type",
                    var_name,
                    existing_type.name(),
                    new_type.name(),
                    var_name
                )
                .into());
            }
        }

        Ok(())
    }
}
