//! Python forms that lower to simpler statements.
//!
//! A statement that uses a conditional expression, a slice, `in`, joining or
//! repeating text and lists, a list method, or tuple assignment becomes
//! statements the rest of the transpiler already handles. Forms that need a
//! loop or a macro call a helper from `prelude.cbl`, which is compiled once
//! per pack and only when used. A value that takes statements to compute goes
//! into a hidden local variable first, so it survives recursion like any
//! other local.

use crate::ast::{
    Assignment, BinaryOp, CobbleType, Expression, ForLoop, FunctionDef, IfStatement, MatchPattern,
    MatchStatement, MultiAssign, Statement, UnaryOp, WhileLoop,
};
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::function_context::called_functions;
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{TranspileError, Transpiler};

const PRELUDE: &str = include_str!("prelude.cbl");

type LoweredValue = Result<Expression, TranspileError>;

/// Where a prelude helper leaves a result that isn't a number. The name is
/// also a variable of that path, so Cobble code can read it.
pub(in crate::transpiler) const RESULT: &str = "_cobble_result";

/// `str` and the list forms of `min`, `max`, and `sum` aren't functions of
/// their own in a pack, but a function with the same name replaces them.
const BUILTIN_FUNCTIONS: [&str; 7] = ["abs", "enumerate", "max", "min", "sorted", "str", "sum"];

/// List methods that change the list where it is.
const LIST_COMMAND_METHODS: [&str; 3] = ["append", "clear", "extend"];
/// List methods that build the new list in a helper.
const LIST_HELPER_METHODS: [&str; 4] = ["insert", "remove", "reverse", "sort"];
/// List methods that give a value.
const LIST_VALUE_METHODS: [&str; 3] = ["count", "index", "pop"];

/// Parts of an expression, for walks that treat them alike.
fn sub_expressions(expr: &Expression) -> Vec<&Expression> {
    match expr {
        Expression::Array(items) => items.iter().collect(),
        Expression::Map(entries) => entries.iter().map(|(_, value)| value).collect(),
        Expression::Attribute(base, _) | Expression::Unary(_, base) => vec![base],
        Expression::Binary(left, _, right) | Expression::Subscript(left, right) => {
            vec![left, right]
        }
        Expression::Call(func, args) => {
            let mut parts = vec![&**func];
            parts.extend(args);
            parts
        }
        Expression::Slice(base, start, stop) => {
            let mut parts = vec![&**base];
            parts.extend([start, stop].into_iter().flatten().map(|bound| &**bound));
            parts
        }
        Expression::Conditional(condition, value, otherwise) => {
            vec![condition, value, otherwise]
        }
        Expression::Number(_)
        | Expression::String(_)
        | Expression::Boolean(_)
        | Expression::None
        | Expression::Identifier(_) => Vec::new(),
    }
}

fn is_arithmetic(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Add
            | BinaryOp::Sub
            | BinaryOp::Mul
            | BinaryOp::Div
            | BinaryOp::Mod
            | BinaryOp::Pow
    )
}

fn is_comparison(op: &BinaryOp) -> bool {
    matches!(
        op,
        BinaryOp::Eq
            | BinaryOp::NotEq
            | BinaryOp::Lt
            | BinaryOp::LtEq
            | BinaryOp::Gt
            | BinaryOp::GtEq
    )
}

/// Whether `expr` combines conditions with `and`, `or`, or `not`.
fn is_logical(expr: &Expression) -> bool {
    matches!(
        expr,
        Expression::Binary(_, BinaryOp::And | BinaryOp::Or, _) | Expression::Unary(UnaryOp::Not, _)
    )
}

/// Whether `expr` tests something instead of computing a number.
/// `a list`, `a number`, and so on, for messages.
fn type_phrase(value_type: &CobbleType) -> &'static str {
    match value_type {
        CobbleType::String => "text",
        CobbleType::List => "a list",
        CobbleType::Map => "a map",
        _ => "a number",
    }
}

fn is_condition(expr: &Expression) -> bool {
    match expr {
        Expression::Binary(_, op, _) => !is_arithmetic(op),
        Expression::Unary(UnaryOp::Not, _) => true,
        _ => false,
    }
}

fn assign(target: &str, value: Expression) -> Statement {
    Statement::Assignment(Assignment {
        target: target.to_string(),
        target_expr: Some(Expression::Identifier(target.to_string())),
        value,
        persist: false,
    })
}

fn assign_to(target: &Expression, value: Expression) -> Statement {
    match target {
        Expression::Identifier(name) => assign(name, value),
        target => Statement::Assignment(Assignment {
            target: assignment_root(target),
            target_expr: Some(target.clone()),
            value,
            persist: false,
        }),
    }
}

/// The variable an assignment target such as `grid[i].cost` writes into.
fn assignment_root(target: &Expression) -> String {
    match target {
        Expression::Identifier(name) => name.clone(),
        Expression::Attribute(base, _) | Expression::Subscript(base, _) => assignment_root(base),
        _ => String::new(),
    }
}

fn call(name: &str, args: Vec<Expression>) -> Expression {
    Expression::Call(Box::new(Expression::Identifier(name.to_string())), args)
}

/// The two lowered sides of a binary form.
fn two(sides: Vec<Expression>) -> (Expression, Expression) {
    let mut sides = sides.into_iter();
    (
        sides.next().unwrap_or(Expression::None),
        sides.next().unwrap_or(Expression::None),
    )
}

fn compare(left: Expression, op: BinaryOp, right: Expression) -> Expression {
    Expression::Binary(Box::new(left), op, Box::new(right))
}

impl Transpiler {
    /// The statements that replace `statement`, when it uses a form this
    /// module lowers.
    pub(in crate::transpiler) fn desugar_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<Option<Vec<Statement>>, TranspileError> {
        let mut out = Vec::new();
        match statement {
            // `items.pop()` on its own is a storage command.
            Statement::Expression(Expression::Call(func, args))
                if matches!(&**func, Expression::Attribute(base, method)
                    if method == "pop" && self.is_list_value(base))
                    && !args.iter().any(|arg| self.needs_lowering(arg)) =>
            {
                return Ok(None)
            }
            Statement::MultiAssign(multi) => self.desugar_multi_assign(multi, &mut out)?,
            // A list or a map written in place would be gone before an item
            // that reads the old value, as in `xs = [xs[1], 0]`.
            Statement::Assignment(assign)
                if matches!(assign.value, Expression::Array(_) | Expression::Map(_))
                    && assign.assigns_name()
                    && ExpressionEvaluator::expression_reads_target(
                        &assign.value,
                        &assign.target,
                    ) =>
            {
                let holder = self.hidden_local(self.value_type(&assign.value));
                out.push(self::assign(&holder, assign.value.clone()));
                out.push(self::assign(&assign.target, Expression::Identifier(holder)));
            }
            Statement::Assignment(assign)
                if self.needs_lowering(&assign.value)
                    || self.is_operand_choice(&assign.value)
                    || assign
                        .target_expr
                        .as_ref()
                        .is_some_and(|target| self.target_needs_lowering(target)) =>
            {
                self.desugar_assignment(assign, &mut out)?
            }
            Statement::Expression(expr) if self.needs_lowering(expr) => {
                self.desugar_expression_statement(expr, &mut out)?
            }
            Statement::Return(Some(expr))
                if self.needs_lowering(expr) || self.is_operand_choice(expr) =>
            {
                match expr {
                    Expression::Conditional(condition, value, otherwise) => {
                        out.push(Statement::If(IfStatement {
                            condition: (**condition).clone(),
                            then_branch: vec![Statement::Return(Some((**value).clone()))],
                            elif_branches: Vec::new(),
                            else_branch: Some(vec![Statement::Return(Some((**otherwise).clone()))]),
                        }));
                    }
                    expr => {
                        let value = self.lower_operand(expr, &mut out)?;
                        out.push(Statement::Return(Some(value)));
                    }
                }
            }
            Statement::If(if_stmt) if self.if_needs_lowering(if_stmt) => {
                self.desugar_if(if_stmt, &mut out)?
            }
            Statement::While(while_loop) if self.needs_lowering(&while_loop.condition) => {
                self.desugar_while(while_loop, &mut out)
            }
            Statement::For(for_loop)
                if for_loop.index.is_some()
                    || self.value_type(&for_loop.iter) == CobbleType::String
                    || self.needs_lowering(&for_loop.iter) =>
            {
                self.desugar_for(for_loop, &mut out)?
            }
            Statement::Match(match_stmt)
                if self.needs_lowering(&match_stmt.value)
                    || match_stmt
                        .cases
                        .iter()
                        .any(|case| matches!(case.pattern, MatchPattern::Text(_))) =>
            {
                self.desugar_match(match_stmt, &mut out)?
            }
            _ => return Ok(None),
        }
        if self.current_function.is_none() && out.len() > 1 {
            return Err(
                "This value takes commands to compute, so it can't be set outside a function. Assign it inside a function."
                    .into(),
            );
        }
        Ok(Some(out))
    }

    fn if_needs_lowering(&self, if_stmt: &IfStatement) -> bool {
        self.needs_lowering(&if_stmt.condition)
            || if_stmt
                .elif_branches
                .iter()
                .any(|(condition, _)| self.needs_lowering(condition))
    }

    /// Whether `expr` holds a form this module lowers.
    fn needs_lowering(&self, expr: &Expression) -> bool {
        let lowers = match expr {
            Expression::Conditional(..) | Expression::Slice(..) => true,
            Expression::Binary(_, BinaryOp::In | BinaryOp::NotIn, _) => true,
            Expression::Binary(left, op, right)
                if is_arithmetic(op) && (is_condition(left) || is_condition(right)) =>
            {
                true
            }
            Expression::Binary(left, op, right)
                if is_comparison(op) && (is_logical(left) || is_logical(right)) =>
            {
                true
            }
            Expression::Unary(UnaryOp::Neg | UnaryOp::Pos, inner) => is_condition(inner),
            Expression::Subscript(base, index) => {
                is_condition(index) || self.reads_base_before_call(base, index)
            }
            Expression::Binary(left, BinaryOp::Add, right) => self.joins(left, right),
            Expression::Binary(left, BinaryOp::Mul, right) => self.repeats(left, right),
            Expression::Call(func, args) => {
                self.is_python_call(func)
                    || args.iter().any(|arg| self.is_passed_condition(arg))
                    || (args.len() == 1
                        && matches!(&**func, Expression::Identifier(name)
                            if matches!(name.as_str(), "min" | "max")
                                && self.is_builtin_name(name)))
            }
            Expression::Array(items) => items.iter().any(|item| self.is_passed_condition(item)),
            Expression::Map(entries) => entries
                .iter()
                .any(|(_, value)| self.is_passed_condition(value)),
            _ => false,
        };
        lowers
            || sub_expressions(expr)
                .into_iter()
                .any(|part| self.needs_lowering(part))
    }

    /// Whether `func` names a builtin function or a list method that this
    /// module has to lower.
    fn is_python_call(&self, func: &Expression) -> bool {
        match func {
            Expression::Identifier(name) => {
                matches!(name.as_str(), "sum" | "str" | "sorted") && self.is_builtin_name(name)
            }
            Expression::Attribute(base, method) => {
                self.is_list_value(base)
                    && (LIST_HELPER_METHODS.contains(&method.as_str())
                        || LIST_VALUE_METHODS.contains(&method.as_str()))
            }
            _ => false,
        }
    }

    fn is_builtin_name(&self, name: &str) -> bool {
        BUILTIN_FUNCTIONS.contains(&name) && !self.functions.params.contains_key(name)
    }

    /// Whether `expr` names a storage value, so a method on it is a list
    /// method and not a call into a module.
    pub(in crate::transpiler) fn is_list_value(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Identifier(name) => {
                self.symbols.variable_storage_paths.contains_key(name)
                    || self.current_context.is_param(name)
            }
            Expression::Subscript(..) => true,
            Expression::Attribute(base, _) => self.is_list_value(base),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn is_list_command_method(method: &str) -> bool {
        LIST_COMMAND_METHODS.contains(&method)
    }

    fn is_container_or_text(&self, expr: &Expression) -> bool {
        matches!(
            self.value_type(expr),
            CobbleType::String | CobbleType::List | CobbleType::Map
        )
    }

    /// Whether `left + right` joins text or lists. With a number on one side
    /// it stays arithmetic, which reports text and reads a list's size.
    fn joins(&self, left: &Expression, right: &Expression) -> bool {
        let is_number = |value_type: &CobbleType| {
            matches!(value_type, CobbleType::Integer | CobbleType::Boolean)
        };
        let (left_type, right_type) = (self.value_type(left), self.value_type(right));
        // A slice is a list or text, even of a parameter.
        let is_slice = |expr: &Expression| matches!(expr, Expression::Slice(..));
        (self.is_container_or_text(left)
            || self.is_container_or_text(right)
            || is_slice(left)
            || is_slice(right))
            && !is_number(&left_type)
            && !is_number(&right_type)
    }

    /// Whether `left * right` repeats text or a list.
    fn repeats(&self, left: &Expression, right: &Expression) -> bool {
        self.is_container_or_text(left) != self.is_container_or_text(right)
    }

    /// The type of `expr`, as far as Cobble can tell before lowering it.
    pub(in crate::transpiler) fn value_type(&self, expr: &Expression) -> CobbleType {
        match expr {
            Expression::Subscript(base, _) if self.is_string_base(base) => CobbleType::String,
            Expression::Subscript(..) | Expression::Attribute(..) => self.storage_read_type(expr),
            Expression::Slice(base, ..) => self.value_type(base),
            Expression::Conditional(_, value, otherwise) => self.common_type(value, otherwise),
            Expression::Binary(left, BinaryOp::And | BinaryOp::Or, right)
                if self.is_operand_choice(expr) =>
            {
                self.common_type(left, right)
            }
            Expression::Binary(left, BinaryOp::Add | BinaryOp::Mul, right) => {
                match (self.value_type(left), self.value_type(right)) {
                    (CobbleType::String, _) | (_, CobbleType::String) => CobbleType::String,
                    (CobbleType::List, _) | (_, CobbleType::List) => CobbleType::List,
                    _ => CobbleType::Integer,
                }
            }
            Expression::Call(func, _)
                if matches!(&**func, Expression::Identifier(name)
                    if name == "str" && self.is_builtin_name(name)) =>
            {
                CobbleType::String
            }
            Expression::Call(func, _)
                if matches!(&**func, Expression::Identifier(name)
                    if name == "sorted" && self.is_builtin_name(name)) =>
            {
                CobbleType::List
            }
            expr => self.infer_type(expr),
        }
    }

    /// The type of a value that is either `left` or `right`.
    fn common_type(&self, left: &Expression, right: &Expression) -> CobbleType {
        match (self.value_type(left), self.value_type(right)) {
            (left, right) if left == right => left,
            (CobbleType::Integer, CobbleType::Boolean)
            | (CobbleType::Boolean, CobbleType::Integer) => CobbleType::Integer,
            _ => CobbleType::Unknown,
        }
    }

    fn desugar_multi_assign(
        &mut self,
        multi: &MultiAssign,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        // Every value is read before any target is written, so `a, b = b, a`
        // swaps them.
        let values: Vec<Expression> = if let [value] = multi.values.as_slice() {
            if matches!(
                self.value_type(value),
                CobbleType::Integer | CobbleType::Boolean
            ) {
                return Err("A number can't be unpacked into several names.".into());
            }
            let holder = self.hidden_local(self.value_type(value));
            out.push(assign(&holder, value.clone()));
            (0..multi.targets.len())
                .map(|index| {
                    Expression::Subscript(
                        Box::new(Expression::Identifier(holder.clone())),
                        Box::new(Expression::Number(index as f64)),
                    )
                })
                .collect()
        } else {
            let mut held = Vec::new();
            for value in &multi.values {
                let holder = self.hidden_local(self.value_type(value));
                out.push(assign(&holder, value.clone()));
                held.push(Expression::Identifier(holder));
            }
            held
        };
        for (target, value) in multi.targets.iter().zip(values) {
            out.push(assign_to(target, value));
        }
        Ok(())
    }

    fn desugar_assignment(
        &mut self,
        assignment: &Assignment,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        // `items[i if a else b] = c` runs the value first, as Python does.
        if let Some(target) = assignment
            .target_expr
            .as_ref()
            .filter(|target| self.target_needs_lowering(target))
        {
            let holder = self.hidden_local(self.value_type(&assignment.value));
            out.push(assign(&holder, assignment.value.clone()));
            let target_expr = Some(self.lower_target(target, out)?);
            out.push(Statement::Assignment(Assignment {
                target_expr,
                value: Expression::Identifier(holder),
                ..assignment.clone()
            }));
            return Ok(());
        }
        if let Expression::Conditional(condition, value, otherwise) = &assignment.value {
            if let Some(constant) = self.try_eval_const(condition) {
                let value = if constant != 0.0 { value } else { otherwise };
                out.push(Statement::Assignment(Assignment {
                    value: (**value).clone(),
                    ..assignment.clone()
                }));
                return Ok(());
            }
            let branch = |value: &Expression| {
                vec![Statement::Assignment(Assignment {
                    value: value.clone(),
                    ..assignment.clone()
                })]
            };
            out.push(Statement::If(IfStatement {
                condition: (**condition).clone(),
                then_branch: branch(value),
                elif_branches: Vec::new(),
                else_branch: Some(branch(otherwise)),
            }));
            return Ok(());
        }
        let value = self.lower_operand(&assignment.value, out)?;
        out.push(Statement::Assignment(Assignment {
            value,
            ..assignment.clone()
        }));
        Ok(())
    }

    /// Whether an index in the assignment target `target` takes statements.
    /// The target itself stays a place to write to.
    fn target_needs_lowering(&self, target: &Expression) -> bool {
        match target {
            Expression::Subscript(base, index) => {
                self.needs_lowering(index)
                    || is_condition(index)
                    || self.target_needs_lowering(base)
            }
            Expression::Attribute(base, _) => self.target_needs_lowering(base),
            _ => false,
        }
    }

    fn lower_target(
        &mut self,
        target: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        match target {
            Expression::Subscript(base, index) => Ok(Expression::Subscript(
                Box::new(self.lower_target(base, out)?),
                Box::new(self.lower_number(index, out)?),
            )),
            Expression::Attribute(base, name) => Ok(Expression::Attribute(
                Box::new(self.lower_target(base, out)?),
                name.clone(),
            )),
            target => Ok(target.clone()),
        }
    }

    /// Whether `base[index]` has to read `base` before `index` calls a
    /// function. A call can only replace text, and Python keeps reading the
    /// old text. A list changed by the call shows the change.
    fn reads_base_before_call(&self, base: &Expression, index: &Expression) -> bool {
        self.current_function.is_some()
            && ExpressionEvaluator::calls_user_code(index)
            && self.value_type(base) == CobbleType::String
            && !matches!(base, Expression::Identifier(name) if name.starts_with("_cobble_hold_"))
    }

    /// Runs a value with a call before a helper gets its list, as Python
    /// runs the argument before the method reads the list.
    fn hold_call(&mut self, value: Expression, out: &mut Vec<Statement>) -> Expression {
        if !ExpressionEvaluator::calls_user_code(&value) {
            return value;
        }
        let holder = self.hidden_local(self.value_type(&value));
        out.push(assign(&holder, value));
        Expression::Identifier(holder)
    }

    /// Whether `expr` is `and` or `or` with an operand that isn't just true
    /// or false. Python gives one of the operands, which a flag can't hold.
    fn is_operand_choice(&self, expr: &Expression) -> bool {
        let Expression::Binary(left, BinaryOp::And | BinaryOp::Or, right) = expr else {
            return false;
        };
        !(self.is_flag(left) && self.is_flag(right))
    }

    fn is_flag(&self, expr: &Expression) -> bool {
        match expr {
            Expression::Boolean(_) | Expression::Unary(UnaryOp::Not, _) => true,
            Expression::Binary(left, BinaryOp::And | BinaryOp::Or, right) => {
                self.is_flag(left) && self.is_flag(right)
            }
            Expression::Binary(_, op, _) => matches!(
                op,
                BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq
                    | BinaryOp::In
                    | BinaryOp::NotIn
            ),
            Expression::Identifier(_) | Expression::Subscript(..) | Expression::Attribute(..) => {
                self.value_type(expr) == CobbleType::Boolean
            }
            _ => false,
        }
    }

    /// Lowers a value that isn't a condition: `a or b` gives an operand.
    fn lower_operand(
        &mut self,
        expr: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        let Expression::Binary(left, op, right) = expr else {
            return self.lower_value(expr, out);
        };
        if !self.is_operand_choice(expr) {
            return self.lower_value(expr, out);
        }
        let (left_type, right_type) = (self.value_type(left), self.value_type(right));
        // A variable holds one type, so it can't take either of two values
        // that Cobble knows have different types.
        let holds_values = |value_type: &CobbleType| {
            matches!(
                value_type,
                CobbleType::String | CobbleType::List | CobbleType::Map
            )
        };
        let known = |value_type: &CobbleType| {
            holds_values(value_type)
                || matches!(value_type, CobbleType::Integer | CobbleType::Boolean)
        };
        if left_type != right_type
            && known(&left_type)
            && known(&right_type)
            && (holds_values(&left_type) || holds_values(&right_type))
        {
            let word = if matches!(op, BinaryOp::And) {
                "and"
            } else {
                "or"
            };
            return Err(format!(
                "`{word}` gives one of its operands, and here one is {} and the other {}, which one value can't be. Give both operands the same type, or use `if` and `else` with a variable of each type.",
                type_phrase(&left_type),
                type_phrase(&right_type)
            )
            .into());
        }
        let holder_type = if left_type == right_type {
            left_type
        } else {
            CobbleType::Unknown
        };
        let holder = self.hidden_local(holder_type);
        out.push(assign(&holder, (**left).clone()));
        let held = Expression::Identifier(holder.clone());
        out.push(Statement::If(IfStatement {
            condition: if matches!(op, BinaryOp::And) {
                held.clone()
            } else {
                Expression::Unary(UnaryOp::Not, Box::new(held.clone()))
            },
            then_branch: vec![assign(&holder, (**right).clone())],
            elif_branches: Vec::new(),
            else_branch: None,
        }));
        Ok(held)
    }

    fn desugar_expression_statement(
        &mut self,
        expr: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        if let Expression::Call(func, args) = expr {
            if let Expression::Attribute(base, method) = &**func {
                if self.is_list_value(base) && LIST_HELPER_METHODS.contains(&method.as_str()) {
                    return self.desugar_list_helper(base, method, args, out);
                }
                if self.is_list_value(base) && method == "pop" {
                    let args = self.lower_values(args, out)?;
                    out.push(Statement::Expression(Expression::Call(func.clone(), args)));
                    return Ok(());
                }
            }
        }
        // A value nobody reads still has to run, as the call in `f(s[1:])`.
        let lowered = self.lower_value(expr, out)?;
        if !matches!(lowered, Expression::Identifier(_)) {
            out.push(Statement::Expression(lowered));
        }
        Ok(())
    }

    /// `items.sort()` and the like: a helper builds the new list, and the
    /// list takes it.
    fn desugar_list_helper(
        &mut self,
        base: &Expression,
        method: &str,
        args: &[Expression],
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        let expected = match method {
            "insert" => 2,
            "remove" => 1,
            _ => 0,
        };
        if args.len() != expected {
            return Err(format!(
                "`{method}` takes {expected} argument(s), but {} provided.",
                args.len()
            )
            .into());
        }
        let helper = format!("_cobble_list_{method}");
        self.require_prelude(&helper)?;
        let mut call_args = vec![base.clone()];
        for arg in self.lower_values(args, out)? {
            call_args.push(self.hold_call(arg, out));
        }
        out.push(Statement::Expression(call(&helper, call_args)));
        self.declare_result(CobbleType::List);
        out.push(assign_to(base, Expression::Identifier(RESULT.to_string())));
        Ok(())
    }

    fn desugar_if(
        &mut self,
        if_stmt: &IfStatement,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        let condition = self.lower_value(&if_stmt.condition, out)?;
        // An `elif` condition only runs when the branches before it didn't,
        // so one that takes statements moves into the `else`.
        let split = if_stmt
            .elif_branches
            .iter()
            .position(|(condition, _)| self.needs_lowering(condition));
        let lowered = match split {
            None => IfStatement {
                condition,
                ..if_stmt.clone()
            },
            Some(index) => {
                let (condition_at, branch) = &if_stmt.elif_branches[index];
                IfStatement {
                    condition,
                    then_branch: if_stmt.then_branch.clone(),
                    elif_branches: if_stmt.elif_branches[..index].to_vec(),
                    else_branch: Some(vec![Statement::If(IfStatement {
                        condition: condition_at.clone(),
                        then_branch: branch.clone(),
                        elif_branches: if_stmt.elif_branches[index + 1..].to_vec(),
                        else_branch: if_stmt.else_branch.clone(),
                    })]),
                }
            }
        };
        out.push(Statement::If(lowered));
        Ok(())
    }

    /// A condition that takes statements runs them on each pass, so the loop
    /// runs on a flag that the condition clears.
    fn desugar_while(&mut self, while_loop: &WhileLoop, out: &mut Vec<Statement>) {
        let flag = self.hidden_local(CobbleType::Integer);
        out.push(assign(&flag, Expression::Number(1.0)));
        out.push(Statement::While(WhileLoop {
            condition: Expression::Identifier(flag.clone()),
            body: vec![Statement::If(IfStatement {
                condition: while_loop.condition.clone(),
                then_branch: while_loop.body.clone(),
                elif_branches: Vec::new(),
                else_branch: Some(vec![assign(&flag, Expression::Number(0.0))]),
            })],
        }));
    }

    fn desugar_for(
        &mut self,
        for_loop: &ForLoop,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        let mut lowered = for_loop.clone();
        if let Some(index) = &for_loop.index {
            let Expression::Call(func, args) = &for_loop.iter else {
                return Err(
                    "Two loop variables need `enumerate(...)`, as in `for i, item in enumerate(items):`."
                        .into(),
                );
            };
            let is_enumerate = matches!(&**func, Expression::Identifier(name)
                if name == "enumerate" && self.is_builtin_name(name));
            if !is_enumerate || args.is_empty() || args.len() > 2 {
                return Err(
                    "Two loop variables need `enumerate(...)`, as in `for i, item in enumerate(items):`."
                        .into(),
                );
            }
            let counter = self.hidden_local(CobbleType::Integer);
            let start = args.get(1).cloned().unwrap_or(Expression::Number(0.0));
            out.push(assign(&counter, start));
            let mut body = vec![
                assign(index, Expression::Identifier(counter.clone())),
                assign(
                    &counter,
                    compare(
                        Expression::Identifier(counter.clone()),
                        BinaryOp::Add,
                        Expression::Number(1.0),
                    ),
                ),
            ];
            body.extend(for_loop.body.iter().cloned());
            lowered.index = None;
            lowered.iter = args[0].clone();
            lowered.body = body;
        }
        let iter_type = self.value_type(&lowered.iter);
        lowered.iter = match self.lower_value(&lowered.iter, out)? {
            // The characters of literal text are known here.
            Expression::String(text) => Expression::Array(
                text.chars()
                    .map(|ch| Expression::String(ch.to_string()))
                    .collect(),
            ),
            // Text in a variable is walked by position.
            iter if iter_type == CobbleType::String => {
                let holder = self.hidden_local(CobbleType::String);
                out.push(assign(&holder, iter));
                let text = Expression::Identifier(holder);
                let position = self.hidden_local(CobbleType::Integer);
                let character = Expression::Subscript(
                    Box::new(text.clone()),
                    Box::new(Expression::Identifier(position.clone())),
                );
                let mut body = vec![assign(&lowered.target, character)];
                body.extend(lowered.body);
                lowered.body = body;
                lowered.target = position;
                call("range", vec![call("len", vec![text])])
            }
            iter => iter,
        };
        out.push(Statement::For(lowered));
        Ok(())
    }

    /// `match` on text runs its cases in order as an `if` chain.
    fn desugar_match(
        &mut self,
        match_stmt: &MatchStatement,
        out: &mut Vec<Statement>,
    ) -> Result<(), TranspileError> {
        let value = self.lower_value(&match_stmt.value, out)?;
        if !match_stmt
            .cases
            .iter()
            .any(|case| matches!(case.pattern, MatchPattern::Text(_)))
        {
            out.push(Statement::Match(MatchStatement {
                value,
                cases: match_stmt.cases.clone(),
            }));
            return Ok(());
        }
        let holder = self.hidden_local(self.value_type(&value));
        out.push(assign(&holder, value));
        let subject = Expression::Identifier(holder);
        let mut chain: Option<Vec<Statement>> = None;
        for case in match_stmt.cases.iter().rev() {
            let condition = match &case.pattern {
                MatchPattern::Text(text) => compare(
                    subject.clone(),
                    BinaryOp::Eq,
                    Expression::String(text.clone()),
                ),
                MatchPattern::Literal(number) => compare(
                    subject.clone(),
                    BinaryOp::Eq,
                    Expression::Number(f64::from(*number)),
                ),
                MatchPattern::Range(low, high) => compare(
                    compare(
                        subject.clone(),
                        BinaryOp::GtEq,
                        Expression::Number(f64::from(*low)),
                    ),
                    BinaryOp::And,
                    compare(
                        subject.clone(),
                        BinaryOp::LtEq,
                        Expression::Number(f64::from(*high)),
                    ),
                ),
                MatchPattern::Wildcard => {
                    chain = Some(case.body.clone());
                    continue;
                }
            };
            chain = Some(vec![Statement::If(IfStatement {
                condition,
                then_branch: case.body.clone(),
                elif_branches: Vec::new(),
                else_branch: chain.take(),
            })]);
        }
        out.extend(chain.unwrap_or_default());
        Ok(())
    }

    fn lower_values(
        &mut self,
        exprs: &[Expression],
        out: &mut Vec<Statement>,
    ) -> Result<Vec<Expression>, TranspileError> {
        let parts: Vec<&Expression> = exprs.iter().collect();
        self.lower_in_order(&parts, out, &Self::lower_passed)
    }

    /// Whether `expr`, passed as an argument, an item, or a field, needs a
    /// holder: `a or b` gives an operand, and a condition gives its 1 or 0.
    fn is_passed_condition(&self, expr: &Expression) -> bool {
        self.is_operand_choice(expr) || is_condition(expr)
    }

    /// A value passed as an argument, an item, or a field, as in
    /// `out.append(s in names)`. A condition there is its 1 or 0.
    fn lower_passed(&mut self, expr: &Expression, out: &mut Vec<Statement>) -> LoweredValue {
        if !is_condition(expr) || self.is_operand_choice(expr) {
            return self.lower_operand(expr, out);
        }
        let holder = self.hidden_local(CobbleType::Boolean);
        out.push(assign(&holder, expr.clone()));
        Ok(Expression::Identifier(holder))
    }

    /// Lowers `parts`, which run left to right. The statements of a later
    /// part run before the expression that uses all of them, so when a call
    /// is involved, the parts before it are held first, as Python reads
    /// them first.
    fn lower_in_order(
        &mut self,
        parts: &[&Expression],
        out: &mut Vec<Statement>,
        lower: &dyn Fn(&mut Self, &Expression, &mut Vec<Statement>) -> LoweredValue,
    ) -> Result<Vec<Expression>, TranspileError> {
        let calls = parts
            .iter()
            .any(|part| ExpressionEvaluator::calls_user_code(part));
        let mut lowered = Vec::with_capacity(parts.len());
        for (index, part) in parts.iter().enumerate() {
            let before = out.len();
            let value = lower(self, part, out)?;
            let has_statements = out.len() > before;
            // Statements of this part run after every part held so far.
            if calls && has_statements {
                self.hold_earlier(&mut lowered, &parts[..index], before, out);
            }
            lowered.push(value);
        }
        Ok(lowered)
    }

    /// Stores `values`, the lowered forms of `parts`, in hidden locals at
    /// position `at` of `out`, so they are read before what comes after.
    fn hold_earlier(
        &mut self,
        values: &mut [Expression],
        parts: &[&Expression],
        at: usize,
        out: &mut Vec<Statement>,
    ) {
        let mut position = at;
        for (value, part) in values.iter_mut().zip(parts) {
            let settled = matches!(
                value,
                Expression::Number(_) | Expression::String(_) | Expression::Boolean(_)
            ) || matches!(value, Expression::Identifier(name) if name.starts_with("_cobble_hold_"));
            if settled {
                continue;
            }
            let holder = self.hidden_local(self.value_type(part));
            out.insert(position, assign(&holder, value.clone()));
            position += 1;
            *value = Expression::Identifier(holder);
        }
    }

    /// `expr` without the forms this module lowers. Statements that compute
    /// a part go to `out`, which runs before `expr`.
    fn lower_value(
        &mut self,
        expr: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        if !self.needs_lowering(expr) {
            return Ok(expr.clone());
        }
        match expr {
            Expression::Conditional(condition, value, otherwise) => {
                if let Some(constant) = self.try_eval_const(condition) {
                    let chosen = if constant != 0.0 { value } else { otherwise };
                    return self.lower_value(chosen, out);
                }
                let holder = self.hidden_local(self.value_type(expr));
                out.push(Statement::If(IfStatement {
                    condition: (**condition).clone(),
                    then_branch: vec![assign(&holder, (**value).clone())],
                    elif_branches: Vec::new(),
                    else_branch: Some(vec![assign(&holder, (**otherwise).clone())]),
                }));
                Ok(Expression::Identifier(holder))
            }
            Expression::Slice(base, start, stop) => {
                self.lower_slice(base, start.as_deref(), stop.as_deref(), out)
            }
            Expression::Binary(left, op, right) => self.lower_binary(left, op, right, out),
            Expression::Unary(UnaryOp::Not, inner) => Ok(Expression::Unary(
                UnaryOp::Not,
                Box::new(self.lower_value(inner, out)?),
            )),
            Expression::Unary(op, inner) => Ok(Expression::Unary(
                op.clone(),
                Box::new(self.lower_number(inner, out)?),
            )),
            Expression::Call(func, args) => self.lower_call(func, args, out),
            Expression::Attribute(base, name) => Ok(Expression::Attribute(
                Box::new(self.lower_value(base, out)?),
                name.clone(),
            )),
            Expression::Subscript(base, index) => {
                let mut value = self.lower_value(base, out)?;
                if self.reads_base_before_call(&value, index) {
                    let holder = self.hidden_local(self.value_type(base));
                    out.push(assign(&holder, value));
                    value = Expression::Identifier(holder);
                }
                Ok(Expression::Subscript(
                    Box::new(value),
                    Box::new(self.lower_number(index, out)?),
                ))
            }
            Expression::Array(items) => Ok(Expression::Array(self.lower_values(items, out)?)),
            Expression::Map(entries) => Ok(Expression::Map(
                entries
                    .iter()
                    .map(|(key, value)| Ok((key.clone(), self.lower_passed(value, out)?)))
                    .collect::<Result<_, TranspileError>>()?,
            )),
            other => Ok(other.clone()),
        }
    }

    fn lower_slice(
        &mut self,
        base: &Expression,
        start: Option<&Expression>,
        stop: Option<&Expression>,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        let base_type = self.value_type(base);
        let helper = match base_type {
            CobbleType::String => "_cobble_text_slice",
            CobbleType::List => "_cobble_list_slice",
            _ => "_cobble_slice",
        };
        let parts = [
            base.clone(),
            start.cloned().unwrap_or(Expression::Number(0.0)),
            stop.cloned()
                .unwrap_or(Expression::Number(f64::from(i32::MAX))),
        ];
        let args = self.lower_values(&parts, out)?;
        self.helper_value(helper, args, base_type, out)
    }

    fn lower_binary(
        &mut self,
        left: &Expression,
        op: &BinaryOp,
        right: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        // The right side of `and` and `or` only runs when it decides the
        // result, so its statements go into a branch.
        if matches!(op, BinaryOp::And | BinaryOp::Or) && self.needs_lowering(right) {
            let truthy = Expression::Unary(
                UnaryOp::Not,
                Box::new(Expression::Unary(UnaryOp::Not, Box::new(right.clone()))),
            );
            let holder = self.hidden_local(CobbleType::Boolean);
            let (then_value, else_value) = if matches!(op, BinaryOp::And) {
                (truthy, Expression::Boolean(false))
            } else {
                (Expression::Boolean(true), truthy)
            };
            out.push(Statement::If(IfStatement {
                condition: left.clone(),
                then_branch: vec![assign(&holder, then_value)],
                elif_branches: Vec::new(),
                else_branch: Some(vec![assign(&holder, else_value)]),
            }));
            return Ok(Expression::Identifier(holder));
        }

        match op {
            BinaryOp::In | BinaryOp::NotIn => self.lower_membership(left, op, right, out),
            BinaryOp::Add if self.joins(left, right) => self.lower_join(left, right, out),
            BinaryOp::Mul if self.repeats(left, right) => {
                if self.is_container_or_text(left) {
                    self.lower_repeat(left, right, out)
                } else {
                    self.lower_repeat(right, left, out)
                }
            }
            op if is_arithmetic(op) => {
                let sides = self.lower_in_order(&[left, right], out, &Self::lower_number)?;
                let (left, right) = two(sides);
                Ok(compare(left, op.clone(), right))
            }
            op if is_comparison(op) => {
                let sides = self.lower_in_order(&[left, right], out, &Self::lower_compared)?;
                let (left, right) = two(sides);
                Ok(compare(left, op.clone(), right))
            }
            op => Ok(compare(
                self.lower_value(left, out)?,
                op.clone(),
                self.lower_value(right, out)?,
            )),
        }
    }

    /// An operand of a comparison.
    fn lower_compared(
        &mut self,
        expr: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        if is_logical(expr) {
            self.lower_number(expr, out)
        } else {
            self.lower_value(expr, out)
        }
    }

    /// An operand of arithmetic. A condition there is its 1 or 0.
    fn lower_number(
        &mut self,
        expr: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        if !is_condition(expr) || self.is_operand_choice(expr) {
            return self.lower_operand(expr, out);
        }
        let holder = self.hidden_local(CobbleType::Boolean);
        out.push(assign(&holder, expr.clone()));
        Ok(Expression::Identifier(holder))
    }

    fn lower_membership(
        &mut self,
        value: &Expression,
        op: &BinaryOp,
        container: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        let found_at = |call: Expression| compare(call, BinaryOp::GtEq, Expression::Number(0.0));
        let container_type = self.value_type(container);
        let (value, container) = two(self.lower_values(&[value.clone(), container.clone()], out)?);
        let value = self.hold_call(value, out);
        let found = match container_type {
            CobbleType::List => {
                self.require_prelude("_cobble_list_index")?;
                found_at(call("_cobble_list_index", vec![container, value]))
            }
            CobbleType::String => {
                self.require_prelude("_cobble_text_find")?;
                found_at(call("_cobble_text_find", vec![container, value]))
            }
            CobbleType::Map => {
                self.require_prelude("_cobble_has_key")?;
                compare(
                    call("_cobble_has_key", vec![container, value]),
                    BinaryOp::Eq,
                    Expression::Number(1.0),
                )
            }
            _ => {
                self.require_prelude("_cobble_contains")?;
                compare(
                    call("_cobble_contains", vec![container, value]),
                    BinaryOp::Eq,
                    Expression::Number(1.0),
                )
            }
        };
        Ok(if matches!(op, BinaryOp::NotIn) {
            Expression::Unary(UnaryOp::Not, Box::new(found))
        } else {
            found
        })
    }

    fn lower_join(
        &mut self,
        left: &Expression,
        right: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        let types = [self.value_type(left), self.value_type(right)];
        let is_list = types.contains(&CobbleType::List);
        let is_text = types.contains(&CobbleType::String);
        let (left, right) = two(self.lower_values(&[left.clone(), right.clone()], out)?);
        // Slices of a parameter: the running pack tells lists from text.
        if !is_list && !is_text {
            return self.helper_value("_cobble_join", vec![left, right], CobbleType::Unknown, out);
        }
        if is_list {
            let holder = self.hidden_local(CobbleType::List);
            out.push(assign(&holder, left));
            out.push(Statement::Expression(Expression::Call(
                Box::new(Expression::Attribute(
                    Box::new(Expression::Identifier(holder.clone())),
                    "extend".to_string(),
                )),
                vec![right],
            )));
            return Ok(Expression::Identifier(holder));
        }
        if let (Expression::String(left), Expression::String(right)) = (&left, &right) {
            return Ok(Expression::String(format!("{left}{right}")));
        }
        self.helper_value(
            "_cobble_text_join",
            vec![left, right],
            CobbleType::String,
            out,
        )
    }

    fn lower_repeat(
        &mut self,
        value: &Expression,
        count: &Expression,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        let value_type = self.value_type(value);
        let value = self.lower_value(value, out)?;
        let count = self.lower_value(count, out)?;
        if let Some(times) = self.try_eval_const(&count) {
            let times = times.max(0.0) as usize;
            match &value {
                Expression::Array(items) if times * items.len() <= 4096 => {
                    let repeated = items.iter().cycle().take(times * items.len());
                    return Ok(Expression::Array(repeated.cloned().collect()));
                }
                Expression::String(text) if times * text.len() <= 4096 => {
                    return Ok(Expression::String(text.repeat(times)));
                }
                _ => {}
            }
        }
        let helper = if value_type == CobbleType::List {
            "_cobble_list_repeat"
        } else {
            "_cobble_text_repeat"
        };
        self.helper_value(helper, vec![value, count], value_type, out)
    }

    fn lower_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        if let Expression::Attribute(base, method) = func {
            if self.is_list_value(base) {
                if let Some(value) = self.lower_list_value(base, method, args, out)? {
                    return Ok(value);
                }
            }
        }
        let lowered_args = self.lower_values(args, out)?;
        if let Expression::Identifier(name) = func {
            if self.is_builtin_name(name) {
                let list_helper = match (name.as_str(), lowered_args.len()) {
                    ("sum", 1) => Some("_cobble_list_sum"),
                    ("min", 1) => Some("_cobble_list_min"),
                    ("max", 1) => Some("_cobble_list_max"),
                    _ => None,
                };
                if let Some(helper) = list_helper {
                    self.require_prelude(helper)?;
                    return Ok(call(helper, lowered_args));
                }
                if name == "str" {
                    return self.helper_value("_cobble_str", lowered_args, CobbleType::String, out);
                }
                if name == "sorted" {
                    return self.helper_value(
                        "_cobble_list_sort",
                        lowered_args,
                        CobbleType::List,
                        out,
                    );
                }
            }
        }
        Ok(Expression::Call(Box::new(func.clone()), lowered_args))
    }

    /// A list method that gives a value. Other methods give `None` here.
    fn lower_list_value(
        &mut self,
        base: &Expression,
        method: &str,
        args: &[Expression],
        out: &mut Vec<Statement>,
    ) -> Result<Option<Expression>, TranspileError> {
        match method {
            "index" | "count" => {
                if args.len() != 1 {
                    return Err(format!("`{method}` takes 1 argument.").into());
                }
                let helper = format!("_cobble_list_{method}");
                self.require_prelude(&helper)?;
                let value = self.lower_value(&args[0], out)?;
                let value = self.hold_call(value, out);
                Ok(Some(call(&helper, vec![base.clone(), value])))
            }
            "pop" => {
                if args.len() > 1 {
                    return Err("`pop` takes at most 1 argument.".into());
                }
                let position = match args.first() {
                    Some(position) => self.lower_value(position, out)?,
                    None => Expression::Number(-1.0),
                };
                let holder = self.hidden_local(CobbleType::Unknown);
                out.push(assign(
                    &holder,
                    Expression::Subscript(
                        Box::new(base.clone()),
                        Box::new(position.clone()),
                    ),
                ));
                out.push(Statement::Expression(Expression::Call(
                    Box::new(Expression::Attribute(
                        Box::new(base.clone()),
                        "pop".to_string(),
                    )),
                    vec![position],
                )));
                Ok(Some(Expression::Identifier(holder)))
            }
            _ if LIST_HELPER_METHODS.contains(&method) => Err(format!(
                "`{method}` changes the list and gives no value, so it can't be part of an expression."
            )
            .into()),
            _ => Ok(None),
        }
    }

    /// Calls `helper`, and gives the hidden local that holds its result.
    fn helper_value(
        &mut self,
        helper: &str,
        args: Vec<Expression>,
        value_type: CobbleType,
        out: &mut Vec<Statement>,
    ) -> Result<Expression, TranspileError> {
        self.require_prelude(helper)?;
        out.push(Statement::Expression(call(helper, args)));
        let holder = self.hidden_local(value_type.clone());
        self.declare_result(value_type);
        out.push(assign(&holder, Expression::Identifier(RESULT.to_string())));
        Ok(Expression::Identifier(holder))
    }

    /// Makes [`RESULT`] readable as a variable of `value_type`.
    fn declare_result(&mut self, value_type: CobbleType) {
        self.symbols
            .variable_storage_paths
            .insert(RESULT.to_string(), RESULT.to_string());
        self.symbols
            .variable_types
            .insert(RESULT.to_string(), value_type);
    }

    /// A new local variable that holds a value for one statement.
    fn hidden_local(&mut self, value_type: CobbleType) -> String {
        let name = format!("_cobble_hold_{}", self.next_temp_id());
        if self.current_function.is_some() {
            self.current_context.locals.push(name.clone());
        }
        if value_type != CobbleType::Unknown {
            self.symbols.variable_types.insert(name.clone(), value_type);
        }
        name
    }

    /// Compiles the prelude helper `name` into the pack, after the helpers
    /// it calls.
    pub(in crate::transpiler) fn require_prelude(
        &mut self,
        name: &str,
    ) -> Result<(), TranspileError> {
        if !self.prelude_compiled.insert(name.to_string()) {
            return Ok(());
        }
        let source = PRELUDE.replace("NS", &self.data_pack.namespace);
        let program = crate::parser::parse(&source)
            .map_err(|errors| format!("The prelude doesn't parse: {}", errors.join("; ")))?;
        let helper: Option<FunctionDef> =
            program
                .statements
                .into_iter()
                .find_map(|statement| match statement {
                    Statement::FunctionDef(func) if func.name == name => Some(func),
                    _ => None,
                });
        let Some(helper) = helper else {
            return Err(format!("The prelude has no helper `{name}`.").into());
        };
        for callee in called_functions(&helper.body) {
            if callee.starts_with("_cobble_") {
                self.require_prelude(&callee)?;
            }
        }
        self.symbols
            .variable_storage_paths
            .insert(RESULT.to_string(), RESULT.to_string());
        self.symbols.variable_types.remove(RESULT);
        let previous_prelude = std::mem::replace(&mut self.in_prelude, true);
        let result = self.process_function_def(&helper);
        self.in_prelude = previous_prelude;
        result
    }
}
