use crate::ast::{Expression, Statement};
use std::collections::HashSet;

/// Context for tracking function parameters and scope
#[derive(Clone)]
pub(in crate::transpiler) struct FunctionContext {
    pub(in crate::transpiler) params: Vec<String>,
    /// Parameters the function assigns somewhere in its body.
    pub(in crate::transpiler) reassigned_params: Vec<String>,
}

#[derive(Clone)]
pub(in crate::transpiler) struct ReturnContext {
    pub(in crate::transpiler) value_score: String,
    pub(in crate::transpiler) flag_score: String,
}

impl FunctionContext {
    pub(in crate::transpiler) fn new() -> Self {
        Self {
            params: Vec::new(),
            reassigned_params: Vec::new(),
        }
    }

    pub(in crate::transpiler) fn with_params(params: Vec<String>, body: &[Statement]) -> Self {
        let mut assigned = HashSet::new();
        Self::collect_assigned_names(body, &mut assigned);
        let reassigned_params = params
            .iter()
            .filter(|param| assigned.contains(*param))
            .cloned()
            .collect();
        Self {
            params,
            reassigned_params,
        }
    }

    fn collect_assigned_names(body: &[Statement], names: &mut HashSet<String>) {
        for statement in body {
            match statement {
                Statement::Assignment(assign)
                    if assign.target_expr.as_ref().is_none_or(
                        |target| matches!(target, Expression::Identifier(name) if name == &assign.target),
                    ) =>
                {
                    names.insert(assign.target.clone());
                }
                Statement::If(if_stmt) => {
                    Self::collect_assigned_names(&if_stmt.then_branch, names);
                    for (_, branch) in &if_stmt.elif_branches {
                        Self::collect_assigned_names(branch, names);
                    }
                    if let Some(branch) = &if_stmt.else_branch {
                        Self::collect_assigned_names(branch, names);
                    }
                }
                Statement::For(for_loop) => Self::collect_assigned_names(&for_loop.body, names),
                Statement::While(while_loop) => {
                    Self::collect_assigned_names(&while_loop.body, names)
                }
                Statement::Match(match_stmt) => {
                    for case in &match_stmt.cases {
                        Self::collect_assigned_names(&case.body, names);
                    }
                }
                Statement::Execute(exec_block) => {
                    Self::collect_assigned_names(&exec_block.body, names)
                }
                _ => {}
            }
        }
    }

    pub(in crate::transpiler) fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p == name)
    }
}
