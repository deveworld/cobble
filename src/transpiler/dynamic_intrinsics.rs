//! Stage runtime command-helper arguments through the regular call ABI.
use super::{TranspileError, Transpiler};
use crate::ast::{
    CobbleType, ExecuteBlock, ExecuteModifier, Expression, FunctionDef, Parameter, Statement,
};

impl Transpiler {
    pub(super) fn execute_format_expressions(
        &mut self,
        block: &ExecuteBlock,
    ) -> Result<Option<Vec<Statement>>, TranspileError> {
        let mut block = block.clone();
        let mut statements = Vec::new();
        for modifier in &mut block.modifiers {
            let (ExecuteModifier::As(text)
            | ExecuteModifier::At(text)
            | ExecuteModifier::IfRaw(text)
            | ExecuteModifier::UnlessRaw(text)
            | ExecuteModifier::Positioned(text)
            | ExecuteModifier::Rotated(text)
            | ExecuteModifier::In(text)
            | ExecuteModifier::Anchored(text)
            | ExecuteModifier::Align(text)
            | ExecuteModifier::Store(text)
            | ExecuteModifier::Other(text)) = modifier;
            if let Some(mut prefix) = self.command_format_expressions(text)? {
                let Statement::MinecraftCommand(rewritten) = prefix.pop().unwrap() else {
                    unreachable!()
                };
                *text = rewritten;
                statements.extend(prefix);
            }
        }
        if statements.is_empty() {
            return Ok(None);
        }
        let fields: Vec<String> = statements
            .iter()
            .filter_map(|statement| match statement {
                Statement::Assignment(assign) => Some(assign.target.clone()),
                _ => None,
            })
            .collect();
        let saved_context = self.current_context.clone();
        self.current_context.params.extend(fields.iter().cloned());
        let capture = self.capture_statements(&[Statement::Execute(block)]);
        self.current_context = saved_context;
        let capture = capture?;
        let helper = format!("_cobble_execute_format_{}", self.next_temp_id());
        self.add_captured_function(helper.clone(), capture);
        let saved = self.hidden_local(CobbleType::Map);
        let saved_path = self.variable_storage_path(&saved);
        self.symbols
            .variable_storage_paths
            .insert(saved, saved_path.clone());
        let ns = self.data_pack.namespace.clone();
        let mut commands = vec![
            format!("data remove storage {ns}:global {saved_path}"),
            format!(
                "data modify storage {ns}:global {saved_path} set from storage {ns}:global args"
            ),
            format!("data modify storage {ns}:global args set value {{}}"),
            format!(
                "data modify storage {ns}:global args merge from storage {ns}:global {saved_path}"
            ),
        ];
        for field in fields {
            commands.push(format!(
                "data modify storage {ns}:global args.{field} set from storage {ns}:global {}",
                self.variable_storage_path(&field)
            ));
        }
        commands.push(self.function_call_command(&helper, true));
        for param in &self.current_context.reassigned_params {
            commands.push(format!("data modify storage {ns}:global {saved_path}.{param} set from storage {ns}:global args.{param}"));
        }
        commands.extend([
            format!("data remove storage {ns}:global args"),
            format!(
                "data modify storage {ns}:global args set from storage {ns}:global {saved_path}"
            ),
            format!("data remove storage {ns}:global {saved_path}"),
        ]);
        commands.extend(self.return_propagation_command());
        statements.extend(commands.into_iter().map(Statement::MinecraftCommand));
        Ok(Some(statements))
    }

    pub(super) fn runtime_intrinsic_call(
        &mut self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<bool, TranspileError> {
        if self.in_prelude {
            return Ok(false);
        }
        let Expression::Attribute(base, method) = func else {
            return Ok(false);
        };
        if self.is_list_value(base) {
            return Ok(false);
        }
        let Some(module) = Self::expression_path(base) else {
            return Ok(false);
        };
        let integer_positions: &[usize] = match (module.as_str(), method.as_str()) {
            ("score", "set" | "add" | "remove")
            | ("timer", "set")
            | ("bossbar", "set_value" | "set_max")
            | ("storage", "insert") => &[1],
            ("random", "int") => &[1, 2],
            ("entity", "effect_give") => &[3],
            _ => &[],
        };
        for index in integer_positions {
            if args.get(*index).is_some_and(|arg| {
                matches!(
                    self.value_type(arg),
                    CobbleType::Float
                        | CobbleType::Double
                        | CobbleType::String
                        | CobbleType::Map
                        | CobbleType::List
                )
            }) {
                return Err(format!("{module}.{method}() argument {} must be an integer; use int(value) for an explicit conversion", index + 1).into());
            }
        }
        let qualify_id = matches!(module.as_str(), "bossbar" | "schedule")
            && args.first().is_some_and(|arg| {
                self.compile_time_text(arg).is_none()
                    && !matches!(arg, Expression::Identifier(name) if self.functions.params.contains_key(name) && !self.current_context.is_param(name))
            });
        let component_slot = |index: usize| match (module.as_str(), method.as_str(), index) {
            ("bossbar", "add" | "set_name", 1)
            | ("team", "add", 1)
            | ("score.objective", "add", 2) => true,
            ("team", "modify", 2) => args
                .get(1)
                .and_then(|arg| self.compile_time_text(arg))
                .is_some_and(|option| {
                    matches!(option.as_str(), "prefix" | "suffix" | "displayName")
                }),
            _ => false,
        };
        let component_slots: Vec<bool> = args
            .iter()
            .enumerate()
            .map(|(index, arg)| {
                component_slot(index)
                    && !matches!(
                        arg,
                        Expression::String(_) | Expression::Map(_) | Expression::Array(_)
                    )
                    && self.compile_time_text(arg).is_none()
            })
            .collect();
        if !matches!(
            module.as_str(),
            "bossbar"
                | "team"
                | "entity"
                | "score"
                | "score.objective"
                | "timer"
                | "random"
                | "schedule"
                | "storage"
                | "text"
        ) || !(qualify_id
            || component_slots.iter().any(|slot| *slot)
            || args.iter().any(|arg| self.intrinsic_needs_staging(arg))
            || matches!(
                (module.as_str(), method.as_str()),
                ("bossbar", "set_visible") | ("entity", "effect_give")
            ) && args
                .last()
                .is_some_and(|arg| !matches!(arg, Expression::Boolean(_))))
        {
            return Ok(false);
        }
        self.require_stdlib_module(&module)?;
        let mut params = Vec::new();
        let mut values = Vec::new();
        let mut rewritten = Vec::new();
        for (index, arg) in args.iter().enumerate() {
            if component_slots[index] {
                let name = format!("_cobble_component_{}", params.len());
                params.push(Parameter {
                    name: name.clone(),
                    default: None,
                });
                values.push(Expression::Call(
                    Box::new(Expression::Identifier("_cobble_component_value".into())),
                    vec![arg.clone()],
                ));
                rewritten.push(Expression::Identifier(name));
                continue;
            }
            let boolean = matches!(
                (module.as_str(), method.as_str(), index),
                ("bossbar", "set_visible", 1) | ("entity", "effect_give", 4)
            );
            let arg = if qualify_id && index == 0 {
                Expression::Call(
                    Box::new(Expression::Identifier("_cobble_qualify_id".into())),
                    vec![arg.clone()],
                )
            } else if boolean && !matches!(arg, Expression::Boolean(_)) {
                Expression::Conditional(
                    Box::new(arg.clone()),
                    Box::new(Expression::String("true".into())),
                    Box::new(Expression::String("false".into())),
                )
            } else if matches!(
                (module.as_str(), method.as_str(), index),
                ("score", _, 0)
                    | ("score", "copy", 1)
                    | ("score", "operation", 2)
                    | ("storage", "read_score", 0)
                    | ("timer", _, 0)
                    | ("random", "int" | "bool", 0)
            ) {
                match arg {
                    Expression::String(name) if self.current_context.is_local(name) => {
                        Expression::String(super::statement_processors::local_holder(
                            self.current_function_name.as_deref().unwrap_or_default(),
                            name,
                        ))
                    }
                    _ => arg.clone(),
                }
            } else {
                arg.clone()
            };
            rewritten.push(self.stage_intrinsic_argument(&arg, &mut params, &mut values));
        }
        if params.is_empty() {
            return Ok(false);
        }
        let name = format!("_cobble_command_{}", self.next_temp_id());
        let definition = FunctionDef {
            name: name.clone(),
            params,
            decorators: Vec::new(),
            body: vec![Statement::Expression(Expression::Call(
                Box::new(func.clone()),
                rewritten,
            ))],
        };
        let previous = std::mem::replace(&mut self.in_prelude, true);
        let result = self.process_function_def(&definition);
        self.in_prelude = previous;
        result?;
        let call = Statement::Expression(Expression::Call(
            Box::new(Expression::Identifier(name)),
            values,
        ));
        self.process_statement(&call)?;
        Ok(true)
    }

    fn intrinsic_needs_staging(&self, expr: &Expression) -> bool {
        if self.compile_time_text(expr).is_some() || self.try_eval_const(expr).is_some() {
            return false;
        }
        match expr {
            Expression::Binary(left, _, right) if super::desugar::has_format_field(expr) => {
                self.intrinsic_needs_staging(left) || self.intrinsic_needs_staging(right)
            }
            Expression::Call(func, args) if matches!(&**func, Expression::Identifier(name) if name == "_cobble_format") => {
                args.iter().any(|arg| self.intrinsic_needs_staging(arg))
            }
            Expression::Identifier(name) => {
                super::intrinsics::is_value_name(name)
                    && !self.current_context.is_param(name)
                    && !self.symbols.compile_time_constants.contains_key(name)
                    && !self.functions.params.contains_key(name)
            }
            Expression::Array(items) => items.iter().any(|item| self.intrinsic_needs_staging(item)),
            Expression::Map(items) => items
                .iter()
                .any(|(_, value)| self.intrinsic_needs_staging(value)),
            Expression::Call(func, args) if matches!(&**func, Expression::Attribute(base, _) if Self::expression_path(base).is_some_and(|module| matches!(module.as_str(), "storage" | "text" | "position" | "selector"))) => {
                args.iter().any(|arg| self.intrinsic_needs_staging(arg))
            }
            Expression::Call(..)
            | Expression::Binary(..)
            | Expression::Unary(..)
            | Expression::Subscript(..)
            | Expression::Attribute(..)
            | Expression::Conditional(..) => true,
            _ => false,
        }
    }

    fn stage_intrinsic_argument(
        &self,
        expr: &Expression,
        params: &mut Vec<Parameter>,
        values: &mut Vec<Expression>,
    ) -> Expression {
        match expr {
            Expression::Binary(left, op, right) if super::desugar::has_format_field(expr) => {
                Expression::Binary(
                    Box::new(self.stage_intrinsic_argument(left, params, values)),
                    op.clone(),
                    Box::new(self.stage_intrinsic_argument(right, params, values)),
                )
            }
            Expression::Call(func, args) if matches!(&**func, Expression::Identifier(name) if name == "_cobble_format") => {
                Expression::Call(
                    func.clone(),
                    args.iter()
                        .map(|arg| self.stage_intrinsic_argument(arg, params, values))
                        .collect(),
                )
            }
            Expression::String(_)
            | Expression::Number(_)
            | Expression::Boolean(_)
            | Expression::None => expr.clone(),
            Expression::Identifier(name)
                if self.symbols.compile_time_constants.contains_key(name)
                    || (!super::intrinsics::is_value_name(name))
                    || self.functions.params.contains_key(name) =>
            {
                expr.clone()
            }
            Expression::Array(items) => Expression::Array(
                items
                    .iter()
                    .map(|item| self.stage_intrinsic_argument(item, params, values))
                    .collect(),
            ),
            Expression::Map(items) => Expression::Map(
                items
                    .iter()
                    .map(|(key, value)| {
                        (
                            key.clone(),
                            self.stage_intrinsic_argument(value, params, values),
                        )
                    })
                    .collect(),
            ),
            Expression::Call(func, args) if matches!(&**func, Expression::Attribute(base, _) if Self::expression_path(base).is_some_and(|module| matches!(module.as_str(), "storage" | "text" | "position" | "selector"))) => {
                Expression::Call(
                    func.clone(),
                    args.iter()
                        .map(|arg| self.stage_intrinsic_argument(arg, params, values))
                        .collect(),
                )
            }
            _ => {
                let name = format!("argument_{}", params.len());
                params.push(Parameter {
                    name: name.clone(),
                    default: None,
                });
                values.push(expr.clone());
                Expression::Identifier(name)
            }
        }
    }

    pub(super) fn command_format_expressions(
        &mut self,
        command: &str,
    ) -> Result<Option<Vec<Statement>>, TranspileError> {
        use crate::parser::strings::{
            format_parts, formatted_string_ranges, parse_format_expression, FormatPart,
        };
        let mut fields = Vec::new();
        for range in formatted_string_ranges(command)? {
            for part in format_parts(&command[range])? {
                if let FormatPart::Name(field) = part {
                    let expr = parse_format_expression(field)?;
                    let offset = field.as_ptr() as usize - command.as_ptr() as usize;
                    fields.push((offset, field.len(), expr));
                }
            }
        }
        if fields
            .iter()
            .all(|(_, _, expr)| matches!(expr, Expression::Identifier(_)))
        {
            return Ok(None);
        }
        let mut statements = Vec::new();
        let mut rewritten = command.to_string();
        let mut replacements = Vec::new();
        for (offset, length, expr) in fields {
            let holder = self.hidden_local(CobbleType::Unknown);
            statements.push(Statement::Assignment(crate::ast::Assignment {
                target: holder.clone(),
                target_expr: Some(Expression::Identifier(holder.clone())),
                value: expr,
                persist: false,
            }));
            replacements.push((offset, length, holder));
        }
        for (offset, length, holder) in replacements.into_iter().rev() {
            rewritten.replace_range(offset..offset + length, &holder);
        }
        statements.push(Statement::MinecraftCommand(rewritten));
        Ok(Some(statements))
    }
}
