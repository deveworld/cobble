use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_schedule_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "once" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("schedule.once() takes 2 or 3 arguments".into());
                }
                let function = self.expr_to_function_arg(&args[0], "scheduled function")?;
                let delay = self.expr_to_plain_arg(&args[1], "schedule delay")?;
                let function = self.qualify_namespaced_id(&function);
                if args.len() == 3 {
                    let mode = self.expr_to_plain_arg(&args[2], "schedule mode")?;
                    if !super::is_macro_arg(&mode) && !matches!(mode.as_str(), "append" | "replace")
                    {
                        return Err("schedule.once() mode must be append or replace"
                            .to_string()
                            .into());
                    }
                    format!("schedule function {} {} {}", function, delay, mode)
                } else {
                    format!("schedule function {} {}", function, delay)
                }
            }
            "clear" => {
                if args.len() != 1 {
                    return Err("schedule.clear() takes 1 argument".into());
                }
                let function = self.expr_to_function_arg(&args[0], "scheduled function")?;
                format!("schedule clear {}", self.qualify_namespaced_id(&function))
            }
            _ => return Err(format!("Unknown schedule function: schedule.{}", method).into()),
        };
        self.push_current_command(command)
    }
}
