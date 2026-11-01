use crate::ast::*;
use crate::transpiler::command_processor::expand_selector_aliases;
use crate::transpiler::condition_lowering::{
    mask_minecraft_conditions, merged_execute_in_condition, RAW_CONDITION_KEYWORDS,
};
use crate::transpiler::condition_translator::{
    condition_clauses, negate_clause, TranslatedCondition,
};
use crate::transpiler::placeholders::{self, Piece};
use crate::transpiler::raw_minecraft_condition_verdict;
use crate::transpiler::{
    FunctionCapture, FunctionReturnKind, GeneratedCommand, GeneratedCommandKind, TranspileError,
    Transpiler,
};

/// Execute conditions that cannot be spliced into the execute prefix
/// directly and are lowered to a flag variable after all modifiers are
/// processed.
enum DeferredExecuteCondition {
    /// `if a or b ...`: set the flag under each alternative, then `if flag`.
    Or(Vec<String>),
    /// `unless (A and B ...)`: set the flag under the full chain, then
    /// `unless flag`.
    UnlessAnd(String),
}

impl DeferredExecuteCondition {
    fn map_text(&mut self, mut f: impl FnMut(&str) -> String) {
        match self {
            Self::Or(alternatives) => {
                for alternative in alternatives {
                    *alternative = f(alternative);
                }
            }
            Self::UnlessAnd(condition) => *condition = f(condition),
        }
    }
}

/// The execute chain built from a block's modifiers: `execute` followed by
/// one part per modifier or condition clause, plus the conditions deferred
/// to flag variables.
struct ExecuteChain {
    parts: Vec<String>,
    deferred: Vec<DeferredExecuteCondition>,
    has_macro_params: bool,
}

impl ExecuteChain {
    fn map_text(&mut self, mut f: impl FnMut(&str) -> String) {
        for part in &mut self.parts {
            *part = f(part);
        }
        for deferred in &mut self.deferred {
            deferred.map_text(&mut f);
        }
    }
}

impl Transpiler {
    /// Resolves an `as`/`at` target: a `{param}` macro placeholder, a
    /// selector alias (`@Name -> @a[...]`), or a literal selector or name.
    /// An `@Name` that is neither an alias nor a vanilla selector variable
    /// would reach Minecraft as an unparsable command, so it is an error.
    fn resolve_execute_selector(
        &self,
        selector: &str,
        has_macro_params: &mut bool,
    ) -> Result<String, TranspileError> {
        if let Some(param_name) = selector
            .strip_prefix('{')
            .and_then(|rest| rest.strip_suffix('}'))
        {
            if self.current_context.is_param(param_name) {
                *has_macro_params = true;
                return Ok(format!("$({})", param_name));
            }
            return Ok(selector.to_string());
        }

        let Some(selector_name) = selector.strip_prefix('@') else {
            return Ok(selector.to_string());
        };
        if let Some(actual_selector) = self.symbols.selector_aliases.get(selector_name) {
            // The header's placeholders are read later, and the alias's
            // escapes are read already.
            return Ok(placeholders::escape_braces(actual_selector));
        }
        let variable = selector_name.split('[').next().unwrap_or("");
        if !matches!(variable, "a" | "e" | "p" | "r" | "s" | "n") {
            let hint = if self.symbols.entity_templates.contains_key(selector_name) {
                format!(
                    " `@{}` is an entity template, which only works with `create`; \
                     add a selector alias `@Name = @e[...]` to target its entities.",
                    selector_name
                )
            } else {
                " Define it with `@Name = @selector[...]` or use a vanilla selector \
                 (@a, @e, @p, @r, @s, @n)."
                    .to_string()
            };
            return Err(format!("Unknown selector `{}`.{}", selector, hint).into());
        }
        Ok(selector.to_string())
    }

    pub(in crate::transpiler) fn process_execute_block(
        &mut self,
        exec_block: &ExecuteBlock,
    ) -> Result<(), TranspileError> {
        // Track a per-site holder slot so condition-scope objectives can be
        // pooled by (nesting depth, slot) instead of growing per call site.
        self.execute_condition_slots.push(0);
        let result = self.process_execute_block_impl(exec_block);
        self.execute_condition_slots.pop();
        result
    }

    fn process_execute_block_impl(
        &mut self,
        exec_block: &ExecuteBlock,
    ) -> Result<(), TranspileError> {
        let mut chain = self.build_execute_chain(&exec_block.modifiers)?;
        self.expand_execute_header_placeholders(&mut chain)?;
        self.lower_deferred_execute_conditions(&mut chain);
        let execute_prefix = chain.parts.join(" ");

        // Capture the whole body first; like `if`, the number of commands it
        // produced decides between inlining and routing through a helper.
        let capture = self.capture_execute_body(&exec_block.body)?;
        if self.execute_body_needs_helper(exec_block, &capture) {
            self.emit_execute_helper_call(
                capture,
                &exec_block.body,
                &execute_prefix,
                chain.has_macro_params,
            );
            return Ok(());
        }
        // A `store` block around a call stores the call's result. The
        // commands that pass the arguments, and clean them up afterwards,
        // run without the `store`.
        if let ([Statement::Expression(Expression::Call(..))], true) = (
            exec_block.body.as_slice(),
            chain.parts.iter().any(|part| part.starts_with("store ")),
        ) {
            let call_index = capture.commands.iter().rposition(|command| {
                command
                    .strip_prefix('$')
                    .unwrap_or(command)
                    .starts_with("function ")
            });
            if let Some(call_index) = call_index.filter(|_| capture.commands.len() > 1) {
                let plain_prefix = chain
                    .parts
                    .iter()
                    .filter(|part| !part.starts_with("store "))
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(" ");
                let mut has_macro_params = chain.has_macro_params;
                let mut index = 0;
                return self.append_transformed_capture(capture, |cmd| {
                    let is_call = index == call_index;
                    index += 1;
                    let inner_cmd = if let Some(stripped) = cmd.strip_prefix('$') {
                        has_macro_params = true;
                        stripped
                    } else {
                        cmd
                    };
                    let prefix = if is_call {
                        &execute_prefix
                    } else {
                        &plain_prefix
                    };
                    let final_cmd = if prefix == "execute" {
                        inner_cmd.to_string()
                    } else if let Some(inner_parts) = inner_cmd.strip_prefix("execute ") {
                        format!("{} {}", prefix, inner_parts)
                    } else {
                        format!("{} run {}", prefix, inner_cmd)
                    };
                    Self::mark_macro_line(final_cmd, has_macro_params)
                });
            }
        }
        self.emit_inline_execute_body(capture, &execute_prefix, chain.has_macro_params)
    }

    fn build_execute_chain(
        &mut self,
        modifiers: &[ExecuteModifier],
    ) -> Result<ExecuteChain, TranspileError> {
        let mut chain = ExecuteChain {
            parts: vec!["execute".to_string()],
            deferred: Vec::new(),
            has_macro_params: false,
        };
        let mut merged_condition: Option<ExecuteModifier> = None;
        for (index, modifier) in modifiers.iter().enumerate() {
            if merged_condition.take().is_some() {
                // The `in` of a condition merged into the one before it.
                continue;
            }
            // `if entity @a and x in items:` reads as `if entity @a and x`
            // and the modifier `in items`: the `in` is the operator of the
            // Python operand before it.
            if let (
                ExecuteModifier::IfRaw(value) | ExecuteModifier::UnlessRaw(value),
                Some(ExecuteModifier::In(container)),
            ) = (modifier, modifiers.get(index + 1))
            {
                if let Some(merged) = merged_execute_in_condition(value, container) {
                    merged_condition = Some(modifier.clone());
                    if matches!(modifier, ExecuteModifier::IfRaw(_)) {
                        self.push_execute_if(&mut chain, &merged)?;
                    } else {
                        self.push_execute_unless(&mut chain, &merged)?;
                    }
                    continue;
                }
            }
            // `if x in items:` reads as `if x` and the modifier `in items`.
            if let (
                ExecuteModifier::IfRaw(value) | ExecuteModifier::UnlessRaw(value),
                Some(ExecuteModifier::In(container)),
            ) = (modifier, modifiers.get(index + 1))
            {
                if raw_minecraft_condition_verdict(value.trim_start()).is_none() {
                    return Err(format!(
                        "Unsupported condition `{} in {}`: an execute header can't use the `in` operator. Test it with an `if` inside the block.",
                        value.trim(),
                        container.trim()
                    )
                    .into());
                }
            }
            match modifier {
                ExecuteModifier::As(selector) => {
                    self.push_execute_target(&mut chain, "as", selector)?
                }
                ExecuteModifier::At(selector) => {
                    self.push_execute_target(&mut chain, "at", selector)?
                }
                ExecuteModifier::IfRaw(condition) => self.push_execute_if(&mut chain, condition)?,
                ExecuteModifier::UnlessRaw(condition) => {
                    self.push_execute_unless(&mut chain, condition)?
                }
                ExecuteModifier::Positioned(pos) => {
                    Self::push_execute_positioned(&mut chain.parts, pos)
                }
                ExecuteModifier::Rotated(rot) => chain.parts.push(format!("rotated {}", rot)),
                ExecuteModifier::In(dimension) => chain.parts.push(format!("in {}", dimension)),
                ExecuteModifier::Anchored(anchor) => {
                    chain.parts.push(format!("anchored {}", anchor))
                }
                ExecuteModifier::Align(axes) => chain.parts.push(format!("align {}", axes)),
                ExecuteModifier::Other(modifier) => chain.parts.push(modifier.clone()),
                ExecuteModifier::Store(store_cmd) => {
                    chain.parts.push(format!("store {}", store_cmd))
                }
            }
        }
        Ok(chain)
    }

    fn push_execute_target(
        &self,
        chain: &mut ExecuteChain,
        keyword: &str,
        selector: &str,
    ) -> Result<(), TranspileError> {
        let selector = self.resolve_execute_selector(selector, &mut chain.has_macro_params)?;
        chain.parts.push(format!("{} {}", keyword, selector));
        Ok(())
    }

    fn push_execute_if(
        &mut self,
        chain: &mut ExecuteChain,
        condition: &str,
    ) -> Result<(), TranspileError> {
        // Check for macro parameters in the condition
        if self
            .current_context
            .params
            .iter()
            .any(|param| condition.contains(&format!("{{{}}}", param)))
        {
            chain.has_macro_params = true;
        }

        if let Some(translated) = self.capture_mixed_condition(chain, condition)? {
            return self.push_python_if_condition(chain, translated);
        }
        if let Some(translated) = self.capture_execute_python_condition(chain, condition)? {
            return self.push_python_if_condition(chain, translated);
        }

        // The condition looks like a Python expression but did not parse.
        if self.looks_like_python_expression(condition) {
            return Err(format!(
                "Failed to translate Python expression '{}' to Minecraft condition.\n\
                 This may be an unsupported construct like OR conditions.\n\
                 Consider rewriting the condition or using raw Minecraft syntax.",
                condition
            )
            .into());
        }
        Self::check_raw_condition(condition)?;
        Self::push_raw_if_condition(chain, condition);
        Ok(())
    }

    fn push_execute_unless(
        &mut self,
        chain: &mut ExecuteChain,
        condition: &str,
    ) -> Result<(), TranspileError> {
        if let Some(translated) = self.capture_mixed_condition(chain, condition)? {
            return self.push_python_unless_condition(chain, translated);
        }
        if let Some(translated) = self.capture_execute_python_condition(chain, condition)? {
            return self.push_python_unless_condition(chain, translated);
        }

        // The condition looks like a Python expression but did not parse.
        if self.looks_like_python_expression(condition) {
            return Err(format!(
                "Failed to translate Python expression '{}' to Minecraft condition.\n\
                 This may be an unsupported construct.\n\
                 Consider rewriting the condition or using raw Minecraft syntax.",
                condition
            )
            .into());
        }
        Self::check_raw_condition(condition)?;
        Self::push_raw_unless_condition(chain, condition);
        Ok(())
    }

    /// A condition that is neither a Cobble expression nor starts with a
    /// Minecraft condition (`entity`, `score`, `block`, ...) would reach the
    /// pack as an `execute` command Minecraft can't load, as `2 in items`
    /// would.
    fn check_raw_condition(condition: &str) -> Result<(), TranspileError> {
        if raw_minecraft_condition_verdict(condition.trim_start()).is_some() {
            return Ok(());
        }
        let keywords = RAW_CONDITION_KEYWORDS
            .iter()
            .map(|keyword| keyword.trim_end())
            .collect::<Vec<_>>()
            .join(", ");
        Err(format!(
            "Unsupported condition `{condition}`. A condition compares numbers, tests a value, or starts with a Minecraft condition: {keywords}."
        )
        .into())
    }

    /// Translate a condition that combines Minecraft conditions with Python
    /// through `and`, `or`, or `not`, as `entity @a and x > 3`: each
    /// Minecraft condition takes the place of a value in the Python
    /// condition. A combination that doesn't translate is an error that
    /// names the chained form.
    fn capture_mixed_condition(
        &mut self,
        chain: &mut ExecuteChain,
        condition: &str,
    ) -> Result<Option<TranslatedCondition>, TranspileError> {
        let Some((masked, conditions)) = mask_minecraft_conditions(condition) else {
            return Ok(None);
        };
        // Each identifier translates to its Minecraft condition, so the
        // commands a short-circuit `and`/`or` writes test the real condition.
        let saved = std::mem::replace(
            &mut self.symbols.minecraft_conditions,
            conditions.iter().cloned().collect(),
        );
        let captured = self.capture_lowered_mixed_condition(chain, &masked, condition);
        self.symbols.minecraft_conditions = saved;
        let Some(translated) = captured? else {
            return Err(format!(
                "Can't combine the Minecraft conditions in `{}` with `and`, `or`, or `not`. \
                 Chain them as modifiers instead, as in `if entity @a if x > 3:`, and use \
                 `unless` for `not`.",
                condition.trim()
            )
            .into());
        };
        Ok(Some(translated))
    }

    /// A mixed condition whose Python part takes statements to compute, as
    /// `entity @a and x in [1, 5]`: the statements run first, under the
    /// modifiers collected so far, with the right side of `and`/`or` in a
    /// branch so it runs only when needed. Under `as`, those statements
    /// would share one result between the entities, so that is an error.
    /// Other conditions translate as usual.
    fn capture_lowered_mixed_condition(
        &mut self,
        chain: &mut ExecuteChain,
        masked: &str,
        condition: &str,
    ) -> Result<Option<TranslatedCondition>, TranspileError> {
        let (masked_tokens, operand_tokens) = Self::mask_condition_operand_tokens(masked);
        let expression = Self::parse_raw_condition_expression(&masked_tokens)
            .filter(|expression| self.needs_lowering(expression));
        let Some(expression) = expression else {
            return self.capture_execute_python_condition(chain, masked);
        };
        if Self::has_as_modifier(&chain.parts) {
            return Err(format!(
                "Unsupported condition `{}`: under `as`, an execute header can't compute `in`, a join, or another value that takes commands. Test it with an `if` inside the block.",
                condition.trim()
            )
            .into());
        }
        let (translated, mut capture) = self
            .capture_commands_with_result(|transpiler| {
                let mut statements = Vec::new();
                let value = transpiler.lower_value(&expression, &mut statements)?;
                for statement in &statements {
                    transpiler.process_statement(statement)?;
                }
                let processed = transpiler.preprocess_condition(&value)?;
                transpiler.translate_condition(&processed)
            })
            .map_err(|error| Self::restore_condition_operand_tokens(error, &operand_tokens))?;
        let mut translated = Self::fold_always_false_conditions(translated);
        for (placeholder, original) in &operand_tokens {
            translated.map_simple(&|text| text.replace(placeholder.as_str(), original));
            for command in &mut capture.commands {
                *command = command.replace(placeholder, original);
            }
            for generated in capture.metadata.values_mut() {
                generated.text = generated.text.replace(placeholder, original);
            }
        }
        self.append_scoped_execute_capture(capture, &chain.parts, &mut chain.has_macro_params)?;
        Ok(Some(translated))
    }

    /// Translate an `if`/`unless` condition that parses as a Python
    /// expression. Its setup commands are scoped to `@s` under `as` and
    /// emitted under the modifiers collected so far.
    fn capture_execute_python_condition(
        &mut self,
        chain: &mut ExecuteChain,
        condition: &str,
    ) -> Result<Option<TranslatedCondition>, TranspileError> {
        let Some((translated, capture)) = self.capture_raw_python_condition(condition)? else {
            return Ok(None);
        };
        let (translated, capture) =
            self.scope_condition_capture_for_execute(translated, capture, &chain.parts);
        self.append_scoped_execute_capture(capture, &chain.parts, &mut chain.has_macro_params)?;
        Ok(Some(translated))
    }

    fn push_python_if_condition(
        &mut self,
        chain: &mut ExecuteChain,
        translated: TranslatedCondition,
    ) -> Result<(), TranspileError> {
        match translated {
            TranslatedCondition::AnyOf(..) => {
                let alternatives = self.flatten_execute_or_alternatives(
                    translated,
                    &chain.parts,
                    &mut chain.has_macro_params,
                )?;
                chain
                    .deferred
                    .push(DeferredExecuteCondition::Or(alternatives));
            }
            TranslatedCondition::AlwaysFalse => {
                chain.parts.push(self.always_false_execute_condition());
            }
            TranslatedCondition::Simple(translated) => {
                chain.parts.push(Self::condition_execute_args(&translated));
            }
            structured => {
                // AND around an OR: lower it to a flag condition right here.
                let rendered = self.lower_structured_execute_condition(
                    structured,
                    &chain.parts,
                    &mut chain.has_macro_params,
                )?;
                chain.parts.push(Self::condition_execute_args(&rendered));
            }
        }
        Ok(())
    }

    fn push_python_unless_condition(
        &mut self,
        chain: &mut ExecuteChain,
        translated: TranslatedCondition,
    ) -> Result<(), TranspileError> {
        match translated {
            TranslatedCondition::AnyOf(..) => {
                // unless (A or B) = unless A and unless B (De Morgan's law)
                let alternatives = self.flatten_execute_or_alternatives(
                    translated,
                    &chain.parts,
                    &mut chain.has_macro_params,
                )?;
                for cond in alternatives {
                    Self::push_negated_execute_condition(cond, chain);
                }
            }
            // `unless` an impossible condition always passes.
            TranslatedCondition::AlwaysFalse => {}
            TranslatedCondition::Simple(translated) => {
                Self::push_negated_execute_condition(translated, chain);
            }
            structured => {
                // AND around an OR: lower it to a flag condition, then
                // negate the result.
                let rendered = self.lower_structured_execute_condition(
                    structured,
                    &chain.parts,
                    &mut chain.has_macro_params,
                )?;
                Self::push_negated_execute_condition(rendered, chain);
            }
        }
        Ok(())
    }

    fn push_raw_if_condition(chain: &mut ExecuteChain, condition: &str) {
        let alternatives = Self::split_raw_conditions(condition, " or ");
        let clauses = Self::split_raw_conditions(condition, " and ");
        if alternatives.len() > 1 {
            // OR conditions need special handling with temp variables.
            // An alternative `A and B` becomes the chain `A if B`.
            chain.deferred.push(DeferredExecuteCondition::Or(
                alternatives
                    .iter()
                    .map(|alternative| {
                        Self::split_raw_conditions(alternative, " and ")
                            .iter()
                            .map(|clause| Self::normalize_condition_spacing(clause))
                            .collect::<Vec<_>>()
                            .join(" if ")
                    })
                    .collect(),
            ));
        } else if clauses.len() > 1 {
            // Split on " and " and create multiple if conditions
            for part in clauses {
                chain
                    .parts
                    .push(format!("if {}", Self::normalize_condition_spacing(&part)));
            }
        } else {
            // Fix spacing for range operators in single conditions too
            chain.parts.push(format!(
                "if {}",
                Self::normalize_condition_spacing(condition)
            ));
        }
    }

    fn push_raw_unless_condition(chain: &mut ExecuteChain, condition: &str) {
        let alternatives = Self::split_raw_conditions(condition, " or ");
        if alternatives.len() > 1 {
            // unless (A or B) = unless A and unless B (De Morgan's law)
            for alternative in alternatives {
                if Self::split_raw_conditions(&alternative, " and ").len() > 1 {
                    chain
                        .deferred
                        .push(DeferredExecuteCondition::UnlessAnd(alternative));
                } else {
                    chain.parts.push(format!(
                        "unless {}",
                        Self::normalize_condition_spacing(&alternative)
                    ));
                }
            }
        } else if Self::split_raw_conditions(condition, " and ").len() > 1 {
            // unless (A and B) requires special handling with temp variable
            chain
                .deferred
                .push(DeferredExecuteCondition::UnlessAnd(condition.to_string()));
        } else {
            // Fix spacing for range operators
            chain.parts.push(format!(
                "unless {}",
                Self::normalize_condition_spacing(condition)
            ));
        }
    }

    fn push_execute_positioned(parts: &mut Vec<String>, pos: &str) {
        // Workaround for parser bug: positioned might incorrectly contain "if" conditions
        // Split at " if " to separate positioned from condition
        let Some(if_pos) = pos.find(" if score ") else {
            parts.push(format!("positioned {}", pos));
            return;
        };
        let actual_pos = &pos[..if_pos];
        let condition = &pos[if_pos + 4..]; // Skip " if "

        parts.push(format!("positioned {}", actual_pos));
        if condition.contains(" and ") {
            for part in condition.split(" and ") {
                let fixed_part = part.trim().replace("matches..", "matches ..");
                parts.push(format!("if {}", fixed_part));
            }
        } else {
            let fixed_condition = condition.replace("matches..", "matches ..");
            parts.push(format!("if {}", fixed_condition));
        }
    }

    /// Selector aliases, `{CONST}`, and `{param}` placeholders may sit
    /// anywhere in the header, such as `if entity @Spawn`,
    /// `as @a[distance=..{RADIUS}]`, or `positioned {x} 100 0`. `{{` and
    /// `}}` are literal braces, as in a raw command. Any other placeholder
    /// would reach Minecraft as written and fail the whole function, so it
    /// is an error.
    fn expand_execute_header_placeholders(
        &self,
        chain: &mut ExecuteChain,
    ) -> Result<(), TranspileError> {
        // An alias holds its text with escapes already read, so its braces
        // are escaped again for the placeholder reading below.
        let aliases: std::collections::HashMap<String, String> = self
            .symbols
            .selector_aliases
            .iter()
            .map(|(name, selector)| (name.clone(), placeholders::escape_braces(selector)))
            .collect();
        chain.map_text(|text| expand_selector_aliases(text, &aliases));
        let mut has_macro_params = chain.has_macro_params;
        let mut error = None;
        chain.map_text(|text| match self.fill_header_placeholders(text) {
            Ok((filled, uses_params)) => {
                has_macro_params |= uses_params;
                filled
            }
            Err(message) => {
                error.get_or_insert(message);
                text.to_string()
            }
        });
        if let Some(message) = error {
            return Err(message.into());
        }
        chain.has_macro_params = has_macro_params;
        Ok(())
    }

    /// `text` with its placeholders filled in, and whether it uses a
    /// parameter as a macro argument.
    fn fill_header_placeholders(&self, text: &str) -> Result<(String, bool), String> {
        let mut filled = String::with_capacity(text.len());
        let mut uses_params = false;
        for piece in placeholders::checked_pieces(text)? {
            let (raw, name) = match piece {
                Piece::Text { text, .. } => {
                    filled.push_str(&text);
                    continue;
                }
                Piece::Field { raw, name, .. } => (raw, name),
            };
            if self.current_context.is_param(name) {
                uses_params = true;
                filled.push_str(&format!("$({})", name));
            } else if let Some(value) = self.symbols.compile_time_constants.get(name) {
                if value.fract() == 0.0 {
                    filled.push_str(&format!("{}", *value as i64));
                } else {
                    filled.push_str(&value.to_string());
                }
            } else if self.symbols.scoreboard_variables.contains(name)
                || self.symbols.variable_storage_paths.contains_key(name)
                || self.symbols.variables.contains_key(name)
            {
                return Err(format!(
                    "Cannot use the variable `{name}` as a placeholder in an execute header. \
                     Only a function parameter or a const can fill `{raw}` there. \
                     To use the variable's value, pass it to a function parameter and \
                     write the header inside that function."
                ));
            } else {
                return Err(format!(
                    "Undefined placeholder `{raw}` in an execute header. Define `{name}` as a \
                     const or a function parameter, or write `{{{{{name}}}}}` for literal braces."
                ));
            }
        }
        Ok((filled, uses_params))
    }

    /// Lower each deferred condition to a flag score: setup commands reset
    /// it and set it under the modifiers collected so far, and the chain
    /// checks it.
    fn lower_deferred_execute_conditions(&mut self, chain: &mut ExecuteChain) {
        if chain.deferred.is_empty() {
            return;
        }

        let modifier_args = Self::modifier_args(&chain.parts);
        let has_as_modifier = Self::has_as_modifier(&chain.parts);
        let use_dedicated_objectives = chain.deferred.len() > 1;
        let mut setup_commands = Vec::new();

        for deferred in std::mem::take(&mut chain.deferred) {
            let (score_holder, objective) =
                self.deferred_condition_flag(&deferred, has_as_modifier, use_dedicated_objectives);

            let reset_cmd = if modifier_args.is_empty() && score_holder != "@s" {
                format!("scoreboard players set {} {} 0", score_holder, objective)
            } else {
                Self::execute_with_modifiers(
                    &modifier_args,
                    &format!(
                        "run scoreboard players set {} {} 0",
                        score_holder, objective
                    ),
                )
            };
            setup_commands.push(reset_cmd);

            match deferred {
                DeferredExecuteCondition::Or(alternatives) => {
                    for cond in alternatives {
                        setup_commands.push(Self::execute_with_modifiers(
                            &modifier_args,
                            &format!(
                                "{} run scoreboard players set {} {} 1",
                                Self::condition_execute_args(&cond),
                                score_holder,
                                objective
                            ),
                        ));
                    }
                    chain
                        .parts
                        .push(format!("if score {} {} matches 1", score_holder, objective));
                }
                DeferredExecuteCondition::UnlessAnd(condition) => {
                    let and_conditions = Self::unless_and_conditions(&condition);
                    let and_check = and_conditions.join(" ");
                    setup_commands.push(Self::execute_with_modifiers(
                        &modifier_args,
                        &format!(
                            "{} run scoreboard players set {} {} 1",
                            and_check, score_holder, objective
                        ),
                    ));
                    chain.parts.push(format!(
                        "unless score {} {} matches 1",
                        score_holder, objective
                    ));
                }
            }
        }

        if let Some(ref mut commands) = self.current_function {
            for command in setup_commands {
                commands.push(Self::mark_macro_line(command, chain.has_macro_params));
            }
        }
    }

    /// The `(score holder, objective)` flag for one deferred condition.
    fn deferred_condition_flag(
        &mut self,
        deferred: &DeferredExecuteCondition,
        has_as_modifier: bool,
        use_dedicated_objectives: bool,
    ) -> (String, String) {
        let unique_id = self.next_temp_id();
        // Under `as`, the flag is each entity's own score, so it can't be on
        // the objective of the module variables: an entity named like one
        // would change it.
        let objective = if use_dedicated_objectives {
            format!("cblx{}", unique_id)
        } else if has_as_modifier {
            self.next_condition_scope_objective()
        } else {
            "temp".to_string()
        };
        self.data_pack.track_objective(&objective);

        let score_holder = if has_as_modifier {
            "@s".to_string()
        } else if use_dedicated_objectives {
            "#cobble_exec".to_string()
        } else if matches!(deferred, DeferredExecuteCondition::Or(_)) {
            format!("or_temp_{}", unique_id)
        } else {
            format!("unless_temp_{}", unique_id)
        };
        (score_holder, objective)
    }

    fn capture_execute_body(
        &mut self,
        body: &[Statement],
    ) -> Result<FunctionCapture, TranspileError> {
        let saved_context = self.current_context.clone();
        let capture = self.capture_statements(body)?;
        self.current_context = saved_context;
        Ok(capture)
    }

    /// Prefixing several commands with the execute chain re-evaluates the
    /// conditions and re-forks the selector for every command: an earlier
    /// command that changes the condition cuts off the rest of the body,
    /// and under a multi-entity `as` every entity runs the first command
    /// before any entity runs the second, so shared state (loop counters,
    /// if/else flags, scratch scores) breaks. A helper runs the whole body
    /// once per execution context instead.
    fn execute_body_needs_helper(
        &self,
        exec_block: &ExecuteBlock,
        capture: &FunctionCapture,
    ) -> bool {
        // `store` blocks keep the per-command form: storing a helper call
        // would store the helper's return value rather than each command's
        // result.
        let has_store_modifier = exec_block
            .modifiers
            .iter()
            .any(|modifier| matches!(modifier, ExecuteModifier::Store(_)));
        // A raw `/return` in the body must return from the enclosing
        // function, which it only does when it runs in that function; inside
        // a helper it would just end the helper. Such bodies keep the
        // per-command form.
        let has_raw_return = exec_block.body.iter().any(|statement| {
            matches!(statement, Statement::MinecraftCommand(command) if Self::is_raw_return_command(command))
        });
        capture.commands.len() > 1
            && !has_store_modifier
            && !has_raw_return
            && self.current_function.is_some()
    }

    fn emit_execute_helper_call(
        &mut self,
        capture: FunctionCapture,
        body: &[Statement],
        execute_prefix: &str,
        has_macro_params: bool,
    ) {
        let helper_name = format!("execute_temp_{}", self.next_temp_id());
        let needs_storage = capture.requires_macro_context();
        let capture = self.stop_forks_after_return(capture, body);
        self.add_captured_function(helper_name.clone(), capture);

        let call_cmd = self.function_call_command(&helper_name, needs_storage);
        let call = Self::mark_macro_line(
            format!("{} run {}", execute_prefix, call_cmd),
            has_macro_params,
        );
        let return_cmd = self.return_propagation_command();
        if let Some(ref mut commands) = self.current_function {
            commands.push(call);
            if let Some(return_cmd) = return_cmd {
                commands.push(return_cmd);
            }
        }
    }

    fn emit_inline_execute_body(
        &mut self,
        capture: FunctionCapture,
        execute_prefix: &str,
        mut has_macro_params: bool,
    ) -> Result<(), TranspileError> {
        self.append_transformed_capture(capture, |cmd| {
            let inner_cmd = if let Some(stripped) = cmd.strip_prefix('$') {
                has_macro_params = true;
                stripped
            } else {
                cmd
            };

            let final_cmd = match inner_cmd.strip_prefix("execute ") {
                // Minecraft checks a modifier's conditions for every entity
                // before the first runs, so a condition of the body, as
                // a nested `if`, stays behind `run`: it is checked for each
                // entity when its turn comes, after the earlier ones ran.
                Some(inner_parts)
                    if !(Self::forks_contexts(execute_prefix)
                        && (inner_parts.starts_with("if ")
                            || inner_parts.starts_with("unless "))) =>
                {
                    format!("{} {}", execute_prefix, inner_parts)
                }
                _ => format!("{} run {}", execute_prefix, inner_cmd),
            };

            Self::mark_macro_line(final_cmd, has_macro_params)
        })
    }

    /// Whether an execute prefix can run what follows for several entities,
    /// as `as @a`, `at @e`, `on passengers`, or `summon` do.
    fn forks_contexts(prefix: &str) -> bool {
        prefix
            .split(' ')
            .any(|word| matches!(word, "as" | "at" | "on" | "summon"))
    }

    /// `as @a:` runs the helper once per entity. A `return` from one entity
    /// sets the function's return flag and ends only that entity's run, so
    /// the helper checks the flag first and skips the entities after it.
    fn stop_forks_after_return(
        &self,
        mut capture: FunctionCapture,
        body: &[Statement],
    ) -> FunctionCapture {
        let Some(context) = self.current_return.as_ref() else {
            return capture;
        };
        if matches!(
            Self::analyze_function_return_kind(body),
            FunctionReturnKind::None
        ) {
            return capture;
        }
        let guard = format!(
            "execute if score {} temp matches 1 run return 0",
            context.flag_score
        );
        capture.metadata = capture
            .metadata
            .into_iter()
            .map(|(index, generated)| (index + 1, generated))
            .collect();
        capture.metadata.insert(
            0,
            GeneratedCommand::new(
                guard.clone(),
                self.current_statement_source.clone(),
                GeneratedCommandKind::ControlFlow,
            ),
        );
        capture.commands.insert(0, guard);
        capture
    }

    /// Prefix `$` for a command in a macro context, but only when the line
    /// actually carries a `$(variable)`: Minecraft refuses to load a function
    /// with a macro line that has no variables ("No variables in macro").
    fn mark_macro_line(command: String, in_macro_context: bool) -> String {
        if in_macro_context && command.contains("$(") {
            format!("${}", command)
        } else {
            command
        }
    }

    /// Whether a raw command returns from the function it runs in
    /// (`/return ...`, or an `execute ... run return ...` chain).
    pub(in crate::transpiler) fn is_raw_return_command(command: &str) -> bool {
        let command = command
            .trim_start_matches('/')
            .trim_start_matches('$')
            .trim();
        command == "return" || command.starts_with("return ") || command.contains(" run return")
    }

    fn scope_condition_capture_for_execute(
        &mut self,
        condition: TranslatedCondition,
        capture: FunctionCapture,
        execute_parts: &[String],
    ) -> (TranslatedCondition, FunctionCapture) {
        let (mut conditions, capture) =
            self.scope_conditions_for_execute(vec![condition], capture, execute_parts);
        (conditions.remove(0), capture)
    }

    /// Inside an `execute as` block, conditions are evaluated once per
    /// entity, so the global fake players they are computed into
    /// (`expr_cond_temp_N temp` operands and `or_temp_N temp` OR flags) are
    /// rewritten to per-entity `@s` scores; otherwise one entity's result
    /// would decide the condition for every entity.
    fn scope_conditions_for_execute(
        &mut self,
        mut conditions: Vec<TranslatedCondition>,
        mut capture: FunctionCapture,
        execute_parts: &[String],
    ) -> (Vec<TranslatedCondition>, FunctionCapture) {
        if !Self::has_as_modifier(execute_parts) {
            return (conditions, capture);
        }

        // Scores that the condition reads, and scores that its setup
        // commands pass between each other (the left side of a
        // short-circuit `and`), hold one entity's value each.
        let mut holders: Vec<String> = Vec::new();
        let mut add_holders = |text: &str| {
            for holder in Self::condition_temp_holders(text) {
                if !holders.contains(&holder) {
                    holders.push(holder);
                }
            }
        };
        for condition in &conditions {
            condition.for_each_simple(&mut |text| add_holders(text));
        }
        for command in &capture.commands {
            add_holders(command);
        }
        for holder in holders {
            let objective = self.next_condition_scope_objective();
            self.data_pack.track_objective(&objective);
            let from = format!("{} temp", holder);
            let to = format!("@s {}", objective);

            for condition in &mut conditions {
                condition.map_simple(&|text| Self::replace_holder(text, &from, &to));
            }
            for command in &mut capture.commands {
                *command = Self::replace_holder(command, &from, &to);
            }
            for generated in capture.metadata.values_mut() {
                generated.text = Self::replace_holder(&generated.text, &from, &to);
            }
        }

        (conditions, capture)
    }

    /// Replace `from` (a `holder objective` pair) only where it starts a
    /// score holder, never inside a longer name (`xor_temp_1 temp`).
    fn replace_holder(text: &str, from: &str, to: &str) -> String {
        let mut result = String::with_capacity(text.len());
        let mut rest = text;
        while let Some(index) = rest.find(from) {
            let at_boundary = rest[..index]
                .chars()
                .next_back()
                .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
            let end = index + from.len();
            let ends_cleanly = rest[end..]
                .chars()
                .next()
                .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
            result.push_str(&rest[..index]);
            result.push_str(if at_boundary && ends_cleanly {
                to
            } else {
                from
            });
            rest = &rest[end..];
        }
        result.push_str(rest);
        result
    }

    /// Lower a structured (AND/NOT around OR) condition to flag-reading
    /// condition text at this point of the execute chain, with its setup
    /// commands run under the modifiers collected so far.
    fn lower_structured_execute_condition(
        &mut self,
        structured: TranslatedCondition,
        execute_parts: &[String],
        has_macro_params: &mut bool,
    ) -> Result<String, TranspileError> {
        let (rendered, capture) = self.capture_commands_with_result(|transpiler| {
            transpiler.lower_translated_condition(structured)
        })?;
        let (mut conditions, capture) = self.scope_conditions_for_execute(
            vec![TranslatedCondition::Simple(rendered)],
            capture,
            execute_parts,
        );
        self.append_scoped_execute_capture(capture, execute_parts, has_macro_params)?;
        let TranslatedCondition::Simple(rendered) = conditions.remove(0) else {
            unreachable!("scoping keeps the condition shape");
        };
        Ok(rendered)
    }

    /// Flatten a translated OR into its renderable alternatives for an
    /// execute condition, dropping impossible (always-false) alternatives.
    /// Structured leaves are lowered to flag conditions whose setup commands
    /// run under the execute modifiers collected so far.
    fn flatten_execute_or_alternatives(
        &mut self,
        condition: TranslatedCondition,
        execute_parts: &[String],
        has_macro_params: &mut bool,
    ) -> Result<Vec<String>, TranspileError> {
        let (leaves, capture) = self.capture_commands_with_result(|transpiler| {
            let mut leaves = Vec::new();
            transpiler.flatten_or_leaves(condition, &mut leaves)?;
            Ok(leaves)
        })?;
        let (leaves, capture) = self.scope_conditions_for_execute(leaves, capture, execute_parts);
        self.append_scoped_execute_capture(capture, execute_parts, has_macro_params)?;
        Ok(leaves
            .into_iter()
            .filter_map(|leaf| match leaf {
                TranslatedCondition::Simple(cond) => Some(cond),
                // An impossible alternative can never set the flag.
                _ => None,
            })
            .collect())
    }

    /// Split a raw (non-Python) execute condition on `separator` (` or ` or
    /// ` and `), honoring parenthesized groups, selector/NBT brackets, and
    /// quoted strings, so `@a[name="Tom or Jerry"]` stays one condition.
    fn split_raw_conditions(condition: &str, separator: &str) -> Vec<String> {
        let mut parts: Vec<String> = Vec::new();
        let mut start = 0;
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        let mut escaped = false;
        let mut chars = condition.char_indices().peekable();
        while let Some((index, ch)) = chars.next() {
            if let Some(active) = quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == active {
                    quote = None;
                }
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth = depth.saturating_sub(1),
                ' ' if depth == 0 && condition[index..].starts_with(separator) => {
                    let part = condition[start..index].trim();
                    if !part.is_empty() {
                        parts.push(part.to_string());
                    }
                    start = index + separator.len();
                    // Skip the rest of the separator so it is not re-split.
                    for _ in 1..separator.chars().count() {
                        chars.next();
                    }
                }
                _ => {}
            }
        }
        let part = condition[start..].trim();
        if !part.is_empty() {
            parts.push(part.to_string());
        }
        parts
    }

    /// Objective used to scope one condition holder to `@s` inside an
    /// `execute as` block. Objectives are pooled by (execute nesting depth,
    /// holder slot within the site): sibling sites reuse the same names because
    /// each site rewrites its holders before reading them, while nested sites
    /// get a deeper name so an inner evaluation cannot clobber the outer
    /// condition that is re-checked on every body command. This keeps the
    /// number of registered objectives bounded by the maximum number of
    /// concurrently live holders instead of the number of call sites.
    fn next_condition_scope_objective(&mut self) -> String {
        let depth = self.execute_condition_slots.len().saturating_sub(1);
        if let Some(slot) = self.execute_condition_slots.last_mut() {
            let index = *slot;
            *slot += 1;
            format!("cblc{}_{}", depth, index)
        } else {
            // Not inside process_execute_block; fall back to a unique name.
            format!("cblc{}", self.next_temp_id())
        }
    }

    /// Global condition holders read by `text`: lowered operands
    /// (`expr_cond_temp_N`) and materialized OR flags (`or_temp_N`).
    fn condition_temp_holders(text: &str) -> Vec<String> {
        let mut holders = Vec::new();
        for prefix in ["expr_cond_temp_", "or_temp_", "cond_and_", "cond_or_"] {
            let mut start = 0;
            while let Some(relative_index) = text[start..].find(prefix) {
                let holder_start = start + relative_index;
                let holder_end = text[holder_start..]
                    .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                    .map(|offset| holder_start + offset)
                    .unwrap_or(text.len());
                let at_boundary = text[..holder_start]
                    .chars()
                    .next_back()
                    .is_none_or(|ch| !(ch.is_alphanumeric() || ch == '_'));
                let holder = text[holder_start..holder_end].to_string();
                if at_boundary && !holders.contains(&holder) {
                    holders.push(holder);
                }
                start = holder_end;
            }
        }
        holders
    }

    fn append_scoped_execute_capture(
        &mut self,
        capture: FunctionCapture,
        execute_parts: &[String],
        has_macro_params: &mut bool,
    ) -> Result<(), TranspileError> {
        if capture.commands.is_empty() {
            return Ok(());
        }

        let modifier_args = Self::modifier_args(execute_parts);
        let params = self.current_context.params.clone();
        self.append_transformed_capture(capture, |cmd| {
            let mut command_needs_macro = false;
            let inner_cmd = if let Some(stripped) = cmd.strip_prefix('$') {
                command_needs_macro = true;
                *has_macro_params = true;
                stripped
            } else {
                cmd
            };

            let mut final_cmd = if modifier_args.is_empty() {
                inner_cmd.to_string()
            } else if let Some(inner_parts) = inner_cmd.strip_prefix("execute ") {
                format!("execute {} {}", modifier_args, inner_parts)
            } else {
                format!("execute {} run {}", modifier_args, inner_cmd)
            };

            if *has_macro_params {
                for param in &params {
                    let from = format!("{{{}}}", param);
                    let to = format!("$({})", param);
                    final_cmd = final_cmd.replace(&from, &to);
                }
            }

            Self::mark_macro_line(final_cmd, command_needs_macro || *has_macro_params)
        })
    }

    fn modifier_args(parts: &[String]) -> String {
        parts
            .iter()
            .filter(|part| part.as_str() != "execute")
            .cloned()
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn has_as_modifier(parts: &[String]) -> bool {
        parts.iter().any(|part| part.starts_with("as "))
    }

    fn execute_with_modifiers(modifier_args: &str, tail: &str) -> String {
        if modifier_args.is_empty() {
            format!("execute {}", tail)
        } else {
            format!("execute {} {}", modifier_args, tail)
        }
    }

    fn always_false_execute_condition(&mut self) -> String {
        self.data_pack.track_objective("temp");
        format!(
            "if score {holder} temp matches 0 unless score {holder} temp matches 0",
            holder = "#cobble_always_false"
        )
    }

    fn unless_and_conditions(condition: &str) -> Vec<String> {
        if let Some(conditions) = Self::split_prefixed_execute_conditions(condition) {
            return conditions;
        }

        Self::split_raw_conditions(condition, " and ")
            .iter()
            .map(|cond| {
                let fixed_cond = Self::normalize_condition_spacing(cond.trim());
                if fixed_cond.starts_with("if ") || fixed_cond.starts_with("unless ") {
                    fixed_cond
                } else {
                    format!("if {}", fixed_cond)
                }
            })
            .collect()
    }

    fn split_prefixed_execute_conditions(condition: &str) -> Option<Vec<String>> {
        let condition = condition.trim();
        if condition.starts_with("if ") || condition.starts_with("unless ") {
            Some(condition_clauses(condition))
        } else {
            None
        }
    }

    fn normalize_condition_spacing(condition: &str) -> String {
        if condition.contains("matches..") {
            condition.replace("matches..", "matches ..")
        } else if condition.contains("matches-") && !condition.contains("matches -") {
            condition.replace("matches-", "matches -")
        } else {
            condition.to_string()
        }
    }

    /// Add `unless <condition>` to the execute chain. A single clause is
    /// flipped in place; a multi-clause chain ("if A if B", meaning A and B)
    /// cannot be negated clause by clause, so it is deferred to an
    /// `UnlessAnd` flag that is set under the whole chain.
    fn push_negated_execute_condition(condition: String, chain: &mut ExecuteChain) {
        let clauses = condition_clauses(&condition);
        if clauses.len() > 1 {
            chain
                .deferred
                .push(DeferredExecuteCondition::UnlessAnd(clauses.join(" ")));
        } else {
            chain.parts.push(negate_clause(&condition));
        }
    }
}
