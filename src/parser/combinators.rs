use super::tokenizer::{tokenize_spanned, SpannedToken, Token};
use crate::ast::*;
use chumsky::prelude::*;

const MAX_UNARY_OPERATORS: usize = 1024;

fn assignment_target_name(expr: &Expression) -> String {
    match expr {
        Expression::Identifier(name) => name.clone(),
        Expression::Attribute(base, attr) => format!("{}.{}", assignment_target_name(base), attr),
        Expression::Subscript(base, index) => {
            format!(
                "{}[{}]",
                assignment_target_name(base),
                assignment_target_name(index)
            )
        }
        Expression::Number(value) if value.fract() == 0.0 => (*value as i32).to_string(),
        Expression::Number(value) => value.to_string(),
        Expression::String(value) => format!("{value:?}"),
        Expression::Boolean(value) => value.to_string(),
        _ => "<expr>".to_string(),
    }
}

/// Lowercase Cobble literals that the tokenizer glued into an identifier
/// token (e.g. the selector `@s[nbt={x:True}]`, or `x:True` via the
/// namespace-separator rule) so re-stringified execute conditions emit
/// valid SNBT (`true`/`false`/`null`).
///
/// Only a `True`/`False`/`None` word directly following a `:` (the SNBT
/// key/value separator) outside of quoted strings is rewritten; selector
/// arguments like `tag=True` and quoted text are left untouched.
fn normalize_snbt_booleans(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.char_indices();
    let mut quote: Option<char> = None;
    let mut escaped = false;
    while let Some((index, ch)) = chars.next() {
        out.push(ch);
        if let Some(open) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == open {
                quote = None;
            }
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            ':' => {
                let rest = &text[index + ch.len_utf8()..];
                let word: String = rest
                    .chars()
                    .take_while(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                let replacement = match word.as_str() {
                    "True" => Some("true"),
                    "False" => Some("false"),
                    "None" => Some("null"),
                    _ => None,
                };
                if let Some(replacement) = replacement {
                    out.push_str(replacement);
                    for _ in 0..word.chars().count() {
                        chars.next();
                    }
                }
            }
            _ => {}
        }
    }
    out
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub span: SourceSpan,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LocatedProgram {
    pub program: Program,
    pub spans: ProgramSpanTable,
}

impl LocatedProgram {
    /// AST-shaped statement span tree aligned 1:1 with `program.statements`.
    ///
    /// `None` means the tree could not be aligned with the parsed program;
    /// consumers must fall back to other position sources instead of zipping
    /// the header-shaped `spans.statements` table positionally.
    pub fn statement_spans(&self) -> Option<&StatementSpanTree> {
        self.spans.statement_tree.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ProgramSpanTable {
    pub imports: Vec<SourceSpan>,
    pub statements: Vec<StatementSpan>,
    /// AST-shaped statement span tree (see [`StatementSpanTree`]). Only the
    /// top-level table returned by [`parse_located_with_diagnostics`] carries
    /// it; the header-shaped `statements` tree above is left untouched for
    /// its existing text-keyed consumers.
    pub statement_tree: Option<StatementSpanTree>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpan {
    pub span: SourceSpan,
    pub children: Vec<StatementSpan>,
}

/// Statement span tree mirroring `Program.statements` exactly: one node per
/// AST [`Statement`], in the same order and nesting. Unlike the header-shaped
/// [`StatementSpan`] table, `elif`/`else` headers are folded into their `If`
/// node and `case` headers into their `Match` node, so a consumer can walk
/// `Program.statements` and `StatementSpanTree.statements` in lockstep.
///
/// Top-level `import`/`from` headers are not part of the tree because
/// `Program.statements` excludes top-level imports; those spans stay in
/// `ProgramSpanTable.imports`, aligned 1:1 with `Program.imports`.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct StatementSpanTree {
    pub statements: Vec<StatementSpanNode>,
}

/// Tag naming the [`Statement`] variant a span node mirrors.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum StatementSpanKind {
    Import,
    FunctionDef,
    Assignment,
    ConstAssignment,
    Expression,
    If,
    For,
    While,
    Match,
    Return,
    Pass,
    MinecraftCommand,
    Global,
    Execute,
    SelectorDef,
    EntityDef,
    CreateEntity,
}

impl StatementSpanKind {
    /// The tag for an AST statement, for lockstep walks and alignment checks.
    pub fn of(statement: &Statement) -> Self {
        match statement {
            Statement::Import(_) => Self::Import,
            Statement::FunctionDef(_) => Self::FunctionDef,
            Statement::Assignment(_) => Self::Assignment,
            Statement::ConstAssignment(_) => Self::ConstAssignment,
            Statement::Expression(_) => Self::Expression,
            Statement::If(_) => Self::If,
            Statement::For(_) => Self::For,
            Statement::While(_) => Self::While,
            Statement::Match(_) => Self::Match,
            Statement::Return(_) => Self::Return,
            Statement::Pass => Self::Pass,
            Statement::MinecraftCommand(_) => Self::MinecraftCommand,
            Statement::Global(_) => Self::Global,
            Statement::Execute(_) => Self::Execute,
            Statement::SelectorDef(_) => Self::SelectorDef,
            Statement::EntityDef(_) => Self::EntityDef,
            Statement::CreateEntity(_) => Self::CreateEntity,
        }
    }
}

/// One span node per AST statement.
///
/// `span` covers the statement's header tokens: first through last token of
/// the header (for a multiline, bracket-continued header that is the whole
/// call; for `define ... create ... end` it is the whole construct). `span`'s
/// `line`/`column` are the header's first token position; `end_line` is the
/// line of the last header token. Body statements live in `children`
/// (`FunctionDef`/`For`/`While`/`Execute` bodies and an `If`'s then-branch);
/// an `If` additionally carries `elif_branches`/`else_branch`, and a `Match`
/// carries its arms exclusively in `cases` (its `children` stay empty). All
/// other kinds are leaves with every list empty.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpanNode {
    pub kind: StatementSpanKind,
    pub span: SourceSpan,
    pub end_line: usize,
    pub children: Vec<StatementSpanNode>,
    pub elif_branches: Vec<StatementSpanBranch>,
    pub else_branch: Option<StatementSpanBranch>,
    pub cases: Vec<StatementSpanBranch>,
}

impl StatementSpanNode {
    fn leaf(kind: StatementSpanKind, span: SourceSpan, end_line: usize) -> Self {
        Self {
            kind,
            span,
            end_line,
            children: Vec::new(),
            elif_branches: Vec::new(),
            else_branch: None,
            cases: Vec::new(),
        }
    }
}

/// An `elif`/`else`/`case` arm folded into its owning statement node.
/// `header` covers the arm's header tokens (`elif cond:` / `else:` /
/// `case pattern:`), `end_line` is the last header token's line, and
/// `children` mirrors the arm's body statements.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatementSpanBranch {
    pub header: SourceSpan,
    pub end_line: usize,
    pub children: Vec<StatementSpanNode>,
}

/// Token parser using chumsky
pub fn token_parser<'a>(
) -> impl Parser<'a, &'a [Token], Program, extra::Err<Rich<'a, Token>>> + Clone {
    token_parser_with_spans(&[])
}

/// Token parser that also knows each token's source span, so raw execute
/// conditions can keep tokens glued that were glued in the source
/// (`^1`, `stone[snowy=false]`, `ns:util/check`). With an empty `spans` slice
/// the parser falls back to token-based spacing.
fn token_parser_with_spans<'a>(
    spans: &'a [SpannedToken],
) -> impl Parser<'a, &'a [Token], Program, extra::Err<Rich<'a, Token>>> + Clone {
    // Whether the token at input index `index` ends exactly where the next
    // token starts (no whitespace between them in the source).
    let glued_to_next = move |index: usize| {
        spans
            .get(index)
            .zip(spans.get(index + 1))
            .is_some_and(|(token, next)| token.span.end == next.span.start)
    };
    recursive(|stmt| {
        // Expression parser
        let expr = recursive(|expr| {
            let array_literal = just(&Token::LBracket)
                .ignore_then(
                    expr.clone()
                        .separated_by(just(&Token::Comma))
                        .allow_trailing()
                        .collect(),
                )
                .then_ignore(just(&Token::RBracket))
                .map(Expression::Array);

            let map_entry = choice((
                select_ref! { Token::String(s) => s.clone() },
                select_ref! { Token::Ident(s) => s.clone() },
            ))
            .then_ignore(just(&Token::Colon))
            .then(expr.clone());

            let map_literal = just(&Token::LBrace)
                .ignore_then(
                    map_entry
                        .separated_by(just(&Token::Comma))
                        .allow_trailing()
                        .collect(),
                )
                .then_ignore(just(&Token::RBrace))
                .map(Expression::Map);

            let atom = choice((
                select_ref! {
                    Token::Number(n) => Expression::Number(n.parse().unwrap_or(0.0)),
                    Token::String(s) => Expression::String(s.clone()),
                    Token::True_ => Expression::Boolean(true),
                    Token::False_ => Expression::Boolean(false),
                    Token::None_ => Expression::None,
                    Token::Ident(s) => Expression::Identifier(s.clone()),
                },
                array_literal,
                map_literal,
                // Parenthesized expression
                just(&Token::LParen)
                    .ignore_then(expr.clone())
                    .then_ignore(just(&Token::RParen)),
            ));

            // Attribute access (e.g., stdlib.event)
            let postfix = atom.foldl(
                choice((
                    just(&Token::Dot)
                        .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                        .map(|attr| (true, Expression::Identifier(attr))), // true = dot access
                    just(&Token::LBracket)
                        .ignore_then(expr.clone())
                        .then_ignore(just(&Token::RBracket))
                        .map(|index| (false, index)), // false = bracket access
                ))
                .repeated(),
                |base, (is_dot, accessor)| {
                    if is_dot {
                        if let Expression::Identifier(attr) = accessor {
                            Expression::Attribute(Box::new(base), attr)
                        } else {
                            unreachable!()
                        }
                    } else {
                        Expression::Subscript(Box::new(base), Box::new(accessor))
                    }
                },
            );

            // Function call
            let call = postfix
                .then(
                    just(&Token::LParen)
                        .ignore_then(
                            expr.clone()
                                .separated_by(just(&Token::Comma))
                                .allow_trailing()
                                .collect(),
                        )
                        .then_ignore(just(&Token::RParen))
                        .or_not(),
                )
                .map(|(func, args)| {
                    if let Some(args) = args {
                        Expression::Call(Box::new(func), args)
                    } else {
                        func
                    }
                });

            // Binary operations with proper precedence
            // Unary +/- (high precedence, but lower than call, higher than power).
            // Parse leading unary operators iteratively instead of recursively so
            // attacker-controlled operator runs cannot consume one parser stack
            // frame per token. Keep an explicit limit to avoid constructing an
            // expression tree deep enough to overflow later recursive walks/drop.
            let unary = choice((
                just(&Token::Minus).to(UnaryOp::Neg),
                just(&Token::Plus).to(UnaryOp::Pos),
            ))
            .repeated()
            .at_most(MAX_UNARY_OPERATORS)
            .collect::<Vec<_>>()
            .then(call.clone())
            .map(|(ops, expr)| {
                ops.into_iter()
                    .rev()
                    .fold(expr, |expr, op| Expression::Unary(op, Box::new(expr)))
            })
            .boxed();

            // Highest precedence: ^ (power) - right-associative
            // Power operator is right-associative: 2^3^2 = 2^(3^2) = 512
            // We parse left-to-right but fold right-to-left for right-associativity
            let power = unary
                .clone()
                .then(
                    just(&Token::Caret)
                        .to(BinaryOp::Pow)
                        .then(unary.clone())
                        .repeated()
                        .collect::<Vec<_>>(),
                )
                .map(|(first, rest)| {
                    if rest.is_empty() {
                        first
                    } else {
                        // Collect all operands: [first, second, third, ...]
                        let mut all_operands = vec![first];
                        for (_, operand) in &rest {
                            all_operands.push(operand.clone());
                        }

                        // Build right-associative tree by folding from right to left
                        // For [a, b, c]: a ^ (b ^ c)
                        let mut result = all_operands.pop().unwrap();
                        while let Some(operand) = all_operands.pop() {
                            result = Expression::Binary(
                                Box::new(operand),
                                BinaryOp::Pow,
                                Box::new(result),
                            );
                        }
                        result
                    }
                });

            // Second highest: *, /, %
            let mul_div_mod = power.clone().foldl(
                choice((
                    just(&Token::Star).to(BinaryOp::Mul),
                    just(&Token::Slash).to(BinaryOp::Div),
                    just(&Token::Percent).to(BinaryOp::Mod),
                ))
                .then(power.clone())
                .repeated(),
                |left, (op, right)| Expression::Binary(Box::new(left), op, Box::new(right)),
            );

            // Middle precedence: + -
            let add_sub = mul_div_mod.clone().foldl(
                choice((
                    just(&Token::Plus).to(BinaryOp::Add),
                    just(&Token::Minus).to(BinaryOp::Sub),
                ))
                .then(mul_div_mod.clone())
                .repeated(),
                |left, (op, right)| Expression::Binary(Box::new(left), op, Box::new(right)),
            );

            // Comparisons
            let comparison = add_sub.clone().foldl(
                choice((
                    just(&Token::EqEq).to(BinaryOp::Eq),
                    just(&Token::NotEq).to(BinaryOp::NotEq),
                    just(&Token::GtEq).to(BinaryOp::GtEq),
                    just(&Token::LtEq).to(BinaryOp::LtEq),
                    just(&Token::Gt).to(BinaryOp::Gt),
                    just(&Token::Lt).to(BinaryOp::Lt),
                ))
                .then(add_sub.clone())
                .repeated(),
                |left, (op, right)| Expression::Binary(Box::new(left), op, Box::new(right)),
            );

            // Not (unary)
            let not_expr = just(&Token::Not)
                .repeated()
                .foldr(comparison.clone(), |_op, expr| {
                    Expression::Unary(UnaryOp::Not, Box::new(expr))
                })
                .or(comparison.clone());

            // And
            let and_expr = not_expr.clone().foldl(
                just(&Token::And)
                    .to(BinaryOp::And)
                    .then(not_expr.clone())
                    .repeated(),
                |left, (op, right)| Expression::Binary(Box::new(left), op, Box::new(right)),
            );

            // Or (lowest precedence)
            and_expr.clone().foldl(
                just(&Token::Or)
                    .to(BinaryOp::Or)
                    .then(and_expr.clone())
                    .repeated(),
                |left, (op, right)| Expression::Binary(Box::new(left), op, Box::new(right)),
            )
        });

        // Block: INDENT statements DEDENT
        let block = just(&Token::Indent)
            .ignore_then(stmt.clone().repeated().collect())
            .then_ignore(just(&Token::Dedent));

        // Minecraft command
        let minecraft_cmd = select_ref! {
            Token::MinecraftCommand(s) => Statement::MinecraftCommand(format!("/{}", s))
        };

        // Import
        let dotted_import_item = select_ref! { Token::Ident(s) => s.clone() }
            .then(
                just(&Token::Dot)
                    .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                    .repeated()
                    .collect::<Vec<_>>(),
            )
            .map(|(head, tail)| {
                if tail.is_empty() {
                    head
                } else {
                    format!("{}.{}", head, tail.join("."))
                }
            });
        let import = choice((
            just(&Token::Import)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .map(|module| {
                    Statement::Import(Import {
                        module,
                        items: vec![],
                    })
                }),
            just(&Token::From)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .then_ignore(just(&Token::Import))
                .then(
                    dotted_import_item
                        .separated_by(just(&Token::Comma))
                        .allow_trailing()
                        .collect(),
                )
                .map(|(module, items)| Statement::Import(Import { module, items })),
        ));

        // Global
        let global = just(&Token::Global)
            .ignore_then(
                select_ref! { Token::Ident(s) => s.clone() }
                    .separated_by(just(&Token::Comma))
                    .allow_trailing()
                    .collect(),
            )
            .map(Statement::Global);

        let assignment_target =
            select_ref! { Token::Ident(s) => Expression::Identifier(s.clone()) }.foldl(
                choice((
                    just(&Token::Dot)
                        .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                        .map(|attr| (true, Expression::Identifier(attr))),
                    just(&Token::LBracket)
                        .ignore_then(expr.clone())
                        .then_ignore(just(&Token::RBracket))
                        .map(|index| (false, index)),
                ))
                .repeated(),
                |base, (is_dot, accessor)| {
                    if is_dot {
                        if let Expression::Identifier(attr) = accessor {
                            Expression::Attribute(Box::new(base), attr)
                        } else {
                            unreachable!()
                        }
                    } else {
                        Expression::Subscript(Box::new(base), Box::new(accessor))
                    }
                },
            );

        // Assignment
        let assignment = assignment_target
            .clone()
            .then_ignore(just(&Token::Equals))
            .then(expr.clone())
            .map(|(target_expr, value)| {
                Statement::Assignment(Assignment {
                    target: assignment_target_name(&target_expr),
                    target_expr: Some(target_expr),
                    value,
                })
            });

        let compound_assignment_op = choice((
            just(&Token::Plus)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Add),
            just(&Token::Minus)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Sub),
            just(&Token::Star)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Mul),
            just(&Token::Slash)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Div),
            just(&Token::Percent)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Mod),
            just(&Token::Caret)
                .then_ignore(just(&Token::Equals))
                .to(BinaryOp::Pow),
        ));

        let compound_assignment = assignment_target
            .clone()
            .then(compound_assignment_op)
            .then(expr.clone())
            .map(|((target_expr, op), value)| {
                let target = assignment_target_name(&target_expr);
                Statement::Assignment(Assignment {
                    target,
                    target_expr: Some(target_expr.clone()),
                    value: Expression::Binary(Box::new(target_expr), op, Box::new(value)),
                })
            });

        // Const assignment
        let const_assignment = just(&Token::Const)
            .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
            .then_ignore(just(&Token::Equals))
            .then(expr.clone())
            .map(|(target, value)| Statement::ConstAssignment(ConstAssignment { target, value }));

        // Selector definition: @Name = @selector[...]
        let selector_def = select_ref! { Token::Ident(s) if s.starts_with('@') => s.clone() }
            .then_ignore(just(&Token::Equals))
            .then(select_ref! { Token::Ident(s) if s.starts_with('@') => s.clone() })
            .map(|(name_with_at, selector)| {
                // Strip @ from name (e.g., "@Player" -> "Player")
                let name = name_with_at
                    .strip_prefix('@')
                    .unwrap_or(&name_with_at)
                    .to_string();
                Statement::SelectorDef(SelectorDef { name, selector })
            });

        // Entity definition: define @Name = @Selector create { ... } end
        let entity_def = just(&Token::Define)
            .ignore_then(select_ref! { Token::Ident(s) if s.starts_with('@') => s.clone() })
            .then_ignore(just(&Token::Equals))
            .then(select_ref! { Token::Ident(s) if s.starts_with('@') => s.clone() })
            .then_ignore(just(&Token::Newline).or_not())
            .then_ignore(just(&Token::Create))
            .then(expr.clone())
            .then_ignore(just(&Token::Newline).or_not())
            .then_ignore(just(&Token::End))
            .map(|((name_with_at, selector), nbt)| {
                // Strip @ from name (e.g., "@Player" -> "Player")
                let name = name_with_at
                    .strip_prefix('@')
                    .unwrap_or(&name_with_at)
                    .to_string();
                Statement::EntityDef(EntityDef {
                    name,
                    selector,
                    nbt,
                })
            });

        // Create entity statement: create @Name
        let create_entity = just(&Token::Create)
            .ignore_then(select_ref! { Token::Ident(s) if s.starts_with('@') => s.clone() })
            .map(|name_with_at| {
                let name = name_with_at
                    .strip_prefix('@')
                    .unwrap_or(&name_with_at)
                    .to_string();
                Statement::CreateEntity(name)
            });

        // Pass
        let pass = just(&Token::Pass).to(Statement::Pass);

        // Return
        let return_stmt = just(&Token::Return)
            .ignore_then(expr.clone().or_not())
            .map(Statement::Return);

        // Docstring (string literal as statement)
        let docstring = select_ref! {
            Token::String(_s) => Statement::Pass  // Ignore docstrings
        };

        // Function definition
        let function = just(&Token::Def)
            .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
            .then(
                just(&Token::LParen)
                    .ignore_then(
                        select_ref! { Token::Ident(s) => s.clone() }
                            .separated_by(just(&Token::Comma))
                            .allow_trailing()
                            .collect::<Vec<String>>(),
                    )
                    .then_ignore(just(&Token::RParen)),
            )
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|((name, params), body)| {
                Statement::FunctionDef(FunctionDef {
                    name,
                    params: params
                        .into_iter()
                        .map(|p| Parameter {
                            name: p,
                            default: None,
                        })
                        .collect(),
                    body,
                    decorators: vec![],
                })
            });

        // If/elif/else
        let if_stmt = just(&Token::If)
            .ignore_then(expr.clone())
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .then(
                just(&Token::Elif)
                    .ignore_then(expr.clone())
                    .then_ignore(just(&Token::Colon))
                    .then_ignore(just(&Token::Newline).or_not())
                    .then(block.clone())
                    .repeated()
                    .collect(),
            )
            .then(
                just(&Token::Else)
                    .ignore_then(just(&Token::Colon))
                    .then_ignore(just(&Token::Newline).or_not())
                    .ignore_then(block.clone())
                    .or_not(),
            )
            .map(|(((condition, then_branch), elif_branches), else_branch)| {
                Statement::If(IfStatement {
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch,
                })
            });

        // For loop
        let for_loop = just(&Token::For)
            .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
            .then_ignore(just(&Token::In))
            .then(expr.clone())
            .then(just(&Token::By).ignore_then(expr.clone()).or_not())
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|(((target, iter), step), body)| {
                Statement::For(ForLoop {
                    target,
                    iter,
                    step,
                    body,
                })
            });

        // While loop
        let while_loop = just(&Token::While)
            .ignore_then(expr.clone())
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|(condition, body)| Statement::While(WhileLoop { condition, body }));

        // Match pattern
        // Case patterns must be integers; reject other numeric literals with a
        // proper parse error instead of panicking on `parse::<i32>().unwrap()`.
        let match_pattern_int = select_ref! { Token::Number(n) => n.clone() }.try_map(|n, span| {
            n.parse::<i32>().map_err(|_| {
                Rich::custom(
                    span,
                    format!("match case patterns must be integers, got {}", n),
                )
            })
        });
        let match_pattern = choice((
            // Wildcard: _
            just(&Token::Underscore).to(MatchPattern::Wildcard),
            // Range: expr to expr
            match_pattern_int
                .then(just(&Token::To).ignore_then(match_pattern_int).or_not())
                .map(|(start, end)| {
                    if let Some(end) = end {
                        MatchPattern::Range(start, end)
                    } else {
                        MatchPattern::Literal(start)
                    }
                }),
        ));

        // Match case
        let match_case = just(&Token::Case)
            .ignore_then(match_pattern)
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|(pattern, body)| MatchCase { pattern, body });

        // Match statement
        let match_stmt = just(&Token::Match)
            .ignore_then(expr.clone())
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline))
            .then_ignore(just(&Token::Indent))
            .then(match_case.repeated().at_least(1).collect())
            .then_ignore(just(&Token::Dedent))
            .map(|(value, cases)| Statement::Match(MatchStatement { value, cases }));

        // Execute block modifiers
        // Helper to parse execute condition (for if/unless modifiers)
        let is_execute_modifier_boundary = |t: &Token| {
            matches!(
                t,
                Token::Colon
                    | Token::Newline
                    | Token::As
                    | Token::At
                    | Token::If
                    | Token::Unless
                    | Token::In
            ) || matches!(
                t,
                Token::Ident(s)
                    if matches!(
                        s.as_str(),
                        "positioned" | "rotated" | "anchored" | "align" | "store"
                    )
            )
        };
        let exec_condition = any()
            .filter(move |t: &Token| !is_execute_modifier_boundary(t))
            .repeated()
            .at_least(1)
            .collect::<Vec<Token>>()
            .map_with(move |tokens, extra| {
                let span: SimpleSpan = extra.span();
                let first_index = span.start;
                // Convert tokens to string, but be smart about spacing
                let mut result = String::new();
                let mut prev_token: Option<&Token> = None;

                for (i, token) in tokens.iter().enumerate() {
                    // Raw conditions are re-stringified for Minecraft, so keyword
                    // literals must render as SNBT literals here. This is scoped
                    // to execute conditions on purpose: the global `Display` impl
                    // keeps the Cobble spellings (`True`/`False`/`None`).
                    // Identifiers may carry glued SNBT (e.g. `@s[nbt={x:True}]`,
                    // `x:True`), which gets the same lowering.
                    let token_str = match token {
                        Token::True_ => "true".to_string(),
                        Token::False_ => "false".to_string(),
                        Token::None_ => "null".to_string(),
                        Token::Ident(s) => normalize_snbt_booleans(s),
                        _ => format!("{}", token),
                    };

                    // Determine if we need a space before this token. Every arm
                    // is deliberate — raw conditions must survive
                    // re-stringification unchanged:
                    let is_comparison = |token: &Token| {
                        matches!(
                            token,
                            Token::EqEq
                                | Token::NotEq
                                | Token::Lt
                                | Token::LtEq
                                | Token::Gt
                                | Token::GtEq
                        )
                    };
                    let need_space = if i == 0 {
                        false
                    } else if let Some(prev) = prev_token {
                        match (prev, token) {
                            // Python comparisons stay spaced so the raw
                            // condition is still recognized and re-parsed as
                            // a Python expression (`x>1` -> `x > 1`).
                            (prev, token) if is_comparison(prev) || is_comparison(token) => true,
                            // Tokens glued in the source stay glued: local
                            // coordinates (`^1`), block states and NBT paths
                            // (`stone[snowy=false]`, `Items[0].id`), and
                            // resource paths (`ns:util/check`).
                            _ if glued_to_next(first_index + i - 1) => false,
                            // Range operator "..": both dots stay glued ("1..")
                            (Token::Dot, Token::Dot) => false,
                            // Range end hugs the dots ("..10", "1..5")
                            (Token::Dot, Token::Number(_)) => false,
                            // A dot before an identifier is a path separator in
                            // NBT/storage/resource paths ("path.sub",
                            // "ns:foo.bar"), never a token break — no space.
                            (Token::Dot, Token::Ident(_)) => false,
                            // A dot before any other token (an operator, brace,
                            // ...) is not part of a path or range; keep a space.
                            (Token::Dot, _) => true,
                            // Never a space before a dot ("path.", "1..")
                            (_, Token::Dot) => false,
                            // Negative numbers: minus glues to its number ("-5")
                            (Token::Minus, Token::Number(_)) => false,
                            // Default: separate tokens with a single space
                            _ => true,
                        }
                    } else {
                        true
                    };

                    if need_space {
                        result.push(' ');
                    }
                    result.push_str(&token_str);
                    prev_token = Some(token);
                }
                result
            });

        // Helper: parse either an identifier or a macro parameter {name}
        let selector_or_macro = choice((
            select_ref! { Token::Ident(s) => s.clone() },
            just(&Token::LBrace)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .then_ignore(just(&Token::RBrace))
                .map(|name| format!("{{{}}}", name)),
        ));

        let execute_modifier = choice((
            just(&Token::As)
                .ignore_then(selector_or_macro)
                .map(ExecuteModifier::As),
            just(&Token::At)
                .ignore_then(selector_or_macro)
                .map(ExecuteModifier::At),
            // if/unless in execute blocks:
            // For now, keep as raw and let transpiler determine if it's Python expression
            just(&Token::If)
                .ignore_then(exec_condition)
                .map(ExecuteModifier::IfRaw),
            just(&Token::Unless)
                .ignore_then(exec_condition)
                .map(ExecuteModifier::UnlessRaw),
            // positioned <coords>
            select_ref! { Token::Ident(s) if s == "positioned" => s.clone() }
                .ignore_then(exec_condition)
                .map(ExecuteModifier::Positioned),
            // rotated <rotation>
            select_ref! { Token::Ident(s) if s == "rotated" => s.clone() }
                .ignore_then(exec_condition)
                .map(ExecuteModifier::Rotated),
            // in <dimension>
            just(&Token::In)
                .ignore_then(exec_condition)
                .map(ExecuteModifier::In),
            // anchored <eyes|feet>
            select_ref! { Token::Ident(s) if s == "anchored" => s.clone() }
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .map(ExecuteModifier::Anchored),
            // align <axes>
            select_ref! { Token::Ident(s) if s == "align" => s.clone() }
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .map(ExecuteModifier::Align),
            // store result/success ...
            select_ref! { Token::Ident(s) if s == "store" => s.clone() }
                .ignore_then(exec_condition)
                .map(ExecuteModifier::Store),
        ));

        // Execute block - support all execute modifiers
        let execute_block = choice((
            // asat @s: -> as @s at @s:
            just(&Token::Asat)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .try_map(|s, span| {
                    if s.starts_with('@') {
                        Ok(s)
                    } else {
                        Err(Rich::custom(
                            span,
                            format!("Expected selector starting with '@', got '{}'", s),
                        ))
                    }
                })
                .then_ignore(just(&Token::Colon))
                .then_ignore(just(&Token::Newline).or_not())
                .then(block.clone())
                .map(|(selector, body)| {
                    Statement::Execute(ExecuteBlock {
                        modifiers: vec![
                            ExecuteModifier::As(selector),
                            ExecuteModifier::At("@s".to_string()),
                        ],
                        body,
                    })
                }),
            // Any execute modifier(s) followed by colon - supports positioned, in, etc. as first modifier
            execute_modifier
                .then(
                    execute_modifier
                        .repeated()
                        .collect::<Vec<ExecuteModifier>>(),
                )
                .then_ignore(just(&Token::Colon))
                .then_ignore(just(&Token::Newline).or_not())
                .then(block.clone())
                .map(|((first, rest), body)| {
                    let mut modifiers = vec![first];
                    modifiers.extend(rest);
                    Statement::Execute(ExecuteBlock { modifiers, body })
                }),
        ));

        // Expression statement (for function calls)
        let expr_stmt = expr.clone().map(Statement::Expression);

        // Simple statement (ends with newline)
        let simple_stmt = choice((
            docstring,
            minecraft_cmd,
            import,
            global,
            return_stmt,
            pass,
            selector_def,
            entity_def,
            create_entity,
            const_assignment,
            compound_assignment,
            assignment,
            expr_stmt,
        ))
        .then_ignore(just(&Token::Newline).or_not());

        // Compound statement (has block)
        let compound_stmt = choice((
            function,
            if_stmt,
            for_loop,
            while_loop,
            match_stmt,
            execute_block,
        ));

        choice((compound_stmt, simple_stmt))
    })
    .repeated()
    .collect()
    .then_ignore(just(&Token::Eof))
    .map(|statements: Vec<Statement>| Program {
        imports: statements
            .iter()
            .filter_map(|s| {
                if let Statement::Import(imp) = s {
                    Some(imp.clone())
                } else {
                    None
                }
            })
            .collect(),
        statements: statements
            .into_iter()
            .filter(|s| !matches!(s, Statement::Import(_)))
            .collect(),
    })
}

/// Parse source code into AST
pub fn parse(source: &str) -> Result<Program, Vec<String>> {
    parse_with_diagnostics(source).map_err(|errors| {
        errors
            .into_iter()
            .map(|error| error.message)
            .collect::<Vec<_>>()
    })
}

/// Parse source code into AST with source spans for parser diagnostics.
pub fn parse_with_diagnostics(source: &str) -> Result<Program, Vec<ParseDiagnostic>> {
    let spanned_tokens = tokenize_spanned(source).map_err(|error| {
        vec![ParseDiagnostic {
            span: SourceSpan::new(0, 0, 1, 1),
            message: error,
        }]
    })?;
    let tokens = spanned_tokens
        .iter()
        .map(|token| token.token.clone())
        .collect::<Vec<_>>();
    let result = token_parser_with_spans(&spanned_tokens).parse(&tokens);

    match result.into_result() {
        Ok(program) => Ok(program),
        Err(errors) => Err(errors
            .into_iter()
            .map(|error| ParseDiagnostic {
                span: parse_error_source_span(error.span().start, &spanned_tokens),
                message: format!("{}", error.reason()),
            })
            .collect()),
    }
}

/// Parse source code into AST plus a non-breaking source provenance sidecar.
pub fn parse_located_with_diagnostics(
    source: &str,
) -> Result<LocatedProgram, Vec<ParseDiagnostic>> {
    let spanned_tokens = tokenize_spanned(source).map_err(|error| {
        vec![ParseDiagnostic {
            span: SourceSpan::new(0, 0, 1, 1),
            message: error,
        }]
    })?;
    let tokens = spanned_tokens
        .iter()
        .map(|token| token.token.clone())
        .collect::<Vec<_>>();
    let result = token_parser_with_spans(&spanned_tokens).parse(&tokens);

    match result.into_result() {
        Ok(program) => {
            let mut spans = collect_program_span_table(&spanned_tokens);
            // Alignment contract: the tree must mirror the parsed program
            // exactly. `build_statement_span_tree` returns `None` on any
            // token-cursor desync, and `verify_span_tree_alignment` re-checks
            // the shape independently; on mismatch the tree is dropped so
            // consumers fall back instead of misattributing positions.
            spans.statement_tree = build_statement_span_tree(&program, &spanned_tokens)
                .filter(|tree| verify_span_tree_alignment(&program, tree).is_ok());
            Ok(LocatedProgram { program, spans })
        }
        Err(errors) => Err(errors
            .into_iter()
            .map(|error| ParseDiagnostic {
                span: parse_error_source_span(error.span().start, &spanned_tokens),
                message: format!("{}", error.reason()),
            })
            .collect()),
    }
}

fn parse_error_source_span(token_index: usize, tokens: &[SpannedToken]) -> SourceSpan {
    tokens
        .get(token_index)
        .or_else(|| {
            token_index
                .checked_sub(1)
                .and_then(|index| tokens.get(index))
        })
        .map(|token| token.span)
        .unwrap_or_else(|| SourceSpan::new(0, 0, 1, 1))
}

fn collect_program_span_table(tokens: &[SpannedToken]) -> ProgramSpanTable {
    let mut index = 0;
    collect_span_block(tokens, &mut index, true)
}

fn collect_span_block(
    tokens: &[SpannedToken],
    index: &mut usize,
    top_level: bool,
) -> ProgramSpanTable {
    let mut table = ProgramSpanTable::default();

    while let Some(token) = tokens.get(*index) {
        match token.token {
            Token::Newline => {
                *index += 1;
            }
            Token::Dedent | Token::Eof => break,
            Token::Indent => {
                *index += 1;
            }
            _ => {
                let starts_top_level_import =
                    top_level && matches!(token.token, Token::Import | Token::From);
                let span = collect_statement_header_span(tokens, index);
                let children = if matches!(
                    tokens.get(*index).map(|token| &token.token),
                    Some(Token::Indent)
                ) {
                    *index += 1;
                    let child_table = collect_span_block(tokens, index, false);
                    if matches!(
                        tokens.get(*index).map(|token| &token.token),
                        Some(Token::Dedent)
                    ) {
                        *index += 1;
                    }
                    child_table.statements
                } else {
                    Vec::new()
                };

                if starts_top_level_import {
                    table.imports.push(span);
                } else {
                    table.statements.push(StatementSpan { span, children });
                }
            }
        }
    }

    table
}

fn collect_statement_header_span(tokens: &[SpannedToken], index: &mut usize) -> SourceSpan {
    let start = tokens
        .get(*index)
        .map(|token| token.span)
        .unwrap_or_else(|| SourceSpan::new(0, 0, 1, 1));
    let mut end = start;

    while let Some(token) = tokens.get(*index) {
        match token.token {
            Token::Newline | Token::Indent | Token::Dedent | Token::Eof => break,
            _ => {
                end = token.span;
                *index += 1;
            }
        }
    }

    if matches!(
        tokens.get(*index).map(|token| &token.token),
        Some(Token::Newline)
    ) {
        *index += 1;
    }

    SourceSpan::new(start.start, end.end, start.line, start.column)
}

/// Build the AST-shaped statement span tree from the spanned token stream.
///
/// The walk is guided by the parsed `Program` so the tree has exactly one
/// node per AST statement by construction: the token cursor advances over
/// each statement's tokens using the same segmentation the grammar applies
/// (greedy expression extents, `elif`/`else` headers folded into their `if`,
/// `case` headers folded into their `match`, `define ... create ... end` as
/// one header, top-level `import`/`from` headers skipped because
/// `Program.statements` excludes them). Comments and blank lines produce no
/// tokens and need no handling; bracket-suppressed newlines make a multiline
/// call one header. Returns `None` if the cursor ever desyncs from the AST
/// (a builder bug or an unmodeled grammar shape) so callers degrade instead
/// of misattributing spans.
fn build_statement_span_tree(
    program: &Program,
    tokens: &[SpannedToken],
) -> Option<StatementSpanTree> {
    let mut builder = SpanTreeBuilder { tokens, index: 0 };
    let statements = builder.build_block(&program.statements, true)?;
    builder.skip_newlines();
    match builder.peek() {
        Some(Token::Eof) | None => Some(StatementSpanTree { statements }),
        Some(_) => None,
    }
}

/// Check that a statement span tree has exactly the shape of the parsed
/// program: same statement count at every level, same variant tag per node,
/// same `elif`/`else` branch counts per `If`, same case count per `Match`.
/// Returns the first mismatch as a path-annotated message. Mismatches are
/// builder bugs; `parse_located_with_diagnostics` drops the tree when this
/// fails so consumers fall back gracefully.
pub fn verify_span_tree_alignment(
    program: &Program,
    tree: &StatementSpanTree,
) -> Result<(), String> {
    verify_span_nodes(&program.statements, &tree.statements, "program")
}

fn verify_span_nodes(
    statements: &[Statement],
    nodes: &[StatementSpanNode],
    path: &str,
) -> Result<(), String> {
    if statements.len() != nodes.len() {
        return Err(format!(
            "{path}: {} statements but {} span nodes",
            statements.len(),
            nodes.len()
        ));
    }

    for (index, (statement, node)) in statements.iter().zip(nodes).enumerate() {
        let path = format!("{path}[{index}]");
        let kind = StatementSpanKind::of(statement);
        if node.kind != kind {
            return Err(format!(
                "{path}: statement is {kind:?} but span node is {:?}",
                node.kind
            ));
        }

        match statement {
            Statement::If(if_statement) => {
                if node.elif_branches.len() != if_statement.elif_branches.len() {
                    return Err(format!(
                        "{path}: {} elif branches but {} span branches",
                        if_statement.elif_branches.len(),
                        node.elif_branches.len()
                    ));
                }
                if node.else_branch.is_some() != if_statement.else_branch.is_some() {
                    return Err(format!("{path}: else branch presence mismatch"));
                }
                if !node.cases.is_empty() {
                    return Err(format!("{path}: If span node carries match cases"));
                }
                verify_span_nodes(
                    &if_statement.then_branch,
                    &node.children,
                    &format!("{path}.then"),
                )?;
                for (branch_index, ((_, body), branch)) in if_statement
                    .elif_branches
                    .iter()
                    .zip(&node.elif_branches)
                    .enumerate()
                {
                    verify_span_nodes(
                        body,
                        &branch.children,
                        &format!("{path}.elif[{branch_index}]"),
                    )?;
                }
                if let (Some(body), Some(branch)) = (&if_statement.else_branch, &node.else_branch) {
                    verify_span_nodes(body, &branch.children, &format!("{path}.else"))?;
                }
            }
            Statement::Match(match_statement) => {
                if node.cases.len() != match_statement.cases.len() {
                    return Err(format!(
                        "{path}: {} match cases but {} span cases",
                        match_statement.cases.len(),
                        node.cases.len()
                    ));
                }
                verify_leaf_shape(node, &path, true)?;
                for (case_index, (case, branch)) in
                    match_statement.cases.iter().zip(&node.cases).enumerate()
                {
                    verify_span_nodes(
                        &case.body,
                        &branch.children,
                        &format!("{path}.case[{case_index}]"),
                    )?;
                }
            }
            Statement::FunctionDef(function) => {
                verify_body_shape(&function.body, node, &path)?;
            }
            Statement::For(for_loop) => verify_body_shape(&for_loop.body, node, &path)?,
            Statement::While(while_loop) => verify_body_shape(&while_loop.body, node, &path)?,
            Statement::Execute(execute) => verify_body_shape(&execute.body, node, &path)?,
            _ => {
                verify_leaf_shape(node, &path, false)?;
                if !node.children.is_empty() {
                    return Err(format!("{path}: leaf span node has children"));
                }
            }
        }
    }

    Ok(())
}

fn verify_body_shape(
    body: &[Statement],
    node: &StatementSpanNode,
    path: &str,
) -> Result<(), String> {
    verify_leaf_shape(node, path, false)?;
    verify_span_nodes(body, &node.children, path)
}

fn verify_leaf_shape(
    node: &StatementSpanNode,
    path: &str,
    allow_cases: bool,
) -> Result<(), String> {
    if !node.elif_branches.is_empty() || node.else_branch.is_some() {
        return Err(format!("{path}: span node carries stray if branches"));
    }
    if !allow_cases && !node.cases.is_empty() {
        return Err(format!("{path}: span node carries stray match cases"));
    }
    if allow_cases && !node.children.is_empty() {
        return Err(format!("{path}: Match span node has direct children"));
    }
    Ok(())
}

struct SpanTreeBuilder<'a> {
    tokens: &'a [SpannedToken],
    index: usize,
}

impl<'a> SpanTreeBuilder<'a> {
    fn peek(&self) -> Option<&'a Token> {
        self.tokens.get(self.index).map(|token| &token.token)
    }

    fn peek_at(&self, offset: usize) -> Option<&'a Token> {
        self.tokens
            .get(self.index + offset)
            .map(|token| &token.token)
    }

    fn expect(&mut self, predicate: impl Fn(&Token) -> bool) -> Option<&'a SpannedToken> {
        let token = self.tokens.get(self.index)?;
        if predicate(&token.token) {
            self.index += 1;
            Some(token)
        } else {
            None
        }
    }

    fn bump_if(&mut self, predicate: impl Fn(&Token) -> bool) {
        if self.peek().is_some_and(predicate) {
            self.index += 1;
        }
    }

    fn skip_newlines(&mut self) {
        while matches!(self.peek(), Some(Token::Newline)) {
            self.index += 1;
        }
    }

    /// Span covering `tokens[start_index..self.index]`, plus the last
    /// token's line. `line`/`column` come from the first token.
    fn span_from(&self, start_index: usize) -> Option<(SourceSpan, usize)> {
        if self.index <= start_index {
            return None;
        }
        let first = self.tokens.get(start_index)?;
        let last = self.tokens.get(self.index - 1)?;
        Some((
            SourceSpan::new(
                first.span.start,
                last.span.end,
                first.span.line,
                first.span.column,
            ),
            last.span.line,
        ))
    }

    /// Consume header tokens up to (not including) the next
    /// `Newline`/`Indent`/`Dedent`/`Eof`. Bracket-suppressed newlines keep a
    /// multiline call inside one header.
    fn header_span_to_newline(&mut self) -> Option<(SourceSpan, usize)> {
        let start_index = self.index;
        while let Some(token) = self.peek() {
            match token {
                Token::Newline | Token::Indent | Token::Dedent | Token::Eof => break,
                _ => self.index += 1,
            }
        }
        self.span_from(start_index)
    }

    fn build_block(
        &mut self,
        statements: &[Statement],
        top_level: bool,
    ) -> Option<Vec<StatementSpanNode>> {
        let mut nodes = Vec::new();
        let mut remaining = statements.iter();
        loop {
            self.skip_newlines();
            match self.peek() {
                None | Some(Token::Dedent | Token::Eof) => break,
                // A block can only start after a header the caller consumed.
                Some(Token::Indent) => return None,
                Some(Token::Import | Token::From) if top_level => {
                    // Top-level import headers live in
                    // `ProgramSpanTable.imports` and have no counterpart in
                    // `Program.statements` (token_parser filters them out).
                    self.header_span_to_newline()?;
                }
                Some(_) => {
                    let statement = remaining.next()?;
                    nodes.push(self.build_statement(statement)?);
                }
            }
        }
        // Both sides must run out together.
        if remaining.next().is_some() {
            return None;
        }
        Some(nodes)
    }

    fn build_statement(&mut self, statement: &Statement) -> Option<StatementSpanNode> {
        let kind = StatementSpanKind::of(statement);
        let start_index = self.index;
        match statement {
            // A `Pass` comes from the `pass` keyword or from a docstring
            // string token. One source line may hold several string tokens
            // (`"""docs"""` tokenizes as three), each its own statement, so
            // consume exactly one token instead of the rest of the line.
            Statement::Pass => {
                self.expect(|token| matches!(token, Token::Pass | Token::String(_)))?;
                self.leaf_from(kind, start_index)
            }
            Statement::MinecraftCommand(_) => {
                self.expect(|token| matches!(token, Token::MinecraftCommand(_)))?;
                self.leaf_from(kind, start_index)
            }
            Statement::Expression(_) => {
                self.scan_expression()?;
                self.leaf_from(kind, start_index)
            }
            Statement::Assignment(_) => {
                self.scan_assignment()?;
                self.leaf_from(kind, start_index)
            }
            Statement::ConstAssignment(_) => {
                self.expect(|token| matches!(token, Token::Const))?;
                self.expect(|token| matches!(token, Token::Ident(_)))?;
                self.expect(|token| matches!(token, Token::Equals))?;
                self.scan_expression()?;
                self.leaf_from(kind, start_index)
            }
            Statement::Return(value) => {
                self.expect(|token| matches!(token, Token::Return))?;
                if value.is_some() {
                    self.scan_expression()?;
                }
                self.leaf_from(kind, start_index)
            }
            Statement::Global(_) => {
                self.expect(|token| matches!(token, Token::Global))?;
                self.scan_ident_list();
                self.leaf_from(kind, start_index)
            }
            Statement::Import(_) => {
                self.scan_import()?;
                self.leaf_from(kind, start_index)
            }
            Statement::SelectorDef(_) => {
                self.expect(|token| matches!(token, Token::Ident(name) if name.starts_with('@')))?;
                self.expect(|token| matches!(token, Token::Equals))?;
                self.expect(|token| matches!(token, Token::Ident(name) if name.starts_with('@')))?;
                self.leaf_from(kind, start_index)
            }
            Statement::CreateEntity(_) => {
                self.expect(|token| matches!(token, Token::Create))?;
                self.expect(|token| matches!(token, Token::Ident(name) if name.starts_with('@')))?;
                self.leaf_from(kind, start_index)
            }
            // `define @Name = @Sel [\n] create <expr> [\n] end` is one AST
            // statement spanning several source lines; its header span covers
            // `define` through `end`.
            Statement::EntityDef(_) => {
                self.expect(|token| matches!(token, Token::Define))?;
                self.expect(|token| matches!(token, Token::Ident(name) if name.starts_with('@')))?;
                self.expect(|token| matches!(token, Token::Equals))?;
                self.expect(|token| matches!(token, Token::Ident(name) if name.starts_with('@')))?;
                self.bump_if(|token| matches!(token, Token::Newline));
                self.expect(|token| matches!(token, Token::Create))?;
                self.scan_expression()?;
                self.bump_if(|token| matches!(token, Token::Newline));
                self.expect(|token| matches!(token, Token::End))?;
                self.leaf_from(kind, start_index)
            }
            Statement::FunctionDef(function) => self.build_body_statement(kind, &function.body),
            Statement::For(for_loop) => self.build_body_statement(kind, &for_loop.body),
            Statement::While(while_loop) => self.build_body_statement(kind, &while_loop.body),
            Statement::Execute(execute) => self.build_body_statement(kind, &execute.body),
            Statement::If(if_statement) => self.build_if_statement(if_statement),
            Statement::Match(match_statement) => self.build_match_statement(match_statement),
        }
    }

    fn leaf_from(
        &mut self,
        kind: StatementSpanKind,
        start_index: usize,
    ) -> Option<StatementSpanNode> {
        let (span, end_line) = self.span_from(start_index)?;
        Some(StatementSpanNode::leaf(kind, span, end_line))
    }

    fn build_body_statement(
        &mut self,
        kind: StatementSpanKind,
        body: &[Statement],
    ) -> Option<StatementSpanNode> {
        let (span, end_line) = self.header_span_to_newline()?;
        let children = self.expect_indented_block(body)?;
        let mut node = StatementSpanNode::leaf(kind, span, end_line);
        node.children = children;
        Some(node)
    }

    fn build_if_statement(&mut self, if_statement: &IfStatement) -> Option<StatementSpanNode> {
        let (span, end_line) = self.header_span_to_newline()?;
        let children = self.expect_indented_block(&if_statement.then_branch)?;
        let mut elif_branches = Vec::new();
        for (_, body) in &if_statement.elif_branches {
            self.skip_newlines();
            if !matches!(self.peek(), Some(Token::Elif)) {
                return None;
            }
            elif_branches.push(self.build_branch(body)?);
        }
        let else_branch = if let Some(body) = &if_statement.else_branch {
            self.skip_newlines();
            if !matches!(self.peek(), Some(Token::Else)) {
                return None;
            }
            Some(self.build_branch(body)?)
        } else {
            None
        };
        let mut node = StatementSpanNode::leaf(StatementSpanKind::If, span, end_line);
        node.children = children;
        node.elif_branches = elif_branches;
        node.else_branch = else_branch;
        Some(node)
    }

    fn build_match_statement(
        &mut self,
        match_statement: &MatchStatement,
    ) -> Option<StatementSpanNode> {
        let (span, end_line) = self.header_span_to_newline()?;
        self.skip_newlines();
        self.expect(|token| matches!(token, Token::Indent))?;
        let mut cases = Vec::new();
        for case in &match_statement.cases {
            self.skip_newlines();
            if !matches!(self.peek(), Some(Token::Case)) {
                return None;
            }
            cases.push(self.build_branch(&case.body)?);
        }
        self.skip_newlines();
        self.expect(|token| matches!(token, Token::Dedent))?;
        let mut node = StatementSpanNode::leaf(StatementSpanKind::Match, span, end_line);
        node.cases = cases;
        Some(node)
    }

    /// One `elif`/`else`/`case` arm: header line plus indented body.
    fn build_branch(&mut self, body: &[Statement]) -> Option<StatementSpanBranch> {
        let (header, end_line) = self.header_span_to_newline()?;
        let children = self.expect_indented_block(body)?;
        Some(StatementSpanBranch {
            header,
            end_line,
            children,
        })
    }

    fn expect_indented_block(
        &mut self,
        statements: &[Statement],
    ) -> Option<Vec<StatementSpanNode>> {
        self.skip_newlines();
        self.expect(|token| matches!(token, Token::Indent))?;
        let children = self.build_block(statements, false)?;
        self.expect(|token| matches!(token, Token::Dedent))?;
        Some(children)
    }

    /// `import module` or `from module import a.b, c` (trailing comma
    /// allowed), mirroring the grammar's import parsers.
    fn scan_import(&mut self) -> Option<()> {
        match self.peek() {
            Some(Token::Import) => {
                self.index += 1;
                self.expect(|token| matches!(token, Token::Ident(_)))?;
            }
            Some(Token::From) => {
                self.index += 1;
                self.expect(|token| matches!(token, Token::Ident(_)))?;
                self.expect(|token| matches!(token, Token::Import))?;
                while matches!(self.peek(), Some(Token::Ident(_))) {
                    self.index += 1;
                    while matches!(self.peek(), Some(Token::Dot))
                        && matches!(self.peek_at(1), Some(Token::Ident(_)))
                    {
                        self.index += 2;
                    }
                    if matches!(self.peek(), Some(Token::Comma)) {
                        self.index += 1;
                    } else {
                        break;
                    }
                }
            }
            _ => return None,
        }
        Some(())
    }

    /// Comma-separated identifier list with optional trailing comma
    /// (`global a, b,`). Zero items is grammatically valid.
    fn scan_ident_list(&mut self) {
        while matches!(self.peek(), Some(Token::Ident(_))) {
            self.index += 1;
            if matches!(self.peek(), Some(Token::Comma)) {
                self.index += 1;
            } else {
                break;
            }
        }
    }

    /// `target = expr` or `target <op>= expr`, mirroring the grammar's
    /// assignment/compound-assignment parsers.
    fn scan_assignment(&mut self) -> Option<()> {
        self.scan_assignment_target()?;
        match self.peek() {
            Some(Token::Equals) => self.index += 1,
            Some(
                Token::Plus
                | Token::Minus
                | Token::Star
                | Token::Slash
                | Token::Percent
                | Token::Caret,
            ) if matches!(self.peek_at(1), Some(Token::Equals)) => self.index += 2,
            _ => return None,
        }
        self.scan_expression()
    }

    /// Assignment target: identifier plus `.ident` / `[index]` accessor
    /// chains (no call groups, matching the grammar's `assignment_target`).
    fn scan_assignment_target(&mut self) -> Option<()> {
        self.expect(|token| matches!(token, Token::Ident(_)))?;
        self.scan_postfix_accessors()
    }

    /// Greedy expression extent mirroring the grammar's precedence chain.
    /// Correct for token streams that parsed successfully: chumsky's
    /// combinators are greedy, so after a binary operator a valid operand
    /// always follows, and expressions never cross `Newline` tokens.
    fn scan_expression(&mut self) -> Option<()> {
        self.scan_operand()?;
        while matches!(
            self.peek(),
            Some(
                Token::Plus
                    | Token::Minus
                    | Token::Star
                    | Token::Slash
                    | Token::Percent
                    | Token::Caret
                    | Token::EqEq
                    | Token::NotEq
                    | Token::Lt
                    | Token::LtEq
                    | Token::Gt
                    | Token::GtEq
                    | Token::And
                    | Token::Or
            )
        ) {
            self.index += 1;
            self.scan_operand()?;
        }
        Some(())
    }

    /// One operand: unary prefixes, an atom, `.ident`/`[index]` accessor
    /// chains, then at most one call-argument group — the grammar's `call`
    /// rule allows a single parenthesized group after the postfix chain.
    fn scan_operand(&mut self) -> Option<()> {
        while matches!(self.peek(), Some(Token::Not | Token::Minus | Token::Plus)) {
            self.index += 1;
        }
        match self.peek() {
            Some(
                Token::Number(_)
                | Token::String(_)
                | Token::True_
                | Token::False_
                | Token::None_
                | Token::Ident(_),
            ) => self.index += 1,
            Some(Token::LParen | Token::LBracket | Token::LBrace) => self.scan_balanced_group()?,
            _ => return None,
        }
        self.scan_postfix_accessors()?;
        if matches!(self.peek(), Some(Token::LParen)) {
            self.scan_balanced_group()?;
        }
        Some(())
    }

    /// `.ident` and `[index]` accessor chains. A `.` not followed by an
    /// identifier is not an accessor (the grammar's postfix choice fails
    /// cleanly there), so it ends the chain without being consumed.
    fn scan_postfix_accessors(&mut self) -> Option<()> {
        loop {
            match self.peek() {
                Some(Token::Dot) if matches!(self.peek_at(1), Some(Token::Ident(_))) => {
                    self.index += 2
                }
                Some(Token::LBracket) => self.scan_balanced_group()?,
                _ => break,
            }
        }
        Some(())
    }

    /// Consume a balanced `(...)`/`[...]`/`{...}` group. The tokenizer
    /// suppresses `Newline`/`Indent`/`Dedent` inside open groups, so this
    /// never crosses a statement boundary.
    fn scan_balanced_group(&mut self) -> Option<()> {
        let mut depth = 0usize;
        loop {
            match self.peek()? {
                Token::LParen | Token::LBracket | Token::LBrace => {
                    depth += 1;
                    self.index += 1;
                }
                Token::RParen | Token::RBracket | Token::RBrace => {
                    depth = depth.checked_sub(1)?;
                    self.index += 1;
                    if depth == 0 {
                        return Some(());
                    }
                }
                Token::Eof => return None,
                _ => self.index += 1,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simple_function() {
        let source = r#"
def test():
    x = 10
    /say Hello
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let program = result.unwrap();
        assert_eq!(program.statements.len(), 1);
    }

    #[test]
    fn test_if_statement() {
        let source = r#"
def test():
    x = 5
    if x == 5:
        /say equal
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }

    #[test]
    fn test_for_loop() {
        let source = r#"
def test():
    for i in range(5):
        /say hello
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }

    #[test]
    fn test_execute_block() {
        let source = r#"
def test():
    as @a at @s:
        /particle flame ~ ~ ~
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }

    #[test]
    fn test_asat() {
        let source = r#"
def test():
    asat @s:
        /say Hello
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }

    #[test]
    fn test_global() {
        let source = r#"
def test():
    global score
    score = 10
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
    }

    #[test]
    fn test_import() {
        let source = r#"
import stdlib
from stdlib import event
"#;
        let result = parse(source);
        assert!(result.is_ok(), "Parse failed: {:?}", result.err());
        let program = result.unwrap();
        assert_eq!(program.imports.len(), 2);
    }

    #[test]
    fn excessive_unary_operators_are_rejected() {
        let source = format!(
            "def test():\n    x = {}1\n",
            "+".repeat(MAX_UNARY_OPERATORS + 1)
        );

        assert!(parse(&source).is_err());
    }

    #[test]
    fn parse_diagnostics_use_token_source_spans() {
        let source = "def main():\n    x = \n";
        let errors = parse_with_diagnostics(source).unwrap_err();

        assert!(!errors.is_empty());
        assert_eq!(errors[0].span.line, 2);
        assert_eq!(errors[0].span.column, 9);
    }

    #[test]
    fn parse_located_tracks_import_and_nested_statement_spans() {
        let source = "import helper\n\ndef main():\n    /say hi\n";
        let located = parse_located_with_diagnostics(source).unwrap();

        assert_eq!(located.program.imports.len(), 1);
        assert_eq!(located.program.statements.len(), 1);
        assert_eq!(located.spans.imports, vec![SourceSpan::new(0, 13, 1, 1)]);
        assert_eq!(located.spans.statements.len(), 1);
        assert_eq!(located.spans.statements[0].span.line, 3);
        assert_eq!(located.spans.statements[0].span.column, 1);
        assert_eq!(located.spans.statements[0].children.len(), 1);
        assert_eq!(located.spans.statements[0].children[0].span.line, 4);
        assert_eq!(located.spans.statements[0].children[0].span.column, 5);
    }

    #[test]
    fn parse_located_preserves_crlf_byte_offsets() {
        let source = "def main():\r\n    /say hi\r\n";
        let located = parse_located_with_diagnostics(source).unwrap();

        assert_eq!(located.spans.statements.len(), 1);
        assert_eq!(
            located.spans.statements[0].span,
            SourceSpan::new(0, 11, 1, 1)
        );
        assert_eq!(
            located.spans.statements[0].children[0].span,
            SourceSpan::new(17, 24, 2, 5)
        );
    }
}
