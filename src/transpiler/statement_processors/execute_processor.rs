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
                    if condition.contains(" if ") || condition.contains(" unless ") {
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
                            // Add each condition as "unless"
                            if cond.starts_with("unless ") {
                                execute_parts.push(cond);
                            } else {
                                execute_parts.push(format!("unless {}", cond));
                            }
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

                    // translate_condition may return "unless ..." for != operator
                    if condition.starts_with("unless ") {
                        execute_parts.push(condition);
                    } else {
                        execute_parts.push(format!("unless {}", condition));
                    }
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
                                    if cond.starts_with("unless ") {
                                        execute_parts.push(cond);
                                    } else {
                                        execute_parts.push(format!("unless {}", cond));
                                    }
                                }
                                continue;
                            }
                            // Check if the translated condition already has "unless" prefix(es)
                            if translated.starts_with("unless ") {
                                execute_parts.push(translated);
                            } else {
                                execute_parts.push(format!("unless {}", translated));
                            }
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

        // Check if we have special conditions
        let has_or_condition = execute_parts.iter().any(|p| p.starts_with("OR_CONDITION:"));
        let has_unless_and = execute_parts.iter().any(|p| p.starts_with("UNLESS_AND:"));

        if has_unless_and {
            // Handle unless (A and B) - use temp variable
            // unless (A and B) = check if both are true, then unless that result
            let mut unless_and_str = None;
            let mut other_modifiers = Vec::new();

            for part in &execute_parts {
                if let Some(stripped) = part.strip_prefix("UNLESS_AND:") {
                    unless_and_str = Some(stripped);
                } else {
                    other_modifiers.push(part.clone());
                }
            }

            if let Some(and_str) = unless_and_str {
                // Generate a unique temp variable for unless AND result
                self.data_pack.track_objective("temp");
                let unless_var = format!("unless_temp_{}", self.get_unique_id());
                let modifier_args = Self::modifier_args(&other_modifiers);
                let score_holder = if Self::has_as_modifier(&other_modifiers) {
                    "@s".to_string()
                } else {
                    unless_var.clone()
                };

                // Initialize result to 0 (false)
                if let Some(ref mut commands) = self.current_function {
                    if modifier_args.is_empty() && score_holder != "@s" {
                        commands.push(format!("scoreboard players set {} temp 0", score_holder));
                    } else {
                        commands.push(Self::execute_with_modifiers(
                            &modifier_args,
                            &format!("run scoreboard players set {} temp 0", score_holder),
                        ));
                    }

                    // Set to 1 if ALL conditions are true (the AND check)
                    // Split the AND conditions and add "if" prefix to each
                    let and_conditions: Vec<String> = and_str
                        .split(" and ")
                        .map(|cond| {
                            let cond = cond.trim();
                            // Fix spacing for range operators
                            let fixed_cond = if cond.contains("matches..") {
                                cond.replace("matches..", "matches ..")
                            } else {
                                cond.to_string()
                            };
                            format!("if {}", fixed_cond)
                        })
                        .collect();

                    let and_check = and_conditions.join(" ");

                    let check_cmd = Self::execute_with_modifiers(
                        &modifier_args,
                        &format!(
                            "{} run scoreboard players set {} temp 1",
                            and_check, score_holder
                        ),
                    );

                    if has_macro_params {
                        commands.push(format!("${}", check_cmd));
                    } else {
                        commands.push(check_cmd);
                    }
                }

                // Now process body with the unless result check (unless the AND was true)
                let execute_prefix = Self::execute_with_modifiers(
                    &modifier_args,
                    &format!("unless score {} temp matches 1", score_holder),
                );

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

                        let final_cmd =
                            if let Some(inner_parts) = inner_cmd.strip_prefix("execute ") {
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
            }
        } else if has_or_condition {
            // Handle OR condition specially
            // Extract the OR condition and other modifiers
            let mut or_condition_str = None;
            let mut other_modifiers = Vec::new();

            for part in &execute_parts {
                if let Some(stripped) = part.strip_prefix("OR_CONDITION:") {
                    or_condition_str = Some(stripped);
                } else {
                    other_modifiers.push(part.clone());
                }
            }

            if let Some(or_str) = or_condition_str {
                // Process OR condition
                let or_conditions = Transpiler::flatten_or_conditions(or_str)?;
                let modifier_args = Self::modifier_args(&other_modifiers);

                // Generate a unique temp variable for this OR result
                self.data_pack.track_objective("temp");
                let or_var = format!("or_temp_{}", self.get_unique_id());
                let score_holder = if Self::has_as_modifier(&other_modifiers) {
                    "@s".to_string()
                } else {
                    or_var.clone()
                };

                // Initialize OR result to 0
                if let Some(ref mut commands) = self.current_function {
                    if modifier_args.is_empty() && score_holder != "@s" {
                        commands.push(format!("scoreboard players set {} temp 0", score_holder));
                    } else {
                        commands.push(Self::execute_with_modifiers(
                            &modifier_args,
                            &format!("run scoreboard players set {} temp 0", score_holder),
                        ));
                    }

                    // Check each OR condition
                    for cond in or_conditions {
                        let cond_prefix = if cond.starts_with("if ") || cond.starts_with("unless ")
                        {
                            cond.clone()
                        } else {
                            format!("if {}", cond)
                        };

                        // If any condition is true, set or_result to 1
                        let check_cmd = Self::execute_with_modifiers(
                            &modifier_args,
                            &format!(
                                "{} run scoreboard players set {} temp 1",
                                cond_prefix, score_holder
                            ),
                        );

                        if has_macro_params {
                            commands.push(format!("${}", check_cmd));
                        } else {
                            commands.push(check_cmd);
                        }
                    }
                }

                // Now process body with the OR result check
                let execute_prefix = Self::execute_with_modifiers(
                    &modifier_args,
                    &format!("if score {} temp matches 1", score_holder),
                );

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

                        let final_cmd =
                            if let Some(inner_parts) = inner_cmd.strip_prefix("execute ") {
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
            }
        } else {
            // Normal execute without OR condition
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
}
