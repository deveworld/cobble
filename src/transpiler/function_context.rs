use crate::ast::{Expression, FunctionDef, Program, Statement};
use std::collections::{BTreeSet, HashSet};

/// Context for tracking function parameters and scope
#[derive(Clone)]
pub(in crate::transpiler) struct FunctionContext {
    pub(in crate::transpiler) params: Vec<String>,
    /// Parameters the function assigns somewhere in its body.
    pub(in crate::transpiler) reassigned_params: Vec<String>,
    /// The function's local variables: names it assigns, or uses as a `for`
    /// target, without declaring them `global`. Parameters are not in this
    /// list; they live in the call's arguments.
    pub(in crate::transpiler) locals: Vec<String>,
    /// Names the function declares `global`.
    pub(in crate::transpiler) globals: Vec<String>,
    /// Where the function reads a local variable before assigning it.
    /// Lowering that statement fails, so the error points at it.
    pub(in crate::transpiler) unbound_read: Option<UnboundRead>,
    /// Names whose score a raw command or a `store ... score` header in the
    /// function writes. Such a name holds a number.
    pub(in crate::transpiler) score_writes: HashSet<String>,
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
            locals: Vec::new(),
            globals: Vec::new(),
            unbound_read: None,
            score_writes: HashSet::new(),
        }
    }

    pub(in crate::transpiler) fn with_params(params: Vec<String>, body: &[Statement]) -> Self {
        let mut assigned = BTreeSet::new();
        let mut globals = HashSet::new();
        collect_bindings(body, &mut assigned, &mut globals);
        let reassigned_params = params
            .iter()
            .filter(|param| assigned.contains(*param))
            .cloned()
            .collect();
        let locals = assigned
            .into_iter()
            .filter(|name| !globals.contains(name) && !params.contains(name))
            .collect();
        let mut globals: Vec<String> = globals.into_iter().collect();
        globals.sort();
        let mut context = Self {
            params,
            reassigned_params,
            locals,
            globals,
            unbound_read: None,
            score_writes: HashSet::new(),
        };
        collect_score_writes(body, &mut context.score_writes);
        context.unbound_read =
            context
                .first_read_before_assignment(body)
                .map(|(statement, name)| UnboundRead {
                    statement: statement_address(statement),
                    top_level: body
                        .iter()
                        .find(|top| contains_statement(top, statement))
                        .map_or(0, statement_address),
                    name,
                });
        context
    }

    pub(in crate::transpiler) fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p == name)
    }

    pub(in crate::transpiler) fn is_local(&self, name: &str) -> bool {
        self.locals.iter().any(|local| local == name)
    }

    /// The first statement of `body` that reads a local variable before any
    /// assignment to it, in source order, with that variable. Python raises
    /// `UnboundLocalError` there. Inside a loop body, every assignment in
    /// that body counts as earlier, since a later iteration sees it.
    pub(in crate::transpiler) fn first_read_before_assignment<'a>(
        &self,
        body: &'a [Statement],
    ) -> Option<(&'a Statement, String)> {
        let mut assigned = HashSet::new();
        self.check_reads(body, &mut assigned).err()
    }

    fn check_reads<'a>(
        &self,
        body: &'a [Statement],
        assigned: &mut HashSet<String>,
    ) -> Result<(), (&'a Statement, String)> {
        for statement in body {
            let at = |name: String| (statement, name);
            match statement {
                Statement::Assignment(assign) => {
                    if let Some(target) = assign
                        .target_expr
                        .as_ref()
                        .filter(|_| !assign.assigns_name())
                    {
                        self.check_expression(target, assigned).map_err(at)?;
                    }
                    self.check_expression(&assign.value, assigned).map_err(at)?;
                    assigned.insert(assign.target.clone());
                }
                Statement::Expression(expr) => self.check_expression(expr, assigned).map_err(at)?,
                Statement::Return(Some(expr)) => {
                    self.check_expression(expr, assigned).map_err(at)?
                }
                Statement::MinecraftCommand(command) => {
                    self.check_text(command, assigned).map_err(at)?
                }
                Statement::If(if_stmt) => {
                    self.check_expression(&if_stmt.condition, assigned)
                        .map_err(at)?;
                    let mut after = assigned.clone();
                    let mut branch = assigned.clone();
                    self.check_reads(&if_stmt.then_branch, &mut branch)?;
                    after.extend(branch);
                    for (condition, elif_branch) in &if_stmt.elif_branches {
                        self.check_expression(condition, assigned).map_err(at)?;
                        let mut branch = assigned.clone();
                        self.check_reads(elif_branch, &mut branch)?;
                        after.extend(branch);
                    }
                    if let Some(else_branch) = &if_stmt.else_branch {
                        let mut branch = assigned.clone();
                        self.check_reads(else_branch, &mut branch)?;
                        after.extend(branch);
                    }
                    *assigned = after;
                }
                Statement::For(for_loop) => {
                    self.check_expression(&for_loop.iter, assigned)
                        .map_err(at)?;
                    if let Some(step) = &for_loop.step {
                        self.check_expression(step, assigned).map_err(at)?;
                    }
                    assigned.insert(for_loop.target.clone());
                    self.check_first_iteration(&for_loop.body, assigned)?;
                    collect_bindings(&for_loop.body, assigned, &mut HashSet::new());
                    self.check_reads(&for_loop.body, assigned)?;
                }
                Statement::While(while_loop) => {
                    self.check_expression(&while_loop.condition, assigned)
                        .map_err(at)?;
                    self.check_first_iteration(&while_loop.body, assigned)?;
                    collect_bindings(&while_loop.body, assigned, &mut HashSet::new());
                    self.check_reads(&while_loop.body, assigned)?;
                }
                Statement::Match(match_stmt) => {
                    self.check_expression(&match_stmt.value, assigned)
                        .map_err(at)?;
                    let mut after = assigned.clone();
                    for case in &match_stmt.cases {
                        let mut branch = assigned.clone();
                        self.check_reads(&case.body, &mut branch)?;
                        after.extend(branch);
                    }
                    *assigned = after;
                }
                Statement::Execute(exec_block) => {
                    for modifier in &exec_block.modifiers {
                        self.check_text(&format!("{:?}", modifier), assigned)
                            .map_err(at)?;
                    }
                    self.check_reads(&exec_block.body, assigned)?;
                }
                _ => {}
            }
        }
        Ok(())
    }

    /// Reads that the first pass through a loop body always reaches: those
    /// outside any block nested in the body. A later iteration can't have
    /// assigned the name first, because it would have passed the same read.
    fn check_first_iteration<'a>(
        &self,
        body: &'a [Statement],
        assigned: &HashSet<String>,
    ) -> Result<(), (&'a Statement, String)> {
        let mut assigned = assigned.clone();
        for statement in body {
            let at = |name: String| (statement, name);
            let header = match statement {
                Statement::If(if_stmt) => Some(&if_stmt.condition),
                Statement::For(for_loop) => Some(&for_loop.iter),
                Statement::While(while_loop) => Some(&while_loop.condition),
                Statement::Match(match_stmt) => Some(&match_stmt.value),
                Statement::Execute(_) => None,
                _ => {
                    self.check_reads(std::slice::from_ref(statement), &mut assigned)?;
                    continue;
                }
            };
            if let Some(header) = header {
                self.check_expression(header, &assigned).map_err(at)?;
            }
            collect_bindings(
                std::slice::from_ref(statement),
                &mut assigned,
                &mut HashSet::new(),
            );
        }
        Ok(())
    }

    fn check_expression(
        &self,
        expr: &Expression,
        assigned: &HashSet<String>,
    ) -> Result<(), String> {
        let mut reads = Vec::new();
        expression_reads(expr, &mut reads);
        match reads
            .into_iter()
            .find(|name| self.is_local(name) && !assigned.contains(*name))
        {
            Some(name) => Err(name.to_string()),
            None => Ok(()),
        }
    }

    /// `{name}` placeholders in raw command and header text.
    fn check_text(&self, text: &str, assigned: &HashSet<String>) -> Result<(), String> {
        for local in &self.locals {
            if !assigned.contains(local) && text.contains(&format!("{{{}}}", local)) {
                return Err(local.clone());
            }
        }
        Ok(())
    }
}

/// Names `body` assigns or uses as a `for` target, and names it declares
/// `global`. Nested function definitions have their own scope.
fn collect_bindings(
    body: &[Statement],
    assigned: &mut impl Extend<String>,
    globals: &mut HashSet<String>,
) {
    for statement in body {
        match statement {
            Statement::Assignment(assign) if assign.assigns_name() => {
                assigned.extend([assign.target.clone()]);
            }
            Statement::Global(names) => globals.extend(names.iter().cloned()),
            Statement::If(if_stmt) => {
                collect_bindings(&if_stmt.then_branch, assigned, globals);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_bindings(branch, assigned, globals);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_bindings(branch, assigned, globals);
                }
            }
            Statement::For(for_loop) => {
                assigned.extend([for_loop.target.clone()]);
                collect_bindings(&for_loop.body, assigned, globals);
            }
            Statement::While(while_loop) => collect_bindings(&while_loop.body, assigned, globals),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_bindings(&case.body, assigned, globals);
                }
            }
            Statement::Execute(exec_block) => collect_bindings(&exec_block.body, assigned, globals),
            _ => {}
        }
    }
}

/// Names whose score a raw command or a `store ... score` header in `body`
/// writes. Nested function definitions have their own scope.
fn collect_score_writes(body: &[Statement], writes: &mut HashSet<String>) {
    let objective = crate::transpiler::vars_objective::SOURCE_OBJECTIVE;
    let add = |writes: &mut HashSet<String>, command: &str| {
        writes.extend(
            crate::transpiler::score_holders::written_holders(command, objective)
                .into_iter()
                .map(str::to_string),
        );
    };
    for statement in body {
        match statement {
            Statement::MinecraftCommand(command) => {
                add(writes, command.strip_prefix('/').unwrap_or(command));
            }
            Statement::Execute(block) => {
                for modifier in &block.modifiers {
                    if let crate::ast::ExecuteModifier::Store(store) = modifier {
                        add(writes, &format!("execute store {store} run say store"));
                    }
                }
                collect_score_writes(&block.body, writes);
            }
            Statement::If(if_stmt) => {
                collect_score_writes(&if_stmt.then_branch, writes);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_score_writes(branch, writes);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_score_writes(branch, writes);
                }
            }
            Statement::For(for_loop) => collect_score_writes(&for_loop.body, writes),
            Statement::While(while_loop) => collect_score_writes(&while_loop.body, writes),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_score_writes(&case.body, writes);
                }
            }
            _ => {}
        }
    }
}

/// The name `statement` itself assigns or uses as a `for` target, if any.
/// Statements inside it don't count.
pub(in crate::transpiler) fn direct_binding(statement: &Statement) -> Option<&str> {
    match statement {
        Statement::Assignment(assign) if assign.assigns_name() => Some(&assign.target),
        Statement::For(for_loop) => Some(&for_loop.target),
        _ => None,
    }
}

/// Variable names an expression reads, in source order. Called function
/// names and attribute names are not reads.
fn expression_reads<'a>(expr: &'a Expression, reads: &mut Vec<&'a str>) {
    match expr {
        Expression::Identifier(name) => reads.push(name),
        Expression::Binary(left, _, right) => {
            expression_reads(left, reads);
            expression_reads(right, reads);
        }
        Expression::Unary(_, inner) => expression_reads(inner, reads),
        Expression::Call(func, args) => {
            if let Expression::Attribute(obj, _) = &**func {
                expression_reads(obj, reads);
            }
            for arg in args {
                expression_reads(arg, reads);
            }
        }
        Expression::Attribute(obj, _) => expression_reads(obj, reads),
        Expression::Subscript(obj, index) => {
            expression_reads(obj, reads);
            expression_reads(index, reads);
        }
        Expression::Array(items) => {
            for item in items {
                expression_reads(item, reads);
            }
        }
        Expression::Map(entries) => {
            for (_, value) in entries {
                expression_reads(value, reads);
            }
        }
        Expression::Number(_)
        | Expression::String(_)
        | Expression::Boolean(_)
        | Expression::None => {}
    }
}

/// Names of the functions `body` calls by name, including calls in nested
/// blocks. Nested function definitions are not searched.
pub(in crate::transpiler) fn called_functions(body: &[Statement]) -> BTreeSet<String> {
    let mut calls = BTreeSet::new();
    let mut expressions = Vec::new();
    collect_statement_expressions(body, &mut expressions);
    for expr in expressions {
        expression_calls(expr, &mut calls);
    }
    calls
}

fn collect_statement_expressions<'a>(body: &'a [Statement], out: &mut Vec<&'a Expression>) {
    for statement in body {
        match statement {
            Statement::Assignment(assign) => {
                out.push(&assign.value);
                if let Some(target) = &assign.target_expr {
                    out.push(target);
                }
            }
            Statement::Expression(expr) | Statement::Return(Some(expr)) => out.push(expr),
            Statement::If(if_stmt) => {
                out.push(&if_stmt.condition);
                collect_statement_expressions(&if_stmt.then_branch, out);
                for (condition, branch) in &if_stmt.elif_branches {
                    out.push(condition);
                    collect_statement_expressions(branch, out);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_statement_expressions(branch, out);
                }
            }
            Statement::For(for_loop) => {
                out.push(&for_loop.iter);
                if let Some(step) = &for_loop.step {
                    out.push(step);
                }
                collect_statement_expressions(&for_loop.body, out);
            }
            Statement::While(while_loop) => {
                out.push(&while_loop.condition);
                collect_statement_expressions(&while_loop.body, out);
            }
            Statement::Match(match_stmt) => {
                out.push(&match_stmt.value);
                for case in &match_stmt.cases {
                    collect_statement_expressions(&case.body, out);
                }
            }
            Statement::Execute(exec_block) => collect_statement_expressions(&exec_block.body, out),
            _ => {}
        }
    }
}

fn expression_calls(expr: &Expression, calls: &mut BTreeSet<String>) {
    match expr {
        Expression::Call(func, args) => {
            if let Expression::Identifier(name) = &**func {
                calls.insert(name.clone());
            }
            expression_calls(func, calls);
            for arg in args {
                expression_calls(arg, calls);
            }
        }
        Expression::Binary(left, _, right) => {
            expression_calls(left, calls);
            expression_calls(right, calls);
        }
        Expression::Unary(_, inner) | Expression::Attribute(inner, _) => {
            expression_calls(inner, calls)
        }
        Expression::Subscript(obj, index) => {
            expression_calls(obj, calls);
            expression_calls(index, calls);
        }
        Expression::Array(items) => {
            for item in items {
                expression_calls(item, calls);
            }
        }
        Expression::Map(entries) => {
            for (_, value) in entries {
                expression_calls(value, calls);
            }
        }
        _ => {}
    }
}

/// A read of a local variable before any assignment to it.
#[derive(Debug, Clone)]
pub(in crate::transpiler) struct UnboundRead {
    /// The address of the reading statement.
    pub(in crate::transpiler) statement: usize,
    /// The address of the function body statement that contains it. A loop
    /// lowers a copy of its body, so the error falls back to the loop.
    pub(in crate::transpiler) top_level: usize,
    pub(in crate::transpiler) name: String,
}

/// Identifies a statement of the function being lowered.
pub(in crate::transpiler) fn statement_address(statement: &Statement) -> usize {
    statement as *const Statement as usize
}

fn contains_statement(outer: &Statement, inner: &Statement) -> bool {
    if std::ptr::eq(outer, inner) {
        return true;
    }
    let nested: Vec<&[Statement]> = match outer {
        Statement::If(if_stmt) => {
            let mut blocks = vec![if_stmt.then_branch.as_slice()];
            blocks.extend(
                if_stmt
                    .elif_branches
                    .iter()
                    .map(|(_, body)| body.as_slice()),
            );
            blocks.extend(if_stmt.else_branch.as_deref());
            blocks
        }
        Statement::For(for_loop) => vec![for_loop.body.as_slice()],
        Statement::While(while_loop) => vec![while_loop.body.as_slice()],
        Statement::Match(match_stmt) => match_stmt
            .cases
            .iter()
            .map(|case| case.body.as_slice())
            .collect(),
        Statement::Execute(exec_block) => vec![exec_block.body.as_slice()],
        _ => Vec::new(),
    };
    nested
        .into_iter()
        .flatten()
        .any(|statement| contains_statement(statement, inner))
}

/// The error for reading a local variable before it is assigned.
pub(in crate::transpiler) fn unbound_local_message(name: &str, function: &str) -> String {
    format!(
        "`{name}` is read before it is assigned in `{function}`. `{function}` assigns `{name}`, so it is a local variable there; add `global {name}` to use the module variable instead."
    )
}

/// The names local to `function`: those it assigns or uses as a `for`
/// target without declaring them `global`, other than its parameters.
pub(crate) fn function_locals(function: &FunctionDef) -> Vec<String> {
    let params = function
        .params
        .iter()
        .map(|param| param.name.clone())
        .collect();
    FunctionContext::with_params(params, &function.body).locals
}

/// For each top-level function of `program`, the module variables it assigns
/// without declaring them `global`, as (function, variable). Those names are
/// the function's own variables, so the module variable keeps its value.
/// `cobble migrate` lists them for code written before 0.11.
pub fn module_variables_made_local(program: &Program) -> Vec<(String, String)> {
    let module_variables: HashSet<&str> = program
        .statements
        .iter()
        .filter_map(|statement| match statement {
            Statement::Assignment(assign) if assign.assigns_name() => Some(assign.target.as_str()),
            _ => None,
        })
        .collect();
    let mut shadowed = Vec::new();
    for statement in &program.statements {
        if let Statement::FunctionDef(func) = statement {
            let params = func.params.iter().map(|param| param.name.clone()).collect();
            let context = FunctionContext::with_params(params, &func.body);
            for local in &context.locals {
                if module_variables.contains(local.as_str()) {
                    shadowed.push((func.name.clone(), local.clone()));
                }
            }
        }
    }
    shadowed
}
