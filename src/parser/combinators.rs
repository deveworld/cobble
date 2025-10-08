use crate::ast::*;
use chumsky::prelude::*;
use super::tokenizer::{Token, tokenize};

/// Token parser using chumsky
pub fn token_parser<'a>(
) -> impl Parser<'a, &'a [Token], Program, extra::Err<Rich<'a, Token>>> + Clone {
    recursive(|stmt| {
        // Expression parser
        let expr = recursive(|expr| {
            let atom = select_ref! {
                Token::Number(n) => Expression::Number(n.parse().unwrap_or(0.0)),
                Token::String(s) => Expression::String(s.clone()),
                Token::True_ => Expression::Boolean(true),
                Token::False_ => Expression::Boolean(false),
                Token::None_ => Expression::None,
                Token::Ident(s) => Expression::Identifier(s.clone()),
            }.or(
                // Parenthesized expression
                just(&Token::LParen)
                    .ignore_then(expr.clone())
                    .then_ignore(just(&Token::RParen))
            );

            // Attribute access (e.g., stdlib.event)
            let postfix = atom.foldl(
                just(&Token::Dot)
                    .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                    .repeated(),
                |base, attr| Expression::Attribute(Box::new(base), attr),
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
            // Unary +/- (high precedence, but lower than call, higher than power)
            // Allow unary operators before any atom/call/parenthesized expression
            let unary = recursive(|unary_rec| {
                choice((
                    just(&Token::Minus).to(UnaryOp::Neg),
                    just(&Token::Plus).to(UnaryOp::Pos),
                ))
                .then(unary_rec.clone())
                .map(|(op, expr)| Expression::Unary(op, Box::new(expr)))
                .or(call.clone())
            });

            // Highest precedence: ^ (power) - right-associative
            // Power operator is right-associative: 2^3^2 = 2^(3^2) = 512
            // We parse left-to-right but fold right-to-left for right-associativity
            let power = unary
                .clone()
                .then(just(&Token::Caret).to(BinaryOp::Pow).then(unary.clone()).repeated().collect::<Vec<_>>())
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
                    select_ref! { Token::Ident(s) => s.clone() }
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

        // Assignment
        let assignment = select_ref! { Token::Ident(s) => s.clone() }
            .then_ignore(just(&Token::Equals))
            .then(expr.clone())
            .map(|(target, value)| Statement::Assignment(Assignment { target, value }));

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
                let name = name_with_at.strip_prefix('@').unwrap_or(&name_with_at).to_string();
                Statement::SelectorDef(SelectorDef { name, selector })
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
            .then(
                just(&Token::By)
                    .ignore_then(expr.clone())
                    .or_not()
            )
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|(((target, iter), step), body)| Statement::For(ForLoop { target, iter, step, body }));

        // While loop
        let while_loop = just(&Token::While)
            .ignore_then(expr.clone())
            .then_ignore(just(&Token::Colon))
            .then_ignore(just(&Token::Newline).or_not())
            .then(block.clone())
            .map(|(condition, body)| Statement::While(WhileLoop { condition, body }));

        // Match pattern
        let match_pattern = choice((
            // Wildcard: _
            just(&Token::Underscore).to(MatchPattern::Wildcard),
            // Range: expr to expr
            select_ref! { Token::Number(n) => n.parse::<i32>().unwrap() }
                .then(
                    just(&Token::To)
                        .ignore_then(select_ref! { Token::Number(n) => n.parse::<i32>().unwrap() })
                        .or_not()
                )
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
        let exec_condition = any()
            .filter(|t: &Token| !matches!(t, Token::Colon | Token::Newline | Token::As | Token::At))
            .repeated()
            .at_least(1)
            .collect::<Vec<Token>>()
            .map(|tokens| {
                // Convert tokens to string, but be smart about spacing
                let mut result = String::new();
                let mut prev_token: Option<&Token> = None;

                for (i, token) in tokens.iter().enumerate() {
                    let token_str = format!("{}", token);

                    // Determine if we need a space before this token
                    let need_space = if i == 0 {
                        false
                    } else if let Some(prev) = prev_token {
                        match (prev, token) {
                            // No space between dots (for "..")
                            (Token::Dot, Token::Dot) => false,
                            // No space after dot if followed by number (for "..10")
                            (Token::Dot, Token::Number(_)) => false,
                            // Space after dot for other cases like "1.. if"
                            (Token::Dot, _) => true,
                            // No space before dots
                            (_, Token::Dot) => false,
                            // No space between minus and number when it's a negative number
                            (Token::Minus, Token::Number(_)) => false,
                            // Default: add space
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

        let execute_modifier = choice((
            just(&Token::As)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .map(ExecuteModifier::As),
            just(&Token::At)
                .ignore_then(select_ref! { Token::Ident(s) => s.clone() })
                .map(ExecuteModifier::At),
            // if/unless in execute blocks:
            // For now, keep as raw and let transpiler determine if it's Python expression
            just(&Token::If)
                .ignore_then(exec_condition.clone())
                .map(ExecuteModifier::IfRaw),
            just(&Token::Unless)
                .ignore_then(exec_condition.clone())
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
            const_assignment,
            assignment,
            expr_stmt,
        ))
        .then_ignore(just(&Token::Newline).or_not());

        // Compound statement (has block)
        let compound_stmt = choice((function, if_stmt, for_loop, while_loop, match_stmt, execute_block));

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
    let tokens = tokenize(source).map_err(|e| vec![e])?;

    let result = token_parser().parse(&tokens);

    match result.into_result() {
        Ok(program) => Ok(program),
        Err(errors) => Err(errors
            .into_iter()
            .map(|e| format!("{}", e.reason()))
            .collect()),
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
}
