//! Values a program writes into lists and maps.
//!
//! Cobble tells the type of `names[i]`, `config.title`, or `grid[r][c]` from
//! the literal the variable holds. A write anywhere in the program can change
//! an item or a field at any depth, in a function that runs first or in a
//! file compiled later, so a read keeps the literal's type only when every
//! write that can reach it gives that type too.

use crate::ast::{Assignment, BinaryOp, CobbleType, Expression, MultiAssign, Statement};
use std::collections::HashMap;

/// One step of a path into a value: a field, or an item picked by an index
/// or a key.
#[derive(Clone, Copy)]
pub(in crate::transpiler) enum Step<'a> {
    Field(&'a str),
    Item(&'a Expression),
}

impl Step<'_> {
    /// The field this step names, if it names one. An index, or a key known
    /// only when the pack runs, can be any field or item.
    fn field(self) -> Option<String> {
        match self {
            Step::Field(field) => Some(field.to_string()),
            Step::Item(Expression::String(field)) => Some(field.clone()),
            Step::Item(_) => None,
        }
    }
}

/// The variable `expr` reads from and the steps into it, as `grid` and
/// `[r], [c]` for `grid[r][c]`.
pub(in crate::transpiler) fn access_path(expr: &Expression) -> Option<(&str, Vec<Step<'_>>)> {
    match expr {
        Expression::Identifier(name) => Some((name, Vec::new())),
        Expression::Attribute(base, field) => {
            let (name, mut steps) = access_path(base)?;
            steps.push(Step::Field(field));
            Some((name, steps))
        }
        Expression::Subscript(base, index) => {
            let (name, mut steps) = access_path(base)?;
            steps.push(Step::Item(index));
            Some((name, steps))
        }
        _ => None,
    }
}

/// `expr` read from `root` instead of from its own variable, as
/// `cfg.x[1]` for `node[1]` and `cfg.x`.
pub(in crate::transpiler) fn rebase(expr: &Expression, root: &Expression) -> Expression {
    match expr {
        Expression::Attribute(base, field) => {
            Expression::Attribute(Box::new(rebase(base, root)), field.clone())
        }
        Expression::Subscript(base, index) => {
            Expression::Subscript(Box::new(rebase(base, root)), index.clone())
        }
        _ => root.clone(),
    }
}

/// A write into a variable: the fields on its path (`None` for an item or a
/// key known only when the pack runs), and the value written there, or
/// `None` when the value can be anything.
#[derive(Clone)]
pub(in crate::transpiler) struct Write {
    path: Vec<Option<String>>,
    value: Option<Expression>,
}

/// The writes of the program, by the name of the variable written. A name
/// means every variable that has it.
pub(in crate::transpiler) type ContainerWrites = HashMap<String, Vec<Write>>;

/// Records the writes of `statements` and of the statements inside them.
/// Function bodies count too.
pub(in crate::transpiler) fn collect_container_writes(
    statements: &[Statement],
    writes: &mut ContainerWrites,
) {
    for statement in statements {
        match statement {
            Statement::Assignment(assign) => record_assignment(assign, writes),
            Statement::MultiAssign(multi) => record_multi_assign(multi, writes),
            Statement::Expression(expr) => record_list_method(expr, writes),
            Statement::FunctionDef(func) => collect_container_writes(&func.body, writes),
            Statement::If(if_stmt) => {
                collect_container_writes(&if_stmt.then_branch, writes);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_container_writes(branch, writes);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_container_writes(branch, writes);
                }
            }
            Statement::For(for_loop) => collect_container_writes(&for_loop.body, writes),
            Statement::While(while_loop) => collect_container_writes(&while_loop.body, writes),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_container_writes(&case.body, writes);
                }
            }
            Statement::Execute(block) => collect_container_writes(&block.body, writes),
            _ => {}
        }
    }
}

/// Records that `value` is written at `target`, or at an item of it for
/// each of `items`.
fn record(
    writes: &mut ContainerWrites,
    target: &Expression,
    items: usize,
    value: Option<&Expression>,
) {
    let Some((name, steps)) = access_path(target) else {
        return;
    };
    let mut path: Vec<Option<String>> = steps.into_iter().map(Step::field).collect();
    path.extend(std::iter::repeat_n(None, items));
    writes.entry(name.to_string()).or_default().push(Write {
        path,
        value: value.cloned(),
    });
}

/// `a[i], b.x = ...` can write anything to each target.
/// A name gets its own value of `a, b = x, y`; an item of the one value of
/// `a, b = pair` can be anything.
fn record_multi_assign(multi: &MultiAssign, writes: &mut ContainerWrites) {
    let paired = multi.values.len() == multi.targets.len();
    for (index, target) in multi.targets.iter().enumerate() {
        let value = match target {
            Expression::Identifier(_) if paired => multi.values.get(index),
            _ => None,
        };
        record(writes, target, 0, value);
    }
}

/// `items.append(x)` and the other methods that add items, on a variable or
/// on a list inside one.
fn record_list_method(expr: &Expression, writes: &mut ContainerWrites) {
    let Expression::Call(func, args) = expr else {
        return;
    };
    let Expression::Attribute(base, method) = &**func else {
        return;
    };
    match (method.as_str(), args.as_slice()) {
        ("append", [value]) | ("insert", [_, value]) => record(writes, base, 1, Some(value)),
        ("extend", _) => record(writes, base, 1, None),
        _ => {}
    }
}

fn record_assignment(assign: &Assignment, writes: &mut ContainerWrites) {
    let target = assign
        .target_expr
        .clone()
        .unwrap_or_else(|| Expression::Identifier(assign.target.clone()));
    // A number or a string doesn't hold items.
    if assign.assigns_name()
        && matches!(
            assign.value,
            Expression::Number(_)
                | Expression::String(_)
                | Expression::Boolean(_)
                | Expression::None
        )
    {
        return;
    }
    // `x += 1` and `s *= 2` keep the type the value had; `items += [4]`
    // and `items = [4] + items` add items, each a write of its own.
    if let Expression::Binary(left, op, right) = &assign.value {
        let added = if **left == target {
            Some(&**right)
        } else if **right == target && *op == BinaryOp::Add {
            Some(&**left)
        } else {
            None
        };
        if let Some(added) = added {
            if *op == BinaryOp::Add {
                record_added_items(writes, &target, added);
            }
            return;
        }
    }
    record(writes, &target, 0, Some(&assign.value));
}

/// Records the items that `target + added` adds to the list at `target`:
/// each item of a list literal, or an item of any type for a value only the
/// running pack knows. A number or a text adds no item of another kind, and
/// the list's own items or slices add none it doesn't hold.
fn record_added_items(writes: &mut ContainerWrites, target: &Expression, added: &Expression) {
    match added {
        Expression::Array(items) => {
            for item in items {
                record(writes, target, 1, Some(item));
            }
        }
        Expression::Number(_) | Expression::String(_) | Expression::Boolean(_) | Expression::None => {}
        Expression::Slice(base, _, _) if **base == *target => {}
        _ if added == target => {}
        Expression::Binary(left, BinaryOp::Add, right) => {
            record_added_items(writes, target, left);
            record_added_items(writes, target, right);
        }
        _ => record(writes, target, 1, None),
    }
}

/// The type `value` stores. A variable, an item, or a call depends on where
/// the write is, so it gives `Unknown`.
fn written_type(value: &Expression) -> CobbleType {
    match value {
        // Booleans, arithmetic, and comparisons store numbers.
        Expression::Number(_)
        | Expression::Boolean(_)
        | Expression::Binary(..)
        | Expression::Unary(..) => CobbleType::Integer,
        Expression::String(_) => CobbleType::String,
        Expression::Array(_) => CobbleType::List,
        Expression::Map(_) => CobbleType::Map,
        _ => CobbleType::Unknown,
    }
}

/// The type at the end of `steps` in the literal `value`, where `leaf`
/// gives the type of each value found there. An index can pick any item of
/// a list and a key known only when the pack runs any field, so they give a
/// type when all of those share it. A character of a string is a string.
pub(in crate::transpiler) fn literal_type(
    value: &Expression,
    steps: &[Step<'_>],
    leaf: &dyn Fn(&Expression) -> CobbleType,
) -> CobbleType {
    let Some((step, rest)) = steps.split_first() else {
        return leaf(value);
    };
    let shared = |values: &mut dyn Iterator<Item = &Expression>| {
        let mut types = values.map(|value| literal_type(value, rest, leaf));
        let first = types.next().unwrap_or(CobbleType::Unknown);
        if types.all(|other| other == first) {
            first
        } else {
            CobbleType::Unknown
        }
    };
    match (value, step) {
        (Expression::Array(items), Step::Item(_)) => shared(&mut items.iter()),
        (Expression::Map(entries), step) => match step.field() {
            Some(field) => entries
                .iter()
                .find(|(key, _)| *key == field)
                .map_or(CobbleType::Unknown, |(_, value)| {
                    literal_type(value, rest, leaf)
                }),
            None => shared(&mut entries.iter().map(|(_, value)| value)),
        },
        // Every further index of a character gives the same character.
        (Expression::String(_), Step::Item(_))
            if rest.iter().all(|step| matches!(step, Step::Item(_))) =>
        {
            CobbleType::String
        }
        _ => CobbleType::Unknown,
    }
}

/// The type a read of `steps` into `name` gives, where its literal gives
/// `literal`: `literal` when every write that can reach the read gives that
/// type, and `Unknown` otherwise. A write deeper inside the value read
/// leaves its type as it was.
pub(in crate::transpiler) fn read_type(
    writes: &ContainerWrites,
    name: &str,
    steps: &[Step<'_>],
    literal: CobbleType,
) -> CobbleType {
    let Some(written) = writes.get(name) else {
        return literal;
    };
    let fields: Vec<Option<String>> = steps.iter().map(|step| step.field()).collect();
    let differs = written.iter().any(|write| {
        if write.path.len() > fields.len() {
            return false;
        }
        let reaches = write
            .path
            .iter()
            .zip(&fields)
            .all(|(written, read)| written.is_none() || read.is_none() || written == read);
        if !reaches {
            return false;
        }
        let written = match &write.value {
            Some(value) => literal_type(value, &steps[write.path.len()..], &written_type),
            None => CobbleType::Unknown,
        };
        written != literal
    });
    if differs {
        CobbleType::Unknown
    } else {
        literal
    }
}

/// A fractional number the literal `value` can hold at `steps`, as 0.6 for
/// `y[0]` after `y = [0.6, 2.5]`. An index known only when the pack runs can
/// pick any item.
pub(in crate::transpiler) fn fractional_literal_at(
    value: &Expression,
    steps: &[Step<'_>],
) -> Option<f64> {
    let Some((step, rest)) = steps.split_first() else {
        return match value {
            Expression::Number(n) if n.fract() != 0.0 && n.is_finite() => Some(*n),
            _ => None,
        };
    };
    match (value, step) {
        (Expression::Array(items), Step::Item(Expression::Number(index)))
            if index.fract() == 0.0 =>
        {
            let at = if *index < 0.0 {
                items.len() as f64 + index
            } else {
                *index
            };
            items
                .get(at as usize)
                .filter(|_| at >= 0.0)
                .and_then(|item| fractional_literal_at(item, rest))
        }
        (Expression::Array(items), Step::Item(_)) => items
            .iter()
            .find_map(|item| fractional_literal_at(item, rest)),
        (Expression::Map(entries), step) => match step.field() {
            Some(field) => entries
                .iter()
                .find(|(key, _)| *key == field)
                .and_then(|(_, value)| fractional_literal_at(value, rest)),
            None => entries
                .iter()
                .find_map(|(_, value)| fractional_literal_at(value, rest)),
        },
        _ => None,
    }
}
