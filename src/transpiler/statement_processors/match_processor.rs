use crate::ast::*;
use crate::transpiler::{FunctionCapture, Transpiler};

struct PreparedMatchCase {
    body_len: usize,
    needs_storage: bool,
    capture: FunctionCapture,
}

impl Transpiler {
    pub(in crate::transpiler) fn process_match(
        &mut self,
        match_stmt: &MatchStatement,
    ) -> Result<(), String> {
        // Create a temporary variable to hold the match value
        let temp_var = format!("match_temp_{}", self.temp_counter);
        self.temp_counter += 1;

        // Evaluate the match expression into the temporary variable
        self.data_pack.track_objective("temp");
        match &match_stmt.value {
            Expression::Identifier(name) => {
                let obj = self
                    .variable_objectives
                    .get(name)
                    .unwrap_or(&"temp".to_string())
                    .clone();
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!(
                        "scoreboard players operation {} temp = {} {}",
                        temp_var, name, obj
                    ));
                }
            }
            Expression::Number(n) => {
                // If it's a constant, assign it to temp variable
                let value = *n as i32;
                if let Some(ref mut commands) = self.current_function {
                    commands.push(format!(
                        "scoreboard players set {} temp {}",
                        temp_var, value
                    ));
                }
            }
            _ => {
                // Complex expression - evaluate it first
                let commands = self.evaluate_expression_to_target(&match_stmt.value, &temp_var)?;
                if let Some(ref mut cmds) = self.current_function {
                    cmds.extend(commands);
                }
            }
        }

        self.process_match_with_var(&temp_var, "temp", match_stmt)
    }

    fn process_match_with_var(
        &mut self,
        result_var: &str,
        result_obj: &str,
        match_stmt: &MatchStatement,
    ) -> Result<(), String> {
        // Convert cases to (min, max, body_idx) tuples
        let mut cases: Vec<(i32, i32, usize)> = Vec::new();
        let mut wildcard_case: Option<usize> = None;

        for (idx, case) in match_stmt.cases.iter().enumerate() {
            match &case.pattern {
                MatchPattern::Literal(val) => {
                    cases.push((*val, *val, idx));
                }
                MatchPattern::Range(start, end) => {
                    cases.push((*start, *end, idx));
                }
                MatchPattern::Wildcard => {
                    // Wildcard case handled separately
                    wildcard_case = Some(idx);
                }
            }
        }

        // Sort cases by min value for the 4-way split algorithm
        cases.sort_by_key(|(min, _, _)| *min);

        // Validate that no cases overlap (like CBScript does)
        if !cases.is_empty() {
            let mut prev_max = i32::MIN;
            for (min, max, _case_idx) in &cases {
                if *min > *max {
                    return Err(format!(
                        "Invalid match case range: {} to {}.\n\n\
                        The start value must be less than or equal to the end value.",
                        min, max
                    ));
                }

                if *min <= prev_max {
                    let case_range = if *min == *max {
                        format!("{}", min)
                    } else {
                        format!("{} to {}", min, max)
                    };

                    return Err(format!(
                        "Match case overlap detected: case {} overlaps with a previous case.\n\n\
                        Previous case ended at: {}\n\
                        Current case starts at: {}\n\n\
                        Each case in a match statement must have non-overlapping ranges.\n\
                        Match statements execute the FIRST matching case only.\n\n\
                        To fix: Ensure all case ranges are mutually exclusive.",
                        case_range, prev_max, min
                    ));
                }

                prev_max = *max;
            }
        }

        // Generate switch tree using 4-way split algorithm
        let mut prepared_cases = self.prepare_match_cases(match_stmt, &cases, wildcard_case)?;
        if !cases.is_empty() {
            self.generate_switch_tree(result_var, result_obj, &cases, &mut prepared_cases)?;
        }

        // Handle wildcard case if present (executes if no other case matched)
        if let Some(wildcard_idx) = wildcard_case {
            let prepared_case = Self::take_prepared_match_case(&mut prepared_cases, wildcard_idx)?;

            // Generate condition that excludes all ranges
            let mut unless_conditions = Vec::new();
            for (min, max, _) in &cases {
                unless_conditions.push(format!(
                    "unless score {} {} matches {}..{}",
                    result_var, result_obj, min, max
                ));
            }

            if prepared_case.body_len == 1 && !prepared_case.needs_storage {
                // Inline single statement with condition
                self.append_transformed_capture(prepared_case.capture, |cmd| {
                    let clean_cmd = Self::strip_command_prefix(cmd);
                    if unless_conditions.is_empty() {
                        clean_cmd
                    } else {
                        format!("execute {} run {}", unless_conditions.join(" "), clean_cmd)
                    }
                })?;
            } else {
                // Create function for multi-statement wildcard
                let func_name = format!("match_default_{}", self.temp_counter);
                self.temp_counter += 1;

                let needs_storage = prepared_case.needs_storage;
                self.add_captured_function(func_name.clone(), prepared_case.capture);

                // Call function only if no other case matched
                let call_cmd = self.function_call_command(&func_name, needs_storage);
                if let Some(ref mut commands) = self.current_function {
                    if unless_conditions.is_empty() {
                        commands.push(call_cmd);
                    } else {
                        // Chain all unless conditions in a single execute command
                        commands.push(format!(
                            "execute {} run {}",
                            unless_conditions.join(" "),
                            call_cmd
                        ));
                    }
                }
            }
        }

        Ok(())
    }

    /// Generate switch tree using 4-way split algorithm (CBScript algorithm)
    fn generate_switch_tree(
        &mut self,
        var: &str,
        obj: &str,
        cases: &[(i32, i32, usize)],
        prepared_cases: &mut [Option<PreparedMatchCase>],
    ) -> Result<(), String> {
        if cases.is_empty() {
            return Ok(());
        }

        // Base case: single case
        if cases.len() == 1 {
            let (min, max, case_idx) = cases[0];
            let prepared_case = Self::take_prepared_match_case(prepared_cases, case_idx)?;

            if prepared_case.body_len == 1 && !prepared_case.needs_storage {
                // Inline single command
                if self.current_function.is_some() {
                    let range = if min == max {
                        format!("{}", min)
                    } else {
                        format!("{}..{}", min, max)
                    };

                    self.append_transformed_capture(prepared_case.capture, |cmd| {
                        if let Some(inner_parts) = cmd.strip_prefix("execute ") {
                            format!(
                                "execute if score {} {} matches {} {}",
                                var, obj, range, inner_parts
                            )
                        } else {
                            format!(
                                "execute if score {} {} matches {} run {}",
                                var, obj, range, cmd
                            )
                        }
                    })?;
                }
            } else {
                // Create function for multi-statement case
                let func_name = format!("match_case_{}_{}", min, self.temp_counter);
                self.temp_counter += 1;

                let needs_storage = prepared_case.needs_storage;
                self.add_captured_function(func_name.clone(), prepared_case.capture);

                let range = if min == max {
                    format!("{}", min)
                } else {
                    format!("{}..{}", min, max)
                };
                let call_cmd = self.function_call_command(&func_name, needs_storage);
                let call_cmd = format!(
                    "execute if score {} {} matches {} run {}",
                    var, obj, range, call_cmd
                );
                if let Some(ref mut commands) = self.current_function {
                    commands.push(call_cmd);
                }
            }
            return Ok(());
        }

        // Recursive case: split into 4 quarters
        for q in 0..4 {
            let imin = q * cases.len() / 4;
            let imax = (q + 1) * cases.len() / 4;

            if imin >= imax {
                continue;
            }

            let sub_cases = &cases[imin..imax];
            let vmin = sub_cases[0].0;
            let vmax = sub_cases[sub_cases.len() - 1].1;

            if sub_cases.len() == 1 {
                // Single case in this quarter - process directly
                self.generate_switch_tree(var, obj, sub_cases, prepared_cases)?;
            } else {
                // Multiple cases - create a function and recursively process
                let func_name = format!("match_switch_{}_{}", vmin, self.temp_counter);
                self.temp_counter += 1;

                // Recursively generate the switch tree in the new function
                let capture = self.capture_commands(|transpiler| {
                    transpiler.generate_switch_tree(var, obj, sub_cases, prepared_cases)
                })?;

                // Add the generated function to the data pack
                self.add_captured_function(func_name.clone(), capture);

                // Call the function with range condition
                if let Some(ref mut commands) = self.current_function {
                    let call_cmd = format!(
                        "execute if score {} {} matches {}..{} run function {}:{}",
                        var, obj, vmin, vmax, self.data_pack.namespace, func_name
                    );
                    commands.push(call_cmd);
                }
            }
        }

        Ok(())
    }

    fn prepare_match_cases(
        &mut self,
        match_stmt: &MatchStatement,
        cases: &[(i32, i32, usize)],
        wildcard_case: Option<usize>,
    ) -> Result<Vec<Option<PreparedMatchCase>>, String> {
        let mut prepared_cases = Vec::with_capacity(match_stmt.cases.len());
        let mut should_prepare = vec![false; match_stmt.cases.len()];
        for (_, _, case_idx) in cases {
            should_prepare[*case_idx] = true;
        }
        if let Some(wildcard_idx) = wildcard_case {
            should_prepare[wildcard_idx] = true;
        }

        for (idx, case) in match_stmt.cases.iter().enumerate() {
            if !should_prepare[idx] {
                prepared_cases.push(None);
                continue;
            }
            let capture = self.capture_statements_isolated(&case.body)?;
            let needs_storage = capture.requires_macro_context();
            prepared_cases.push(Some(PreparedMatchCase {
                body_len: case.body.len(),
                needs_storage,
                capture,
            }));
        }

        Ok(prepared_cases)
    }

    fn take_prepared_match_case(
        prepared_cases: &mut [Option<PreparedMatchCase>],
        case_idx: usize,
    ) -> Result<PreparedMatchCase, String> {
        prepared_cases
            .get_mut(case_idx)
            .and_then(Option::take)
            .ok_or_else(|| {
                format!(
                    "Internal error: match case {} was already generated",
                    case_idx
                )
            })
    }
}
