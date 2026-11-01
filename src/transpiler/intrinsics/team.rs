use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_team_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("team.add() takes 1 or 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                if args.len() == 2 {
                    let display =
                        self.expr_to_helper_text_component(&args[1], "team display name")?;
                    format!("team add {} {}", name, display)
                } else {
                    format!("team add {}", name)
                }
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("team.remove() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                format!("team remove {}", name)
            }
            "join" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("team.join() takes 1 or 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                if args.len() == 2 {
                    let members = self.expr_to_selector_arg(&args[1], "team members")?;
                    format!("team join {} {}", name, members)
                } else {
                    format!("team join {}", name)
                }
            }
            "leave" => {
                if args.len() != 1 {
                    return Err("team.leave() takes 1 argument".into());
                }
                let members = self.expr_to_selector_arg(&args[0], "team members")?;
                format!("team leave {}", members)
            }
            "modify" => {
                if args.len() != 3 {
                    return Err("team.modify() takes 3 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                let option = self.expr_to_plain_arg(&args[1], "team option")?;
                let value = match option.as_str() {
                    "prefix" | "suffix" | "displayName" => {
                        self.expr_to_helper_text_component(&args[2], "team option value")?
                    }
                    _ => self.expr_to_plain_arg(&args[2], "team option value")?,
                };
                format!("team modify {} {} {}", name, option, value)
            }
            _ => return Err(format!("Unknown team function: team.{}", method).into()),
        };
        self.push_current_command(command)
    }
}
