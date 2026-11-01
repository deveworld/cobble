use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_bossbar_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if args.len() != 2 {
                    return Err("bossbar.add() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let name = self.expr_to_helper_text_component(&args[1], "bossbar name")?;
                format!("bossbar add {} {}", self.qualify_namespaced_id(&id), name)
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("bossbar.remove() takes 1 argument".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                format!("bossbar remove {}", self.qualify_namespaced_id(&id))
            }
            "set_value" | "set_max" => {
                if args.len() != 2 {
                    return Err(format!("bossbar.{}() takes 2 arguments", method).into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let value = self.expr_to_int_arg(&args[1], "bossbar value")?;
                let property = method.strip_prefix("set_").unwrap();
                format!(
                    "bossbar set {} {} {}",
                    self.qualify_namespaced_id(&id),
                    property,
                    value
                )
            }
            "set_name" => {
                if args.len() != 2 {
                    return Err("bossbar.set_name() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let name = self.expr_to_helper_text_component(&args[1], "bossbar name")?;
                format!(
                    "bossbar set {} name {}",
                    self.qualify_namespaced_id(&id),
                    name
                )
            }
            "set_color" | "set_style" => {
                if args.len() != 2 {
                    return Err(format!("bossbar.{}() takes 2 arguments", method).into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let value = self.expr_to_plain_arg(&args[1], "bossbar property value")?;
                let property = method.strip_prefix("set_").unwrap();
                match method {
                    "set_color"
                        if !super::is_macro_arg(&value)
                            && !Self::is_valid_bossbar_color(&value) =>
                    {
                        return Err(format!("Invalid bossbar color: {}", value).into());
                    }
                    "set_style"
                        if !super::is_macro_arg(&value)
                            && !Self::is_valid_bossbar_style(&value) =>
                    {
                        return Err(format!("Invalid bossbar style: {}", value).into());
                    }
                    _ => {}
                }
                format!(
                    "bossbar set {} {} {}",
                    self.qualify_namespaced_id(&id),
                    property,
                    value
                )
            }
            "set_visible" => {
                if args.len() != 2 {
                    return Err("bossbar.set_visible() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let visible = self.expr_to_bool(&args[1], "bossbar visibility")?;
                format!(
                    "bossbar set {} visible {}",
                    self.qualify_namespaced_id(&id),
                    visible
                )
            }
            "set_players" => {
                if args.len() != 2 {
                    return Err("bossbar.set_players() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let players = self.expr_to_selector_arg(&args[1], "bossbar players")?;
                format!(
                    "bossbar set {} players {}",
                    self.qualify_namespaced_id(&id),
                    players
                )
            }
            _ => return Err(format!("Unknown bossbar function: bossbar.{}", method).into()),
        };
        self.push_current_command(command)
    }

    pub(in crate::transpiler) fn is_valid_bossbar_color(value: &str) -> bool {
        matches!(
            value,
            "pink" | "blue" | "red" | "green" | "yellow" | "purple" | "white"
        )
    }

    pub(in crate::transpiler) fn is_valid_bossbar_style(value: &str) -> bool {
        matches!(
            value,
            "progress" | "notched_6" | "notched_10" | "notched_12" | "notched_20"
        )
    }
}
