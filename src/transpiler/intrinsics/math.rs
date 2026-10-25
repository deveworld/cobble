use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_math_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        match method {
            "sqrt" => {
                if args.len() != 1 {
                    return Err("math.sqrt() takes 1 argument".into());
                }
                self.process_math_unary_op(args, "sqrt")
            }
            "abs" => {
                if args.len() != 1 {
                    return Err("math.abs() takes 1 argument".into());
                }
                self.process_math_unary_op(args, "abs")
            }
            "min" => {
                if args.len() != 2 {
                    return Err("math.min() takes 2 arguments".into());
                }
                self.process_math_binary_op(args, "min")
            }
            "max" => {
                if args.len() != 2 {
                    return Err("math.max() takes 2 arguments".into());
                }
                self.process_math_binary_op(args, "max")
            }
            _ => Err(format!("Unknown math function: math.{}", method).into()),
        }
    }

    pub(in crate::transpiler) fn process_math_unary_op(
        &mut self,
        args: &[Expression],
        op: &str,
    ) -> Result<(), TranspileError> {
        let input_var = "#math_input";
        // output_var is implied to be "#math_result" by convention in helpers

        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(input_var.to_string(), "temp".to_string());
        self.symbols
            .scoreboard_variables
            .insert(input_var.to_string());

        // Evaluate argument
        let eval_commands = self.evaluate_expression_to_target(&args[0], input_var)?;

        // Ensure helper exists
        self.ensure_math_helper(op);

        if let Some(ref mut commands) = self.current_function {
            commands.extend(eval_commands);
            commands.push(format!(
                "function {}:_cobble_math_{}",
                self.data_pack.namespace, op
            ));
        }
        Ok(())
    }

    pub(in crate::transpiler) fn process_math_binary_op(
        &mut self,
        args: &[Expression],
        op: &str,
    ) -> Result<(), TranspileError> {
        let input1 = "#math_input";
        let input2 = "#math_input2";

        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(input1.to_string(), "temp".to_string());
        self.symbols
            .variable_objectives
            .insert(input2.to_string(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(input1.to_string());
        self.symbols.scoreboard_variables.insert(input2.to_string());

        // Evaluating an argument can itself run a math helper (a nested
        // math.* call), which stages into the shared #math_input scores and
        // would clobber an argument staged earlier. Stage each argument into
        // a unique scratch and copy into the helper inputs right before the
        // call.
        let arg1_scratch = format!("#math_arg_{}", self.next_temp_id());
        let arg2_scratch = format!("#math_arg_{}", self.next_temp_id());

        let mut cmds = Vec::new();
        cmds.extend(self.evaluate_expression_to_target(&args[0], &arg1_scratch)?);
        cmds.extend(self.evaluate_expression_to_target(&args[1], &arg2_scratch)?);
        cmds.push(format!(
            "scoreboard players operation {} temp = {} temp",
            input1, arg1_scratch
        ));
        cmds.push(format!(
            "scoreboard players operation {} temp = {} temp",
            input2, arg2_scratch
        ));

        self.ensure_math_helper(op);

        if let Some(ref mut commands) = self.current_function {
            commands.extend(cmds);
            commands.push(format!(
                "function {}:_cobble_math_{}",
                self.data_pack.namespace, op
            ));
        }
        Ok(())
    }

    pub(in crate::transpiler) fn ensure_math_helper(&mut self, op: &str) {
        self.data_pack.ensure_math_helper(op);
    }
}
