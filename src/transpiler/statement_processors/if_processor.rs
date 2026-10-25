use crate::ast::*;
use crate::transpiler::expression_evaluator::ExpressionEvaluator;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_if(
        &mut self,
        if_stmt: &IfStatement,
    ) -> Result<(), TranspileError> {
        let condition_args = self.lower_condition_to_execute_args(&if_stmt.condition)?;

        // With `elif` or `else`, a flag records that a branch ran, so the
        // later branches check the flag instead of their predecessors'
        // conditions.
        let branch_var = if !if_stmt.elif_branches.is_empty() || if_stmt.else_branch.is_some() {
            let var = format!("if_branch_{}", self.next_temp_id());
            self.data_pack.track_objective("temp");
            if let Some(ref mut commands) = self.current_function {
                commands.push(format!("scoreboard players set {} temp 0", var));
                commands.push(format!(
                    "execute {} run scoreboard players set {} temp 1",
                    condition_args, var
                ));
            }
            Some(var)
        } else {
            None
        };
        self.emit_branch_body(&if_stmt.then_branch, &condition_args, "if_temp")?;

        let Some(branch_var) = branch_var else {
            return Ok(());
        };
        for (elif_condition, elif_branch) in &if_stmt.elif_branches {
            let taken = self.lower_elif_condition(elif_condition, &branch_var)?;
            self.emit_branch_body(
                elif_branch,
                &format!("if score {} temp matches 1", taken),
                "elif_temp",
            )?;
        }
        if let Some(else_branch) = &if_stmt.else_branch {
            self.emit_branch_body(
                else_branch,
                &format!("unless score {} temp matches 1", branch_var),
                "else_temp",
            )?;
        }
        Ok(())
    }

    /// Lowers an `elif` condition to a fresh flag that is 1 when this branch
    /// runs, and marks `branch_var` when it does. The condition is evaluated
    /// only when no earlier branch ran: a condition that needs setup commands
    /// (a function call, say) runs them in a helper behind the branch flag.
    /// Returns the flag's name.
    fn lower_elif_condition(
        &mut self,
        condition: &Expression,
        branch_var: &str,
    ) -> Result<String, TranspileError> {
        let not_taken = format!("unless score {} temp matches 1", branch_var);
        let (execute_args, setup) = if ExpressionEvaluator::calls_user_code(condition) {
            let (args, setup) = self.capture_commands_with_result(|transpiler| {
                transpiler.lower_condition_to_execute_args(condition)
            })?;
            (args, Some(setup))
        } else {
            (self.lower_condition_to_execute_args(condition)?, None)
        };
        let taken = format!("elif_taken_{}", self.next_temp_id());
        let set_taken = format!("scoreboard players set {} temp 1", taken);
        let mut commands = vec![format!("scoreboard players set {} temp 0", taken)];
        match setup {
            Some(mut helper) => {
                helper
                    .commands
                    .push(format!("execute {} run {}", execute_args, set_taken));
                let helper_name = format!("elif_cond_{}", self.next_temp_id());
                let needs_storage = helper.requires_macro_context();
                self.add_captured_function(helper_name.clone(), helper);
                let call = self.function_call_command(&helper_name, needs_storage);
                commands.push(format!("execute {} run {}", not_taken, call));
            }
            None => commands.push(format!(
                "execute {} {} run {}",
                not_taken, execute_args, set_taken
            )),
        }
        self.data_pack.track_objective("temp");
        commands.push(format!(
            "execute if score {} temp matches 1 run scoreboard players set {} temp 1",
            taken, branch_var
        ));
        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }
        Ok(taken)
    }

    /// Emits a branch body under `condition_args`. The body is captured
    /// first; the number of commands it produced decides between inlining
    /// and routing through a helper function. Inlining is only safe for a
    /// single command: prefixing several commands with the condition
    /// re-evaluates it per command, so an earlier command that changes the
    /// condition variables would cut off the rest of the branch.
    fn emit_branch_body(
        &mut self,
        statements: &[Statement],
        condition_args: &str,
        helper_prefix: &str,
    ) -> Result<(), TranspileError> {
        let saved_context = self.current_context.clone();
        let capture = self.capture_statements(statements)?;
        self.current_context = saved_context;

        if capture.commands.len() <= 1 {
            return self.append_transformed_capture(capture, |cmd| {
                Self::guard_command(condition_args, cmd)
            });
        }
        let helper_name = format!("{}_{}", helper_prefix, self.next_temp_id());
        let needs_storage = capture.requires_macro_context();
        self.add_captured_function(helper_name.clone(), capture);
        let call = self.function_call_command(&helper_name, needs_storage);
        let return_cmd = self.return_propagation_command();
        if let Some(ref mut commands) = self.current_function {
            commands.push(format!("execute {} run {}", condition_args, call));
            commands.extend(return_cmd);
        }
        Ok(())
    }

    /// Run one generated command under `execute <condition_args>`, merging
    /// an inner `execute` chain. A macro line keeps its `$` at the very front
    /// (`$execute if ... run tellraw $(p) ...`); `execute ... run $tellraw`
    /// would be an unknown command.
    pub(in crate::transpiler) fn guard_command(condition_args: &str, cmd: &str) -> String {
        let (macro_marker, cmd) = match cmd.strip_prefix('$') {
            Some(rest) => ("$", rest),
            None => ("", cmd),
        };
        let clean_cmd = Self::strip_command_prefix(cmd);
        if let Some(inner_parts) = clean_cmd.strip_prefix("execute ") {
            format!("{}execute {} {}", macro_marker, condition_args, inner_parts)
        } else {
            format!(
                "{}execute {} run {}",
                macro_marker, condition_args, clean_cmd
            )
        }
    }

    pub(in crate::transpiler) fn condition_execute_args(condition_cmd: &str) -> String {
        if condition_cmd.starts_with("if ") || condition_cmd.starts_with("unless ") {
            condition_cmd.to_string()
        } else {
            format!("if {}", condition_cmd)
        }
    }
}
