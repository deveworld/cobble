use crate::ast::{Expression, FunctionDef, Program, Statement};
use std::collections::{BTreeSet, HashMap, HashSet};

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
    /// How often the whole function body reads each name it uses as a
    /// `for` target. A compile-time `for` whose target is read outside the
    /// loop keeps the target in a real variable.
    pub(in crate::transpiler) for_target_reads: HashMap<String, usize>,
    /// The `for` targets the function reads outside every loop over that
    /// target, as after the loop. A runtime loop keeps such a target's last
    /// value after it.
    pub(in crate::transpiler) for_targets_read_after: HashSet<String>,
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
            for_target_reads: HashMap::new(),
            for_targets_read_after: HashSet::new(),
        }
    }

    pub(in crate::transpiler) fn with_params(params: Vec<String>, body: &[Statement]) -> Self {
        let mut assigned = BTreeSet::new();
        let mut globals = HashSet::new();
        collect_bindings(body, &mut assigned, &mut globals);
        // A parameter whose items or fields the body changes holds a new
        // value too: a later macro line must read it from `args`.
        let mut changed = BTreeSet::new();
        collect_changed_containers(body, &mut changed);
        let reassigned_params = params
            .iter()
            .filter(|param| assigned.contains(*param) || changed.contains(*param))
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
            for_target_reads: HashMap::new(),
            for_targets_read_after: HashSet::new(),
        };
        collect_score_writes(body, &mut context.score_writes);
        let mut targets = BTreeSet::new();
        collect_for_targets(body, &mut targets);
        context.for_targets_read_after = targets
            .iter()
            .filter(|target| {
                let mut loops = Vec::new();
                collect_target_loops(body, target, &mut loops);
                let in_loops: usize = loops
                    .into_iter()
                    .map(|for_loop| count_reads(std::slice::from_ref(for_loop), target))
                    .sum();
                count_reads(body, target) > in_loops
            })
            .cloned()
            .collect();
        context.for_target_reads = targets
            .into_iter()
            .map(|target| {
                let reads = count_reads(body, &target);
                (target, reads)
            })
            .collect();
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
                Statement::MultiAssign(multi) => {
                    for value in &multi.values {
                        self.check_expression(value, assigned).map_err(at)?;
                    }
                    for target in &multi.targets {
                        match target {
                            Expression::Identifier(name) => {
                                assigned.insert(name.clone());
                            }
                            target => self.check_expression(target, assigned).map_err(at)?,
                        }
                    }
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
                    assigned.extend(for_loop.index.iter().cloned());
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
            Statement::MultiAssign(multi) => {
                assigned.extend(multi.targets.iter().filter_map(|target| match target {
                    Expression::Identifier(name) => Some(name.clone()),
                    _ => None,
                }));
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
                assigned.extend(for_loop.index.iter().cloned());
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

/// List methods that change the list they are called on.
const CHANGING_METHODS: [&str; 8] = [
    "append", "extend", "insert", "pop", "remove", "sort", "reverse", "clear",
];

/// Names whose items or fields `body` changes: as the target of an
/// assignment such as `t[0] = 9` or `cfg.x += 1`, or with a list method
/// such as `t.append(1)`. Nested function definitions are not searched.
fn collect_changed_containers(body: &[Statement], changed: &mut BTreeSet<String>) {
    fn root(expr: &Expression) -> Option<&str> {
        match expr {
            Expression::Identifier(name) => Some(name),
            Expression::Subscript(base, _)
            | Expression::Attribute(base, _)
            | Expression::Slice(base, ..) => root(base),
            _ => None,
        }
    }
    fn method_calls(expr: &Expression, changed: &mut BTreeSet<String>) {
        if let Expression::Call(func, _) = expr {
            if let Expression::Attribute(base, method) = &**func {
                if CHANGING_METHODS.contains(&method.as_str()) {
                    changed.extend(root(base).map(str::to_string));
                }
            }
        }
        for part in crate::transpiler::desugar::sub_expressions(expr) {
            method_calls(part, changed);
        }
    }
    for statement in body {
        match statement {
            Statement::Assignment(assign) if !assign.assigns_name() => {
                changed.extend(
                    assign
                        .target_expr
                        .as_ref()
                        .and_then(root)
                        .map(str::to_string),
                );
            }
            Statement::MultiAssign(multi) => {
                changed.extend(
                    multi
                        .targets
                        .iter()
                        .filter(|target| !matches!(target, Expression::Identifier(_)))
                        .filter_map(root)
                        .map(str::to_string),
                );
            }
            _ => {}
        }
        for expr in crate::transpiler::statement_expressions(statement) {
            method_calls(expr, changed);
        }
        match statement {
            Statement::If(if_stmt) => {
                collect_changed_containers(&if_stmt.then_branch, changed);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_changed_containers(branch, changed);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_changed_containers(branch, changed);
                }
            }
            Statement::For(for_loop) => collect_changed_containers(&for_loop.body, changed),
            Statement::While(while_loop) => collect_changed_containers(&while_loop.body, changed),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_changed_containers(&case.body, changed);
                }
            }
            Statement::Execute(exec_block) => collect_changed_containers(&exec_block.body, changed),
            _ => {}
        }
    }
}

/// How often `body` reads `name`: in expressions, and as a `{name}`
/// placeholder in raw commands and execute headers. Assigning a name, or
/// using it as a `for` target, is not a read. Nested function definitions
/// are not searched.
pub(in crate::transpiler) fn count_reads(body: &[Statement], name: &str) -> usize {
    let in_expression = |expr: &Expression| {
        let mut reads = Vec::new();
        expression_reads(expr, &mut reads);
        reads.into_iter().filter(|read| *read == name).count()
    };
    let placeholder = format!("{{{name}}}");
    let in_text = |text: &str| text.matches(placeholder.as_str()).count();
    let mut count = 0;
    for statement in body {
        count += match statement {
            Statement::Assignment(assign) => {
                let target = assign
                    .target_expr
                    .as_ref()
                    .filter(|_| !assign.assigns_name())
                    .map_or(0, in_expression);
                target + in_expression(&assign.value)
            }
            Statement::MultiAssign(multi) => {
                multi.values.iter().map(in_expression).sum::<usize>()
                    + multi
                        .targets
                        .iter()
                        .filter(|target| !matches!(target, Expression::Identifier(_)))
                        .map(in_expression)
                        .sum::<usize>()
            }
            Statement::Expression(expr) | Statement::Return(Some(expr)) => in_expression(expr),
            Statement::MinecraftCommand(command) => in_text(command),
            Statement::If(if_stmt) => {
                in_expression(&if_stmt.condition)
                    + count_reads(&if_stmt.then_branch, name)
                    + if_stmt
                        .elif_branches
                        .iter()
                        .map(|(condition, branch)| {
                            in_expression(condition) + count_reads(branch, name)
                        })
                        .sum::<usize>()
                    + if_stmt
                        .else_branch
                        .as_ref()
                        .map_or(0, |branch| count_reads(branch, name))
            }
            Statement::For(for_loop) => {
                in_expression(&for_loop.iter)
                    + for_loop.step.as_ref().map_or(0, in_expression)
                    + count_reads(&for_loop.body, name)
            }
            Statement::While(while_loop) => {
                in_expression(&while_loop.condition) + count_reads(&while_loop.body, name)
            }
            Statement::Match(match_stmt) => {
                in_expression(&match_stmt.value)
                    + match_stmt
                        .cases
                        .iter()
                        .map(|case| count_reads(&case.body, name))
                        .sum::<usize>()
            }
            Statement::Execute(exec_block) => {
                in_text(&format!("{:?}", exec_block.modifiers))
                    + count_reads(&exec_block.body, name)
            }
            _ => 0,
        };
    }
    count
}

/// The targets of every `for` loop in `body`. Nested function definitions
/// are not searched.
fn collect_for_targets(body: &[Statement], targets: &mut BTreeSet<String>) {
    for statement in body {
        match statement {
            Statement::For(for_loop) => {
                targets.insert(for_loop.target.clone());
                targets.extend(for_loop.index.iter().cloned());
                collect_for_targets(&for_loop.body, targets);
            }
            Statement::If(if_stmt) => {
                collect_for_targets(&if_stmt.then_branch, targets);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_for_targets(branch, targets);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_for_targets(branch, targets);
                }
            }
            Statement::While(while_loop) => collect_for_targets(&while_loop.body, targets),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_for_targets(&case.body, targets);
                }
            }
            Statement::Execute(exec_block) => collect_for_targets(&exec_block.body, targets),
            _ => {}
        }
    }
}

/// The outermost `for` loops over `target` in `body`. Nested function
/// definitions are not searched.
fn collect_target_loops<'a>(body: &'a [Statement], target: &str, loops: &mut Vec<&'a Statement>) {
    for statement in body {
        match statement {
            Statement::For(for_loop) if for_loop.target == target => loops.push(statement),
            Statement::For(for_loop) => collect_target_loops(&for_loop.body, target, loops),
            Statement::If(if_stmt) => {
                collect_target_loops(&if_stmt.then_branch, target, loops);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_target_loops(branch, target, loops);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_target_loops(branch, target, loops);
                }
            }
            Statement::While(while_loop) => collect_target_loops(&while_loop.body, target, loops),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_target_loops(&case.body, target, loops);
                }
            }
            Statement::Execute(exec_block) => collect_target_loops(&exec_block.body, target, loops),
            _ => {}
        }
    }
}

/// Whether `body` assigns the name `name`, or uses it as a `for` target.
/// Nested function definitions are not searched.
pub(in crate::transpiler) fn assigns_name(body: &[Statement], name: &str) -> bool {
    let mut assigned = BTreeSet::new();
    collect_bindings(body, &mut assigned, &mut HashSet::new());
    assigned.contains(name)
}

/// Module variables that `function` declares `global` and assigns a literal
/// (a number, boolean, string, list, or map), each with its first such
/// value. Nested function definitions are included, each with its own
/// `global` names.
pub(in crate::transpiler) fn global_literal_assignments(
    function: &FunctionDef,
) -> Vec<(String, Expression)> {
    fn walk(body: &[Statement], globals: &HashSet<String>, out: &mut Vec<(String, Expression)>) {
        for statement in body {
            match statement {
                Statement::Assignment(assign)
                    if assign.assigns_name()
                        && globals.contains(&assign.target)
                        && matches!(
                            assign.value,
                            Expression::Number(_)
                                | Expression::Boolean(_)
                                | Expression::String(_)
                                | Expression::Array(_)
                                | Expression::Map(_)
                        ) =>
                {
                    if !out.iter().any(|(name, _)| name == &assign.target) {
                        out.push((assign.target.clone(), assign.value.clone()));
                    }
                }
                Statement::FunctionDef(nested) => {
                    out.extend(global_literal_assignments(nested));
                }
                Statement::If(if_stmt) => {
                    walk(&if_stmt.then_branch, globals, out);
                    for (_, branch) in &if_stmt.elif_branches {
                        walk(branch, globals, out);
                    }
                    if let Some(branch) = &if_stmt.else_branch {
                        walk(branch, globals, out);
                    }
                }
                Statement::For(for_loop) => walk(&for_loop.body, globals, out),
                Statement::While(while_loop) => walk(&while_loop.body, globals, out),
                Statement::Match(match_stmt) => {
                    for case in &match_stmt.cases {
                        walk(&case.body, globals, out);
                    }
                }
                Statement::Execute(exec_block) => walk(&exec_block.body, globals, out),
                _ => {}
            }
        }
    }
    let mut assigned = Vec::new();
    let mut globals = HashSet::new();
    collect_bindings(&function.body, &mut assigned, &mut globals);
    let mut out = Vec::new();
    walk(&function.body, &globals, &mut out);
    out
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
pub(in crate::transpiler) fn expression_reads<'a>(expr: &'a Expression, reads: &mut Vec<&'a str>) {
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
        Expression::Slice(obj, start, stop) => {
            expression_reads(obj, reads);
            for bound in [start, stop].into_iter().flatten() {
                expression_reads(bound, reads);
            }
        }
        Expression::Conditional(condition, value, otherwise) => {
            expression_reads(condition, reads);
            expression_reads(value, reads);
            expression_reads(otherwise, reads);
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

/// Parameters of `function` that its body reads in an expression, such as
/// `v + 1` or `other(v)`, rather than only in command text as `{v}`.
pub(in crate::transpiler) fn params_read_in_expressions(function: &FunctionDef) -> HashSet<String> {
    let mut expressions = Vec::new();
    collect_statement_expressions(&function.body, &mut expressions);
    let mut reads = Vec::new();
    for expr in expressions {
        expression_reads(expr, &mut reads);
    }
    function
        .params
        .iter()
        .filter(|param| reads.contains(&param.name.as_str()))
        .map(|param| param.name.clone())
        .collect()
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
            Statement::MultiAssign(multi) => {
                out.extend(&multi.values);
                out.extend(&multi.targets);
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
        Expression::Slice(obj, start, stop) => {
            expression_calls(obj, calls);
            for bound in [start, stop].into_iter().flatten() {
                expression_calls(bound, calls);
            }
        }
        Expression::Conditional(condition, value, otherwise) => {
            expression_calls(condition, calls);
            expression_calls(value, calls);
            expression_calls(otherwise, calls);
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
