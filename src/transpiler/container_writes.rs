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
    // `s = 2 * s` and `x = x` keep the type too.
    if keeps_type(&assign.value, &target) {
        return;
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
        Expression::Number(_)
        | Expression::String(_)
        | Expression::Boolean(_)
        | Expression::None => {}
        Expression::Slice(base, _, _) if **base == *target => {}
        _ if added == target => {}
        Expression::Binary(left, BinaryOp::Add, right) => {
            record_added_items(writes, target, left);
            record_added_items(writes, target, right);
        }
        _ => record(writes, target, 1, None),
    }
}

/// Whether assigning `value` to `target` keeps the type `target` had: an
/// operation on `target` itself, as `x += 1`, `s *= 2`, or `s = 2 * s`.
fn keeps_type(value: &Expression, target: &Expression) -> bool {
    match value {
        // `x = x` changes nothing.
        value if value == target => true,
        Expression::Binary(left, op, right) => {
            **left == *target || (matches!(op, BinaryOp::Add | BinaryOp::Mul) && **right == *target)
        }
        _ => false,
    }
}

/// Every value the program assigns to each name as a whole, `None` for one
/// it can't see, as a parameter, a loop variable, or a name unpacked.
pub(in crate::transpiler) type RootValues = HashMap<String, Vec<Option<Expression>>>;

/// Records the whole-name assignments of `statements` and of the
/// statements inside them, function bodies included.
pub(in crate::transpiler) fn collect_root_values(statements: &[Statement], roots: &mut RootValues) {
    let unseen = |name: &str, roots: &mut RootValues| {
        roots.entry(name.to_string()).or_default().push(None);
    };
    for statement in statements {
        match statement {
            Statement::Assignment(assign) if assign.assigns_name() => roots
                .entry(assign.target.clone())
                .or_default()
                .push(Some(assign.value.clone())),
            Statement::MultiAssign(multi) => {
                for target in &multi.targets {
                    if let Expression::Identifier(name) = target {
                        unseen(name, roots);
                    }
                }
            }
            Statement::FunctionDef(func) => {
                for param in &func.params {
                    unseen(&param.name, roots);
                }
                collect_root_values(&func.body, roots);
            }
            Statement::If(if_stmt) => {
                collect_root_values(&if_stmt.then_branch, roots);
                for (_, branch) in &if_stmt.elif_branches {
                    collect_root_values(branch, roots);
                }
                if let Some(branch) = &if_stmt.else_branch {
                    collect_root_values(branch, roots);
                }
            }
            Statement::For(for_loop) => {
                unseen(&for_loop.target, roots);
                if let Some(index) = &for_loop.index {
                    unseen(index, roots);
                }
                collect_root_values(&for_loop.body, roots);
            }
            Statement::While(while_loop) => collect_root_values(&while_loop.body, roots),
            Statement::Match(match_stmt) => {
                for case in &match_stmt.cases {
                    collect_root_values(&case.body, roots);
                }
            }
            Statement::Execute(block) => collect_root_values(&block.body, roots),
            _ => {}
        }
    }
}

/// The type of the variable `name` where it is written back into the place
/// `root` and `steps` name: the type its assignments give it, leaving out
/// copies of that place and operations on `name` itself, which keep the
/// type the place has, `place`. `Unknown` when an assignment can give
/// anything.
pub(in crate::transpiler) fn copy_type(
    roots: &RootValues,
    name: &str,
    root: &str,
    steps: &[Step<'_>],
    place: &CobbleType,
) -> CobbleType {
    let Some(values) = roots.get(name) else {
        return CobbleType::Unknown;
    };
    let itself = Expression::Identifier(name.to_string());
    let mut found: Option<CobbleType> = None;
    for value in values {
        let Some(value) = value else {
            return CobbleType::Unknown;
        };
        if keeps_type(value, &itself) {
            continue;
        }
        let copies_place = access_path(value).is_some_and(|(read_root, read_steps)| {
            read_root == root
                && read_steps.len() == steps.len()
                && read_steps
                    .iter()
                    .zip(steps)
                    .all(|(read, step)| same_step(*read, *step))
        });
        if copies_place {
            continue;
        }
        let value_type = match written_type(value) {
            CobbleType::Boolean => CobbleType::Integer,
            other => other,
        };
        if value_type == CobbleType::Unknown || found.as_ref().is_some_and(|t| *t != value_type) {
            return CobbleType::Unknown;
        }
        found = Some(value_type);
    }
    found.unwrap_or_else(|| place.clone())
}

/// Whether two steps always pick the same field or item: the same field,
/// or the same literal index.
fn same_step(first: Step<'_>, second: Step<'_>) -> bool {
    match (first.field(), second.field()) {
        (Some(first), Some(second)) => first == second,
        (None, None) => match (first, second) {
            (Step::Item(Expression::Number(a)), Step::Item(Expression::Number(b))) => a == b,
            _ => false,
        },
        _ => false,
    }
}

/// Whether the literal `value` has nothing at the end of `steps`: an item of
/// an empty list, or a field a map literal doesn't have. Only writes then
/// tell what is there.
pub(in crate::transpiler) fn literal_lacks(value: &Expression, steps: &[Step<'_>]) -> bool {
    let Some((step, rest)) = steps.split_first() else {
        return false;
    };
    match (value, step) {
        (Expression::Array(items), Step::Item(_)) => {
            items.is_empty() || items.iter().all(|item| literal_lacks(item, rest))
        }
        (Expression::Map(entries), step) => match step.field() {
            Some(field) => entries
                .iter()
                .find(|(key, _)| *key == field)
                .is_none_or(|(_, value)| literal_lacks(value, rest)),
            None => {
                entries.is_empty() || entries.iter().all(|(_, value)| literal_lacks(value, rest))
            }
        },
        _ => false,
    }
}

/// The type `value` stores. A variable, an item, or a call depends on where
/// the write is, so it gives `Unknown`.
fn written_type(value: &Expression) -> CobbleType {
    match value {
        // `+` and `*` with text or a list join or repeat it.
        Expression::Binary(left, BinaryOp::Add | BinaryOp::Mul, right) => {
            match (written_type(left), written_type(right)) {
                (CobbleType::String, _) | (_, CobbleType::String) => CobbleType::String,
                (CobbleType::List, _) | (_, CobbleType::List) => CobbleType::List,
                _ => CobbleType::Integer,
            }
        }
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
/// What the literal of a variable says about a read of it.
pub(in crate::transpiler) struct Literal<'a> {
    /// The type the literal has where the read is.
    pub(in crate::transpiler) value_type: CobbleType,
    /// Whether the literal has nothing there, as an item of `[]`.
    pub(in crate::transpiler) lacks: bool,
    /// The literal itself.
    pub(in crate::transpiler) root: Option<&'a Expression>,
}

pub(in crate::transpiler) fn read_type(
    writes: &ContainerWrites,
    name: &str,
    steps: &[Step<'_>],
    literal: Literal<'_>,
    variable: &dyn Fn(&str, &[Step<'_>], &CobbleType) -> CobbleType,
    read: &dyn Fn(&Expression) -> CobbleType,
) -> CobbleType {
    let Literal {
        value_type: literal,
        lacks,
        root,
    } = literal;
    let Some(written) = writes.get(name) else {
        return literal;
    };
    // The type `value` reads as, where it is written at `path`. A read of
    // the place written, or of a place inside it, has the type of its
    // literal, as text does in `s = s[0] * 2`: any other type would come
    // from another write, which the read sees too.
    let typed = |value: &Expression, path: &[Option<String>]| -> CobbleType {
        let Some((source, source_steps)) = access_path(value) else {
            return CobbleType::Unknown;
        };
        let into_place = source == name
            && source_steps.len() >= path.len()
            && source_steps.iter().zip(path).all(|(step, field)| {
                field.is_none() || step.field().is_none() || step.field() == *field
            });
        if into_place {
            root.map_or(CobbleType::Unknown, |root| {
                literal_type(root, &source_steps, &written_type)
            })
        } else {
            read(value)
        }
    };
    // The type of an operand of `+` or `*`; a variable is taken for a
    // number, as written_type does.
    fn joined(
        value: &Expression,
        path: &[Option<String>],
        typed: &dyn Fn(&Expression, &[Option<String>]) -> CobbleType,
    ) -> CobbleType {
        match value {
            Expression::Binary(left, BinaryOp::Add | BinaryOp::Mul, right) => {
                match (joined(left, path, typed), joined(right, path, typed)) {
                    (CobbleType::String, _) | (_, CobbleType::String) => CobbleType::String,
                    (CobbleType::List, _) | (_, CobbleType::List) => CobbleType::List,
                    _ => CobbleType::Integer,
                }
            }
            Expression::Subscript(..) | Expression::Attribute(..) => typed(value, path),
            value => written_type(value),
        }
    }
    let fields: Vec<Option<String>> = steps.iter().map(|step| step.field()).collect();
    let mut types = written.iter().filter_map(|write| {
        if write.path.len() > fields.len() {
            return None;
        }
        let reaches = write
            .path
            .iter()
            .zip(&fields)
            .all(|(written, read)| written.is_none() || read.is_none() || written == read);
        if !reaches {
            return None;
        }
        let rest = &steps[write.path.len()..];
        // A literal with nothing there, as `m = {}`, writes nothing there.
        if lacks
            && write
                .value
                .as_ref()
                .is_some_and(|value| literal_lacks(value, rest))
        {
            return None;
        }
        Some(match &write.value {
            // A variable written there, as `cfg.s = t`, has the type it
            // holds where it is written.
            Some(Expression::Identifier(source)) => {
                let place = if rest.is_empty() {
                    literal.clone()
                } else {
                    CobbleType::Unknown
                };
                match variable(source, &steps[..write.path.len()], &place) {
                    whole if rest.is_empty() => whole,
                    // Every further index of text gives a character.
                    CobbleType::String if rest.iter().all(|step| matches!(step, Step::Item(_))) => {
                        CobbleType::String
                    }
                    _ => CobbleType::Unknown,
                }
            }
            // An item or a field copied there has the type it reads as.
            Some(value @ (Expression::Subscript(..) | Expression::Attribute(..))) => {
                typed(&extend(value, rest), &write.path)
            }
            // Text joined or repeated with an item or a field.
            Some(value @ Expression::Binary(_, BinaryOp::Add | BinaryOp::Mul, _)) => {
                match joined(value, &write.path, &typed) {
                    whole if rest.is_empty() => whole,
                    CobbleType::String if rest.iter().all(|step| matches!(step, Step::Item(_))) => {
                        CobbleType::String
                    }
                    _ => CobbleType::Unknown,
                }
            }
            Some(value) => literal_type(value, rest, &written_type),
            None => CobbleType::Unknown,
        })
    });
    // Nothing in the literal, as an item of `[]`: the writes decide when
    // they agree.
    if lacks && literal == CobbleType::Unknown {
        let Some(first) = types.next() else {
            return literal;
        };
        return if types.all(|other| other == first) {
            first
        } else {
            CobbleType::Unknown
        };
    }
    if types.any(|written| written != literal) {
        CobbleType::Unknown
    } else {
        literal
    }
}

/// `value` with `steps` read from it, as `cfg.x[1]` for `cfg.x` and `[1]`.
fn extend(value: &Expression, steps: &[Step<'_>]) -> Expression {
    steps.iter().fold(value.clone(), |base, step| match step {
        Step::Field(field) => Expression::Attribute(Box::new(base), field.to_string()),
        Step::Item(index) => Expression::Subscript(Box::new(base), Box::new((*index).clone())),
    })
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
