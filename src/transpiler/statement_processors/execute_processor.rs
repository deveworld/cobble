use crate::ast::*;
use crate::transpiler::Transpiler;

impl Transpiler {
    pub(in crate::transpiler) fn process_execute_block(
        &mut self,
        exec_block: &ExecuteBlock,
    ) -> Result<(), String> {
        // Build execute command from modifiers
        let mut execute_parts = vec!["execute".to_string()];

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
                ExecuteModifier::If(expr) => {
                    // Python-style expression - translate to Minecraft condition
                    let condition = self.translate_condition(expr)?;

                    // Special handling for OR conditions
                    if condition.starts_with("OR(") {
                        // Mark that this execute block has OR condition
                        // We'll handle this specially when generating commands
                        execute_parts.push(format!("OR_CONDITION:{}", condition));
                        // Don't return error, continue processing
                        continue;
                    }

                    // Special handling for AND conditions which return multiple "if" parts
                    if Self::is_prefixed_condition_chain(&condition) {
                        // This is an AND condition that already includes multiple conditions
                        // Don't add another "if" prefix
                        execute_parts.push(condition);
                    } else if condition.starts_with("if ") || condition.starts_with("unless ") {
                        // Single condition with prefix
                        execute_parts.push(condition);
                    } else {
                        // Single condition without prefix
                        execute_parts.push(format!("if {}", condition));
                    }
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

                    // Check if this is actually a Python expression that needs translation
                    if self.looks_like_python_expression(condition) {
                        // Try to parse and translate as Python expression
                        if let Ok(translated) =
                            self.try_translate_python_expression(condition, false)
                        {
                            // Check if it's an OR condition marker
                            if translated.starts_with("OR(") {
                                // Mark as OR condition for special handling
                                execute_parts.push(format!("OR_CONDITION:{}", translated));
                                continue;
                            }
                            if translated == Self::ALWAYS_FALSE_CONDITION {
                                execute_parts.push(self.always_false_execute_condition());
                                continue;
                            }
                            // Check if the translated condition already has a prefix
                            // This happens with AND conditions ("if ... if ...") or != conditions ("unless ...")
                            if translated.starts_with("if ") || translated.starts_with("unless ") {
                                execute_parts.push(translated);
                            } else {
                                execute_parts.push(format!("if {}", translated));
                            }
                        } else {
                            // Translation failed - this is a Python expression we can't handle
                            return Err(format!(
                                "Failed to translate Python expression '{}' to Minecraft condition.\n\
                                 This may be an unsupported construct like OR conditions.\n\
                                 Consider rewriting the condition or using raw Minecraft syntax.",
                                condition
                            ));
                        }
                    } else {
                        // Raw Minecraft syntax - check for AND/OR conditions
                        if condition.contains(" or ") {
                            // OR conditions need special handling with temp variables
                            // Mark as OR condition for later processing
                            execute_parts.push(format!(
                                "OR_CONDITION:OR({})",
                                condition.replace(" or ", ";")
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
                ExecuteModifier::Unless(expr) => {
                    // Python-style expression - translate to Minecraft condition
                    let condition = self.translate_condition(expr)?;

                    // Special handling for OR conditions
                    if condition.starts_with("OR(") {
                        // unless (A or B) = unless A and unless B (De Morgan's law)
                        // We can chain unless conditions in Minecraft
                        let or_conditions = Transpiler::flatten_or_conditions(&condition)?;
                        for cond in or_conditions {
                            execute_parts.push(Self::negate_execute_condition(&cond));
                        }
                        continue;
                    }

                    // Special handling for AND conditions
                    if condition.contains(" if ") || condition.contains(" unless ") {
                        // This is an AND condition that has "if" parts
                        // For unless (A and B), we apply De Morgan's law: unless (A and B) = unless A or unless B
                        // Since Minecraft doesn't support OR directly, we need to use a flag variable
                        execute_parts.push(format!("UNLESS_AND:{}", condition));
                        continue;
                    }

                    execute_parts.push(Self::negate_execute_condition(&condition));
                }
                ExecuteModifier::UnlessRaw(condition) => {
                    // Check if this is actually a Python expression that needs translation
                    if self.looks_like_python_expression(condition) {
                        // Try to parse and translate as Python expression
                        if let Ok(translated) =
                            self.try_translate_python_expression(condition, true)
                        {
                            // Check if it's an OR condition marker
                            if translated.starts_with("OR(") {
                                // unless (A or B) = unless A and unless B (De Morgan's law)
                                let or_conditions = Transpiler::flatten_or_conditions(&translated)?;
                                for cond in or_conditions {
                                    if cond == Self::ALWAYS_FALSE_CONDITION {
                                        continue;
                                    }
                                    execute_parts.push(Self::negate_execute_condition(&cond));
                                }
                                continue;
                            }
                            if translated == Self::ALWAYS_FALSE_CONDITION {
                                continue;
                            }
                            if Self::is_prefixed_condition_chain(&translated) {
                                execute_parts.push(format!("UNLESS_AND:{}", translated));
                                continue;
                            }
                            execute_parts.push(Self::negate_execute_condition(&translated));
                        } else {
                            // Translation failed - this is a Python expression we can't handle
                            return Err(format!(
                                "Failed to translate Python expression '{}' to Minecraft condition.\n\
                                 This may be an unsupported construct.\n\
                                 Consider rewriting the condition or using raw Minecraft syntax.",
                                condition
                            ));
                        }
                    } else {
                        // Raw Minecraft syntax - check for AND conditions
                        if condition.contains(" and ") {
                            // Split on " and " and create multiple unless conditions
                            // unless (A and B) requires special handling with temp variable
                            execute_parts.push(format!("UNLESS_AND:{}", condition));
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
        }

        let mut final_execute_parts = Vec::new();
        let mut special_conditions = Vec::new();
        for part in &execute_parts {
            if let Some(stripped) = part.strip_prefix("OR_CONDITION:") {
                special_conditions.push(("or", stripped.to_string()));
            } else if let Some(stripped) = part.strip_prefix("UNLESS_AND:") {
                special_conditions.push(("unless_and", stripped.to_string()));
            } else {
                final_execute_parts.push(part.clone());
            }
        }

        if !special_conditions.is_empty() {
            let modifier_args = Self::modifier_args(&final_execute_parts);
            let has_as_modifier = Self::has_as_modifier(&final_execute_parts);
            let use_dedicated_objectives = special_conditions.len() > 1;
            let mut setup_commands = Vec::new();

            for (kind, condition) in special_conditions {
                let unique_id = self.get_unique_id();
                let objective = if use_dedicated_objectives {
                    format!("cblx{}", unique_id)
                } else {
                    "temp".to_string()
                };
                self.data_pack.track_objective(&objective);

                let score_holder = if has_as_modifier {
                    "@s".to_string()
                } else if use_dedicated_objectives {
                    "#cobble_exec".to_string()
                } else if kind == "or" {
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

                if kind == "or" {
                    for cond in Transpiler::flatten_or_conditions(&condition)? {
                        if cond == Self::ALWAYS_FALSE_CONDITION {
                            continue;
                        }
                        let cond_prefix = if cond.starts_with("if ") || cond.starts_with("unless ")
                        {
                            cond
                        } else {
                            format!("if {}", cond)
                        };
                        setup_commands.push(Self::execute_with_modifiers(
                            &modifier_args,
                            &format!(
                                "{} run scoreboard players set {} {} 1",
                                cond_prefix, score_holder, objective
                            ),
                        ));
                    }
                    final_execute_parts
                        .push(format!("if score {} {} matches 1", score_holder, objective));
                } else {
                    let and_conditions = Self::unless_and_conditions(&condition);
                    let and_check = and_conditions.join(" ");
                    setup_commands.push(Self::execute_with_modifiers(
                        &modifier_args,
                        &format!(
                            "{} run scoreboard players set {} {} 1",
                            and_check, score_holder, objective
                        ),
                    ));
                    final_execute_parts.push(format!(
                        "unless score {} {} matches 1",
                        score_holder, objective
                    ));
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

        let execute_prefix = final_execute_parts.join(" ");

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
