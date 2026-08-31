use crate::ast::*;
use crate::transpiler::condition_translator::TranslatedCondition;
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
                    // Check if selector contains a variable interpolation pattern {param}
                    let mut processed_selector =
                        if selector.starts_with('{') && selector.ends_with('}') {
                            let param_name = &selector[1..selector.len() - 1];
                            if self.current_context.is_param(param_name) {
                                has_macro_params = true;
                                format!("$({})", param_name)
                            } else {
                                selector.clone()
                            }
                        } else {
                            selector.clone()
                        };

                    // Replace selector aliases (@Name -> @a[...])
                    if processed_selector.starts_with('@') {
                        let selector_name = processed_selector.strip_prefix('@').unwrap_or("");
                        if let Some(actual_selector) = self.selector_aliases.get(selector_name) {
                            processed_selector = actual_selector.clone();
                        }
                    }

                    execute_parts.push(format!("as {}", processed_selector));
                }
                ExecuteModifier::At(selector) => {
                    // Check if selector contains a variable interpolation pattern {param}
                    let mut processed_selector =
                        if selector.starts_with('{') && selector.ends_with('}') {
                            let param_name = &selector[1..selector.len() - 1];
                            if self.current_context.is_param(param_name) {
                                has_macro_params = true;
                                format!("$({})", param_name)
                            } else {
                                selector.clone()
                            }
                        } else {
                            selector.clone()
                        };

                    // Replace selector aliases (@Name -> @a[...])
                    if processed_selector.starts_with('@') {
                        let selector_name = processed_selector.strip_prefix('@').unwrap_or("");
                        if let Some(actual_selector) = self.selector_aliases.get(selector_name) {
                            processed_selector = actual_selector.clone();
                        }
                    }

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
                                // AND/NOT around an OR: lower it to a flag
                                // condition right here.
                                let (rendered, or_capture) =
                                    self.capture_commands_with_result(|transpiler| {
                                        transpiler.lower_translated_condition(structured)
                                    })?;
                                self.append_scoped_execute_capture(
                                    or_capture,
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
                                    execute_parts.push(Self::negate_execute_condition(&cond));
                                }
                            }
                            // `unless` an impossible condition always passes.
                            TranslatedCondition::AlwaysFalse => {}
                            TranslatedCondition::Simple(translated) => {
                                if Self::is_prefixed_condition_chain(&translated) {
                                    special_conditions
                                        .push(DeferredExecuteCondition::UnlessAnd(translated));
                                } else {
                                    execute_parts.push(Self::negate_execute_condition(&translated));
                                }
                            }
                            structured => {
                                // AND/NOT around an OR: lower it to a flag
                                // condition, then negate the result.
                                let (rendered, or_capture) =
                                    self.capture_commands_with_result(|transpiler| {
                                        transpiler.lower_translated_condition(structured)
                                    })?;
                                self.append_scoped_execute_capture(
                                    or_capture,
                                    &execute_parts,
                                    &mut has_macro_params,
                                )?;
                                if Self::is_prefixed_condition_chain(&rendered) {
                                    special_conditions
                                        .push(DeferredExecuteCondition::UnlessAnd(rendered));
                                } else {
                                    execute_parts.push(Self::negate_execute_condition(&rendered));
                                }
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
                    if has_macro_params {
                        commands.push(format!("${}", command));
                    } else {
                        commands.push(command);
                    }
                }
            }
        }

        let execute_prefix = execute_parts.join(" ");

        // Process body statements
        for stmt in &exec_block.body {
            let capture = self.capture_statement(stmt)?;
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

                if has_macro_params {
                    format!("${}", final_cmd)
                } else {
                    final_cmd
                }
            })?;
        }

        Ok(())
    }

    fn scope_condition_capture_for_execute(
        &mut self,
        condition: TranslatedCondition,
        mut capture: FunctionCapture,
        execute_parts: &[String],
    ) -> (TranslatedCondition, FunctionCapture) {
        if !Self::has_as_modifier(execute_parts) {
            return (condition, capture);
        }

        let mut scoped_condition = condition;
        let mut holders: Vec<String> = Vec::new();
        scoped_condition.for_each_simple(&mut |text| {
            for holder in Self::condition_temp_holders(text) {
                if !holders.contains(&holder) {
                    holders.push(holder);
                }
            }
        });
        for holder in holders {
            let objective = self.next_condition_scope_objective();
            self.data_pack.track_objective(&objective);
            let from = format!("{} temp", holder);
            let to = format!("@s {}", objective);

            scoped_condition.map_simple(&|text| text.replace(&from, &to));
            for command in &mut capture.commands {
                *command = command.replace(&from, &to);
            }
            for generated in capture.metadata.values_mut() {
                generated.text = generated.text.replace(&from, &to);
            }
        }

        (scoped_condition, capture)
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

    fn condition_temp_holders(text: &str) -> Vec<String> {
        let mut holders = Vec::new();
        let mut start = 0;
        while let Some(relative_index) = text[start..].find("expr_cond_temp_") {
            let holder_start = start + relative_index;
            let holder_end = text[holder_start..]
                .find(|ch: char| !(ch.is_ascii_alphanumeric() || ch == '_'))
                .map(|offset| holder_start + offset)
                .unwrap_or(text.len());
            let holder = text[holder_start..holder_end].to_string();
            if !holders.contains(&holder) {
                holders.push(holder);
            }
            start = holder_end;
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

            if command_needs_macro || *has_macro_params {
                format!("${}", final_cmd)
            } else {
                final_cmd
            }
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

    fn is_prefixed_condition_chain(condition: &str) -> bool {
        Self::split_prefixed_execute_conditions(condition)
            .is_some_and(|conditions| conditions.len() > 1)
    }

    fn split_prefixed_execute_conditions(condition: &str) -> Option<Vec<String>> {
        let mut rest = condition.trim();
        let mut conditions = Vec::new();

        while !rest.is_empty() {
            let (prefix, after_prefix) = if let Some(after_prefix) = rest.strip_prefix("if ") {
                ("if", after_prefix)
            } else if let Some(after_prefix) = rest.strip_prefix("unless ") {
                ("unless", after_prefix)
            } else {
                return if conditions.is_empty() {
                    None
                } else {
                    Some(conditions)
                };
            };

            let next_if = after_prefix.find(" if ");
            let next_unless = after_prefix.find(" unless ");
            let next = match (next_if, next_unless) {
                (Some(if_index), Some(unless_index)) => Some(if_index.min(unless_index)),
                (Some(index), None) | (None, Some(index)) => Some(index),
                (None, None) => None,
            };

            if let Some(next) = next {
                let condition = after_prefix[..next].trim();
                conditions.push(format!("{} {}", prefix, condition));
                rest = after_prefix[next + 1..].trim_start();
            } else {
                let condition = after_prefix.trim();
                if !condition.is_empty() {
                    conditions.push(format!("{} {}", prefix, condition));
                }
                break;
            }
        }

        if conditions.is_empty() {
            None
        } else {
            Some(conditions)
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

    fn negate_execute_condition(condition: &str) -> String {
        if let Some(stripped) = condition.strip_prefix("if ") {
            format!("unless {}", stripped)
        } else if let Some(stripped) = condition.strip_prefix("unless ") {
            format!("if {}", stripped)
        } else {
            format!("unless {}", condition)
        }
    }
}
