use crate::ast::*;
use crate::transpiler::condition_translator::{
    condition_clauses, negate_clause, TranslatedCondition,
};
use crate::transpiler::{FunctionCapture, TranspileError, Transpiler};

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
    fn substitute_macro_param(&mut self, from: &str, to: &str) {
        match self {
            Self::Or(alternatives) => {
                for alternative in alternatives {
                    *alternative = alternative.replace(from, to);
                }
            }
            Self::UnlessAnd(condition) => *condition = condition.replace(from, to),
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
        if let Some(actual_selector) = self.selector_aliases.get(selector_name) {
            return Ok(actual_selector.clone());
        }
        let variable = selector_name.split('[').next().unwrap_or("");
        if !matches!(variable, "a" | "e" | "p" | "r" | "s" | "n") {
            let hint = if self.entity_templates.contains_key(selector_name) {
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
        // Build execute command from modifiers
        let mut execute_parts = vec!["execute".to_string()];
        let mut special_conditions: Vec<DeferredExecuteCondition> = Vec::new();

        let mut has_macro_params = false;
        for modifier in &exec_block.modifiers {
            match modifier {
                ExecuteModifier::As(selector) => {
                    let processed_selector =
                        self.resolve_execute_selector(selector, &mut has_macro_params)?;
                    execute_parts.push(format!("as {}", processed_selector));
                }
                ExecuteModifier::At(selector) => {
                    let processed_selector =
                        self.resolve_execute_selector(selector, &mut has_macro_params)?;
                    execute_parts.push(format!("at {}", processed_selector));
                }
                ExecuteModifier::IfRaw(condition) => {
                    // Check for macro parameters in the condition
                    if condition.contains("{") && condition.contains("}") {
                        // Check if any of the {var} patterns are macro parameters
                        for param in &self.current_context.params {
                            if condition.contains(&format!("{{{}}}", param)) {
                                has_macro_params = true;
                                break;
                            }
                        }
                    }

                    if let Some((translated, capture)) =
                        self.capture_raw_python_condition(condition)?
                    {
                        let (translated, capture) = self.scope_condition_capture_for_execute(
                            translated,
                            capture,
                            &execute_parts,
                        );
                        self.append_scoped_execute_capture(
                            capture,
                            &execute_parts,
                            &mut has_macro_params,
                        )?;

                        match translated {
                            TranslatedCondition::AnyOf(..) => {
                                let alternatives = self.flatten_execute_or_alternatives(
                                    translated,
                                    &execute_parts,
                                    &mut has_macro_params,
                                )?;
                                special_conditions.push(DeferredExecuteCondition::Or(alternatives));
                            }
                            TranslatedCondition::AlwaysFalse => {
                                let always_false = self.always_false_execute_condition();
                                execute_parts.push(always_false);
                            }
                            TranslatedCondition::Simple(translated) => {
                                execute_parts.push(Self::condition_execute_args(&translated));
                            }
                            structured => {
                                // AND around an OR: lower it to a flag
                                // condition right here.
                                let rendered = self.lower_structured_execute_condition(
                                    structured,
                                    &execute_parts,
                                    &mut has_macro_params,
                                )?;
                                execute_parts.push(Self::condition_execute_args(&rendered));
                            }
                        }
                        continue;
                    }

                    // The condition looked like a Python expression but did not
                    // parse; there is no string-based fallback translator anymore.
                    if self.looks_like_python_expression(condition) {
                        return Err(format!(
                            "Failed to translate Python expression '{}' to Minecraft condition.\n\
                             This may be an unsupported construct like OR conditions.\n\
                             Consider rewriting the condition or using raw Minecraft syntax.",
                            condition
                        )
                        .into());
                    } else {
                        // Raw Minecraft syntax - check for AND/OR conditions
                        if condition.contains(" or ") {
                            // OR conditions need special handling with temp variables
                            special_conditions.push(DeferredExecuteCondition::Or(
                                Self::split_raw_or_conditions(condition),
                            ));
                        } else if condition.contains(" and ") {
                            // Split on " and " and create multiple if conditions
                            let parts: Vec<&str> = condition.split(" and ").collect();
                            for part in parts {
                                let part = part.trim();
                                // Fix spacing for range operators
                                let fixed_part = if part.contains("matches..") {
                                    part.replace("matches..", "matches ..")
                                } else if part.contains("matches-") && !part.contains("matches -") {
                                    part.replace("matches-", "matches -")
                                } else {
                                    part.to_string()
                                };
                                execute_parts.push(format!("if {}", fixed_part));
                            }
                        } else {
                            // Fix spacing for range operators in single conditions too
                            let fixed_condition = if condition.contains("matches..") {
                                condition.replace("matches..", "matches ..")
                            } else if condition.contains("matches-")
                                && !condition.contains("matches -")
                            {
                                condition.replace("matches-", "matches -")
                            } else {
                                condition.to_string()
                            };
                            execute_parts.push(format!("if {}", fixed_condition));
                        }
                    }
                }
                ExecuteModifier::UnlessRaw(condition) => {
                    if let Some((translated, capture)) =
                        self.capture_raw_python_condition(condition)?
                    {
                        let (translated, capture) = self.scope_condition_capture_for_execute(
                            translated,
                            capture,
                            &execute_parts,
                        );
                        self.append_scoped_execute_capture(
                            capture,
                            &execute_parts,
                            &mut has_macro_params,
                        )?;

                        match translated {
                            TranslatedCondition::AnyOf(..) => {
                                // unless (A or B) = unless A and unless B (De Morgan's law)
                                let alternatives = self.flatten_execute_or_alternatives(
                                    translated,
                                    &execute_parts,
                                    &mut has_macro_params,
                                )?;
                                for cond in alternatives {
                                    Self::push_negated_execute_condition(
                                        cond,
                                        &mut execute_parts,
                                        &mut special_conditions,
                                    );
                                }
                            }
                            // `unless` an impossible condition always passes.
                            TranslatedCondition::AlwaysFalse => {}
                            TranslatedCondition::Simple(translated) => {
                                Self::push_negated_execute_condition(
                                    translated,
                                    &mut execute_parts,
                                    &mut special_conditions,
                                );
                            }
                            structured => {
                                // AND around an OR: lower it to a flag
                                // condition, then negate the result.
                                let rendered = self.lower_structured_execute_condition(
                                    structured,
                                    &execute_parts,
                                    &mut has_macro_params,
                                )?;
                                Self::push_negated_execute_condition(
                                    rendered,
                                    &mut execute_parts,
                                    &mut special_conditions,
                                );
                            }
                        }
                        continue;
                    }

                    // The condition looked like a Python expression but did not
                    // parse; there is no string-based fallback translator anymore.
                    if self.looks_like_python_expression(condition) {
                        return Err(format!(
                            "Failed to translate Python expression '{}' to Minecraft condition.\n\
                             This may be an unsupported construct.\n\
                             Consider rewriting the condition or using raw Minecraft syntax.",
                            condition
                        )
                        .into());
                    } else {
                        // Raw Minecraft syntax - check for AND conditions
                        if condition.contains(" and ") {
                            // Split on " and " and create multiple unless conditions
                            // unless (A and B) requires special handling with temp variable
                            special_conditions
                                .push(DeferredExecuteCondition::UnlessAnd(condition.clone()));
                        } else {
                            // Fix spacing for range operators
                            let fixed_condition = if condition.contains("matches..") {
                                condition.replace("matches..", "matches ..")
                            } else if condition.contains("matches-")
                                && !condition.contains("matches -")
                            {
                                condition.replace("matches-", "matches -")
                            } else {
                                condition.to_string()
                            };
                            execute_parts.push(format!("unless {}", fixed_condition));
                        }
                    }
                }
                ExecuteModifier::Positioned(pos) => {
                    // Workaround for parser bug: positioned might incorrectly contain "if" conditions
                    if pos.contains(" if score ") {
                        // Split at " if " to separate positioned from condition
                        if let Some(if_pos) = pos.find(" if score ") {
                            let actual_pos = &pos[..if_pos];
                            let condition = &pos[if_pos + 4..]; // Skip " if "

                            execute_parts.push(format!("positioned {}", actual_pos));

                            // Process the condition part
                            if condition.contains(" and ") {
                                // Split AND conditions
                                let parts: Vec<&str> = condition.split(" and ").collect();
                                for part in parts {
                                    let part = part.trim();
                                    let fixed_part = if part.contains("matches..") {
                                        part.replace("matches..", "matches ..")
                                    } else {
                                        part.to_string()
                                    };
                                    execute_parts.push(format!("if {}", fixed_part));
                                }
                            } else {
                                let fixed_condition = if condition.contains("matches..") {
                                    condition.replace("matches..", "matches ..")
                                } else {
                                    condition.to_string()
                                };
                                execute_parts.push(format!("if {}", fixed_condition));
                            }
                        } else {
                            execute_parts.push(format!("positioned {}", pos));
                        }
                    } else {
                        execute_parts.push(format!("positioned {}", pos));
                    }
                }
                ExecuteModifier::Rotated(rot) => {
                    execute_parts.push(format!("rotated {}", rot));
                }
                ExecuteModifier::In(dimension) => {
                    execute_parts.push(format!("in {}", dimension));
                }
                ExecuteModifier::Anchored(anchor) => {
                    execute_parts.push(format!("anchored {}", anchor));
                }
                ExecuteModifier::Align(axes) => {
                    execute_parts.push(format!("align {}", axes));
                }
                ExecuteModifier::Store(store_cmd) => {
                    execute_parts.push(format!("store {}", store_cmd));
                }
            }
        }

        // If we have macro params, replace {param} with $(param) in execute parts
        if has_macro_params {
            execute_parts = execute_parts
                .into_iter()
                .map(|part| {
                    let mut result = part.clone();
                    for param in &self.current_context.params {
                        let from = format!("{{{}}}", param);
                        let to = format!("$({})", param);
                        result = result.replace(&from, &to);
                    }
                    result
                })
                .collect();
            for param in &self.current_context.params {
                let from = format!("{{{}}}", param);
                let to = format!("$({})", param);
                for special in &mut special_conditions {
                    special.substitute_macro_param(&from, &to);
                }
            }
        }

        if !special_conditions.is_empty() {
            let modifier_args = Self::modifier_args(&execute_parts);
            let has_as_modifier = Self::has_as_modifier(&execute_parts);
            let use_dedicated_objectives = special_conditions.len() > 1;
            let mut setup_commands = Vec::new();

            for special in special_conditions {
                let unique_id = self.next_temp_id();
                let objective = if use_dedicated_objectives {
                    format!("cblx{}", unique_id)
                } else {
                    "temp".to_string()
                };
                self.data_pack.track_objective(&objective);

                let is_or = matches!(special, DeferredExecuteCondition::Or(_));
                let score_holder = if has_as_modifier {
                    "@s".to_string()
                } else if use_dedicated_objectives {
                    "#cobble_exec".to_string()
                } else if is_or {
                    format!("or_temp_{}", unique_id)
                } else {
                    format!("unless_temp_{}", unique_id)
                };

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

                match special {
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
                        execute_parts
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
                        execute_parts.push(format!(
                            "unless score {} {} matches 1",
                            score_holder, objective
                        ));
                    }
                }
            }

            if let Some(ref mut commands) = self.current_function {
                for command in setup_commands {
                    commands.push(Self::mark_macro_line(command, has_macro_params));
                }
            }
        }

        let execute_prefix = execute_parts.join(" ");

        // Capture the whole body first; like `if`, the number of commands it
        // produced decides between inlining and routing through a helper.
        // Prefixing several commands with the execute chain re-evaluates the
        // conditions and re-forks the selector for every command: an earlier
        // command that changes the condition cuts off the rest of the body,
        // and under a multi-entity `as` every entity runs the first command
        // before any entity runs the second, so shared state (loop counters,
        // if/else flags, scratch scores) breaks. A helper runs the whole body
        // once per execution context instead.
        //
        // `store` blocks keep the per-command form: storing a helper call
        // would store the helper's return value rather than each command's
        // result.
        let saved_context = self.current_context.clone();
        let capture = self.capture_statements(&exec_block.body)?;
        self.current_context = saved_context;

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
        if capture.commands.len() > 1
            && !has_store_modifier
            && !has_raw_return
            && self.current_function.is_some()
        {
            let helper_name = format!("execute_temp_{}", self.next_temp_id());
            let needs_storage = capture.requires_macro_context();
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
            return Ok(());
        }

        self.append_transformed_capture(capture, |cmd| {
            let inner_cmd = if let Some(stripped) = cmd.strip_prefix('$') {
                has_macro_params = true;
                stripped
            } else {
                cmd
            };

            let final_cmd = if let Some(inner_parts) = inner_cmd.strip_prefix("execute ") {
                format!("{} {}", execute_prefix, inner_parts)
            } else {
                format!("{} run {}", execute_prefix, inner_cmd)
            };

            Self::mark_macro_line(final_cmd, has_macro_params)
        })?;

        Ok(())
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
    fn is_raw_return_command(command: &str) -> bool {
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

        let mut holders: Vec<String> = Vec::new();
        for condition in &conditions {
            condition.for_each_simple(&mut |text| {
                for holder in Self::condition_temp_holders(text) {
                    if !holders.contains(&holder) {
                        holders.push(holder);
                    }
                }
            });
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

    /// Split a raw (non-Python) execute condition on ` or `, honoring
    /// parenthesized groups, into its alternative conditions.
    fn split_raw_or_conditions(condition: &str) -> Vec<String> {
        let joined = condition.replace(" or ", ";");
        let mut parts: Vec<String> = Vec::new();
        let mut current = String::new();
        let mut depth = 0i32;
        for ch in joined.chars() {
            match ch {
                '(' => {
                    depth += 1;
                    current.push(ch);
                }
                ')' => {
                    depth -= 1;
                    current.push(ch);
                }
                ';' if depth == 0 => {
                    if !current.is_empty() {
                        parts.push(current.trim().to_string());
                        current.clear();
                    }
                }
                _ => current.push(ch),
            }
        }
        if !current.is_empty() {
            parts.push(current.trim().to_string());
        }
        parts.retain(|part| !part.is_empty());
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
        for prefix in ["expr_cond_temp_", "or_temp_"] {
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

        condition
            .split(" and ")
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
    fn push_negated_execute_condition(
        condition: String,
        execute_parts: &mut Vec<String>,
        special_conditions: &mut Vec<DeferredExecuteCondition>,
    ) {
        let clauses = condition_clauses(&condition);
        if clauses.len() > 1 {
            special_conditions.push(DeferredExecuteCondition::UnlessAnd(clauses.join(" ")));
        } else {
            execute_parts.push(negate_clause(&condition));
        }
    }
}
