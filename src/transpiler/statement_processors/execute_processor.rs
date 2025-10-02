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
                    let mut processed_selector = if selector.starts_with('{') && selector.ends_with('}') {
                        let param_name = &selector[1..selector.len()-1];
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
                    let mut processed_selector = if selector.starts_with('{') && selector.ends_with('}') {
                        let param_name = &selector[1..selector.len()-1];
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
                    // Check if condition already has "if" or "unless" prefix
                    if condition.starts_with("if ") || condition.starts_with("unless ") {
                        execute_parts.push(condition);
                    } else {
                        execute_parts.push(format!("if {}", condition));
                    }
                }
                ExecuteModifier::IfRaw(condition) => {
                    // Raw Minecraft syntax - use as-is
                    execute_parts.push(format!("if {}", condition));
                }
                ExecuteModifier::Unless(expr) => {
                    // Python-style expression - translate to Minecraft condition
                    let condition = self.translate_condition(expr)?;
                    // translate_condition may return "unless ..." for != operator
                    if condition.starts_with("unless ") {
                        execute_parts.push(condition);
                    } else {
                        execute_parts.push(format!("unless {}", condition));
                    }
                }
                ExecuteModifier::UnlessRaw(condition) => {
                    // Raw Minecraft syntax - use as-is
                    execute_parts.push(format!("unless {}", condition));
                }
                ExecuteModifier::Positioned(pos) => {
                    execute_parts.push(format!("positioned {}", pos));
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

        let execute_prefix = execute_parts.join(" ");

        // Process body statements
        for stmt in &exec_block.body {
            // Save current function
            let saved_function = self.current_function.take();
            self.current_function = Some(Vec::new());

            self.process_statement(stmt)?;

            // Get generated commands
            if let Some(stmt_commands) = self.current_function.take() {
                self.current_function = saved_function;

                // Prepend execute modifiers to each command
                for cmd in stmt_commands {
                    if let Some(ref mut commands) = self.current_function {
                        // Strip $ prefix from inner command if present (we'll add it at the start)
                        let inner_cmd = if let Some(stripped) = cmd.strip_prefix('$') {
                            has_macro_params = true; // Inner command has macros
                            stripped // Strip the $
                        } else {
                            &cmd
                        };

                        let final_cmd = format!("{} run {}", execute_prefix, inner_cmd);

                        // Add $ prefix at START of entire command if any part needs macros
                        if has_macro_params {
                            commands.push(format!("${}", final_cmd));
                        } else {
                            commands.push(final_cmd);
                        }
                    }
                }
            } else {
                self.current_function = saved_function;
            }
        }

        Ok(())
    }
}
