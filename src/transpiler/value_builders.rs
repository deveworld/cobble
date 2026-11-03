//! Materialize selectors, positions, paths, and text components as ordinary values.
use super::{TranspileError, Transpiler};
use crate::ast::{Assignment, BinaryOp, CobbleType, Expression, Statement};

fn text(value: &str) -> Expression {
    Expression::String(value.into())
}
fn join(parts: Vec<Expression>) -> Expression {
    parts
        .into_iter()
        .reduce(|left, right| Expression::Binary(Box::new(left), BinaryOp::Add, Box::new(right)))
        .unwrap_or_else(|| text(""))
}
fn shown(value: Expression) -> Expression {
    Expression::Call(
        Box::new(Expression::Identifier("_cobble_format".into())),
        vec![value],
    )
}

impl Transpiler {
    pub(super) fn value_builder_type(&self, func: &Expression) -> Option<CobbleType> {
        let Expression::Attribute(base, method) = func else {
            return None;
        };
        if self.is_list_value(base) {
            return None;
        }
        match (Self::expression_path(base)?.as_str(), method.as_str()) {
            ("text", "plain" | "colored" | "score" | "selector") => Some(CobbleType::Map),
            (
                "selector",
                "current" | "nearest_player" | "all_players" | "all_entities" | "tagged"
                | "players_tagged" | "entity_type",
            )
            | ("position", "here" | "absolute" | "relative" | "local")
            | ("storage", "path" | "child" | "index") => Some(CobbleType::String),
            _ => None,
        }
    }

    // Inline builders remain available to build-time resource declarations.
    pub(super) fn template_argument_needs_lowering(&self, value: &Expression) -> bool {
        match value {
            Expression::Call(func, args) if self.value_builder_type(func).is_some() => args
                .iter()
                .any(|arg| self.template_argument_needs_lowering(arg)),
            Expression::Array(values) => values
                .iter()
                .any(|value| self.template_argument_needs_lowering(value)),
            Expression::Map(entries) => entries
                .iter()
                .any(|(_, value)| self.template_argument_needs_lowering(value)),
            _ => self.needs_lowering(value),
        }
    }

    pub(super) fn lower_value_builder(
        &mut self,
        func: &Expression,
        args: &[Expression],
        out: &mut Vec<Statement>,
    ) -> Result<Option<Expression>, TranspileError> {
        if self.value_builder_type(func).is_none() {
            return Ok(None);
        }
        let Expression::Attribute(base, method) = func else {
            unreachable!()
        };
        let module = Self::expression_path(base).unwrap();
        self.require_stdlib_module(&module)?;
        let arity = match (module.as_str(), method.as_str()) {
            ("selector", "current" | "nearest_player" | "all_players" | "all_entities")
            | ("position", "here") => 0,
            ("position", _) => 3,
            ("text", "colored" | "score") | ("storage", "child" | "index") => 2,
            ("storage", "path") if !args.is_empty() => args.len(),
            _ => 1,
        };
        if args.len() != arity {
            return Err(format!("{module}.{method}() takes {arity} arguments").into());
        }
        if module == "storage" {
            for (index, arg) in args.iter().enumerate() {
                if method == "path" || method == "child" && index == 1 {
                    if let Some(value) = self.compile_time_text(arg) {
                        self.expr_to_storage_path_component(
                            &text(&value),
                            "storage path component",
                        )?;
                    }
                }
            }
            if method == "index"
                && matches!(
                    self.value_type(&args[1]),
                    CobbleType::Float
                        | CobbleType::Double
                        | CobbleType::String
                        | CobbleType::List
                        | CobbleType::Map
                )
            {
                return Err("storage.index() index must be an integer".into());
            }
        }
        if module == "text" && method == "colored" {
            if let Some(color) = self.compile_time_text(&args[1]) {
                super::intrinsics::text::validate_text_color(&color)?;
            }
        }
        // Capture inputs in order: a coordinate may be used both in a zero
        // test and in the output, and later arguments may mutate earlier ones.
        let mut values = Vec::new();
        for arg in args {
            let value = self.lower_value(arg, out)?;
            if matches!(
                value,
                Expression::String(_) | Expression::Number(_) | Expression::Boolean(_)
            ) {
                values.push(value);
            } else {
                let holder = self.hidden_local(self.value_type(arg));
                out.push(Statement::Assignment(Assignment {
                    target: holder.clone(),
                    target_expr: None,
                    value,
                    persist: false,
                }));
                values.push(Expression::Identifier(holder));
            }
        }
        let value = match module.as_str() {
            "selector" => match method.as_str() {
                "current" => text("@s"),
                "nearest_player" => text("@p"),
                "all_players" => text("@a"),
                "all_entities" => text("@e"),
                "tagged" | "players_tagged" => join(vec![
                    text(if method == "tagged" {
                        "@e[tag="
                    } else {
                        "@a[tag="
                    }),
                    shown(values.remove(0)),
                    text("]"),
                ]),
                "entity_type" => join(vec![text("@e[type="), shown(values.remove(0)), text("]")]),
                _ => unreachable!(),
            },
            "position" => {
                if method == "here" {
                    text("~ ~ ~")
                } else {
                    let prefix = match method.as_str() {
                        "relative" => "~",
                        "local" => "^",
                        _ => "",
                    };
                    let mut parts = Vec::new();
                    for coordinate in values {
                        if !parts.is_empty() {
                            parts.push(text(" "));
                        }
                        parts.push(if prefix.is_empty() {
                            shown(coordinate)
                        } else {
                            Expression::Conditional(
                                Box::new(Expression::Binary(
                                    Box::new(coordinate.clone()),
                                    BinaryOp::Eq,
                                    Box::new(Expression::Number(0.0)),
                                )),
                                Box::new(text(prefix)),
                                Box::new(join(vec![text(prefix), shown(coordinate)])),
                            )
                        });
                    }
                    join(parts)
                }
            }
            "storage" => match method.as_str() {
                "path" => {
                    let mut parts = Vec::new();
                    for part in values {
                        if !parts.is_empty() {
                            parts.push(text("."));
                        }
                        parts.push(shown(part));
                    }
                    join(parts)
                }
                "child" => join(vec![
                    shown(values.remove(0)),
                    text("."),
                    shown(values.remove(0)),
                ]),
                "index" => join(vec![
                    shown(values.remove(0)),
                    text("["),
                    shown(values.remove(0)),
                    text("]"),
                ]),
                _ => unreachable!(),
            },
            "text" => {
                let entries = match method.as_str() {
                    "plain" => vec![("text".into(), shown(values.remove(0)))],
                    "colored" => vec![
                        ("text".into(), shown(values.remove(0))),
                        ("color".into(), shown(values.remove(0))),
                    ],
                    "selector" => vec![("selector".into(), shown(values.remove(0)))],
                    "score" => vec![(
                        "score".into(),
                        Expression::Map(vec![
                            ("name".into(), shown(values.remove(0))),
                            ("objective".into(), shown(values.remove(0))),
                        ]),
                    )],
                    _ => unreachable!(),
                };
                Expression::Map(entries)
            }
            _ => unreachable!(),
        };
        self.lower_value(&value, out).map(Some)
    }
}
