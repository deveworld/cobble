//! Values a program writes into lists and maps.
//!
//! Cobble tells the type of `names[i]` or `config.title` from the literal the
//! variable holds. A write anywhere in the program can change an item or a
//! field, in a function that runs first or in a file compiled later, so a
//! read keeps the literal's type only when every write has that type too.

use crate::ast::{Assignment, CobbleType, Expression, MultiAssign, Statement};
use std::collections::{HashMap, HashSet};

/// The places and types each variable gets written. The place is a field
/// name, or `None` for an item, a key known only when the pack runs, or the
/// whole value.
pub(in crate::transpiler) type ContainerWrites =
    HashMap<String, HashSet<(Option<String>, CobbleType)>>;

/// Records the writes of `statements` and of the statements inside them.
/// Function bodies count too, and a name means every variable that has it.
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

/// `a[i], b.x = ...` writes like one assignment per target.
fn record_multi_assign(multi: &MultiAssign, writes: &mut ContainerWrites) {
    for target in &multi.targets {
        if let Expression::Subscript(base, _) | Expression::Attribute(base, _) = target {
            if let Expression::Identifier(name) = &**base {
                writes
                    .entry(name.clone())
                    .or_default()
                    .insert((None, CobbleType::Unknown));
            }
        }
    }
}

/// `items.append(x)` and the other methods that add items.
fn record_list_method(expr: &Expression, writes: &mut ContainerWrites) {
    let Expression::Call(func, args) = expr else {
        return;
    };
    let Expression::Attribute(base, method) = &**func else {
        return;
    };
    let Expression::Identifier(name) = &**base else {
        return;
    };
    let value_type = match (method.as_str(), args.as_slice()) {
        ("append", [value]) | ("insert", [_, value]) => written_type(value),
        ("extend", _) => CobbleType::Unknown,
        _ => return,
    };
    writes
        .entry(name.clone())
        .or_default()
        .insert((None, value_type));
}

fn record_assignment(assign: &Assignment, writes: &mut ContainerWrites) {
    let mut record = |name: &str, field: Option<&str>, value_type: CobbleType| {
        writes
            .entry(name.to_string())
            .or_default()
            .insert((field.map(str::to_string), value_type));
    };
    if assign.assigns_name() {
        match &assign.value {
            Expression::Array(items) => {
                for item in items {
                    record(&assign.target, None, written_type(item));
                }
            }
            Expression::Map(entries) => {
                for (field, value) in entries {
                    record(&assign.target, Some(field), written_type(value));
                }
            }
            // A number or a string doesn't hold items.
            Expression::Number(_)
            | Expression::String(_)
            | Expression::Boolean(_)
            | Expression::None => {}
            // A copy of a variable, or a call: any item can be anything.
            _ => record(&assign.target, None, CobbleType::Unknown),
        }
        return;
    }
    // A write deeper inside, such as `config.nested.value = 1`, leaves
    // `config.nested` a map.
    match &assign.target_expr {
        Some(Expression::Subscript(base, index)) => {
            if let Expression::Identifier(name) = &**base {
                let field = match &**index {
                    Expression::String(field) => Some(field.as_str()),
                    _ => None,
                };
                record(name, field, written_type(&assign.value));
            }
        }
        Some(Expression::Attribute(base, field)) => {
            if let Expression::Identifier(name) = &**base {
                record(name, Some(field), written_type(&assign.value));
            }
        }
        _ => {}
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

/// The type a read of an item or a field of `name` gives, where its literal
/// gives `literal`: `literal` when every write that can reach the read has
/// that type, and `Unknown` otherwise. `field` is the field the read names,
/// or `None` for an index or a key known only when the pack runs.
pub(in crate::transpiler) fn read_type(
    writes: &ContainerWrites,
    name: &str,
    field: Option<&str>,
    literal: CobbleType,
) -> CobbleType {
    let Some(written) = writes.get(name) else {
        return literal;
    };
    let differs = written.iter().any(|(written_field, written_type)| {
        let reaches =
            field.is_none() || written_field.is_none() || written_field.as_deref() == field;
        reaches && *written_type != literal
    });
    if differs {
        CobbleType::Unknown
    } else {
        literal
    }
}
