use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_score_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("score.set() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_int_arg(&args[1], "score value")?;
                format!("scoreboard players set {} temp {}", name, value)
            }
            "add" | "remove" => {
                if args.len() != 2 {
                    return Err(format!("score.{}() takes 2 arguments", method).into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_int_arg(&args[1], "score value")?;
                format!("scoreboard players {} {} temp {}", method, name, value)
            }
            "reset" => {
                if args.len() != 1 {
                    return Err("score.reset() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                format!("scoreboard players reset {} temp", name)
            }
            "copy" => {
                if args.len() != 2 {
                    return Err("score.copy() takes 2 arguments".into());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let src = self.expr_to_plain_arg(&args[1], "source score")?;
                format!("scoreboard players operation {} temp = {} temp", dst, src)
            }
            "operation" => {
                if args.len() != 3 {
                    return Err("score.operation() takes 3 arguments".into());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let op = self.expr_to_plain_arg(&args[1], "score operation")?;
                let src = self.expr_to_plain_arg(&args[2], "source score")?;
                const ALLOWED_OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "<", ">", "><"];
                if !super::is_macro_arg(&op) && !ALLOWED_OPS.contains(&op.as_str()) {
                    return Err(format!("Unsupported scoreboard operation '{}'", op).into());
                }
                format!(
                    "scoreboard players operation {} temp {} {} temp",
                    dst, op, src
                )
            }
            _ => return Err(format!("Unknown score function: score.{}", method).into()),
        };
        self.push_current_command(command)
    }

    pub(in crate::transpiler) fn process_score_objective_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("score.objective.add() takes 2 or 3 arguments"
                        .to_string()
                        .into());
                }
                let name = self.expr_to_plain_arg(&args[0], "objective name")?;
                let criteria = self.expr_to_plain_arg(&args[1], "objective criteria")?;
                if args.len() == 3 {
                    let display =
                        self.expr_to_helper_text_component(&args[2], "objective display name")?;
                    format!(
                        "scoreboard objectives add {} {} {}",
                        name, criteria, display
                    )
                } else {
                    format!("scoreboard objectives add {} {}", name, criteria)
                }
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("score.objective.remove() takes 1 argument"
                        .to_string()
                        .into());
                }
                let name = self.expr_to_plain_arg(&args[0], "objective name")?;
                format!("scoreboard objectives remove {}", name)
            }
            "display" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("score.objective.display() takes 1 or 2 arguments"
                        .to_string()
                        .into());
                }
                let slot = self.expr_to_plain_arg(&args[0], "scoreboard display slot")?;
                if args.len() == 2 {
                    let name = self.expr_to_plain_arg(&args[1], "objective name")?;
                    format!("scoreboard objectives setdisplay {} {}", slot, name)
                } else {
                    format!("scoreboard objectives setdisplay {}", slot)
                }
            }
            _ => {
                return Err(format!(
                    "Unknown score objective function: score.objective.{}",
                    method
                )
                .into())
            }
        };
        self.push_current_command(command)
    }

    pub(in crate::transpiler) fn process_random_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "int" => {
                if args.len() != 3 {
                    return Err("random.int() takes 3 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                let min = self.expr_to_int_arg(&args[1], "random min")?;
                let max = self.expr_to_int_arg(&args[2], "random max")?;
                if super::is_macro_arg(&min) || super::is_macro_arg(&max) {
                    // `random value` refuses a range of one value, which a
                    // parameter can give only when the function runs.
                    self.push_current_command(format!(
                        "scoreboard players set {} temp {}",
                        name, min
                    ))?;
                    format!(
                        "execute unless score {name} temp matches {max} store result score {name} temp run random value {min}..{max}"
                    )
                } else {
                    let (min, max): (i32, i32) =
                        (min.parse().unwrap_or(0), max.parse().unwrap_or(0));
                    Self::random_int_command(&name, min, max)?
                }
            }
            "bool" => {
                if args.len() != 1 {
                    return Err("random.bool() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                format!(
                    "execute store result score {} temp run random value 0..1",
                    name
                )
            }
            _ => return Err(format!("Unknown random function: random.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn random_int_command(name: &str, min: i32, max: i32) -> Result<String, TranspileError> {
        if min > max {
            return Err("random.int() min cannot be greater than max"
                .to_string()
                .into());
        }
        Ok(if min == max {
            format!("scoreboard players set {} temp {}", name, min)
        } else {
            format!(
                "execute store result score {} temp run random value {}..{}",
                name, min, max
            )
        })
    }

    pub(in crate::transpiler) fn process_timer_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("timer.set() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                let ticks = self.expr_to_int_arg(&args[1], "timer ticks")?;
                format!("scoreboard players set {} temp {}", name, ticks)
            }
            "tick" => {
                if args.len() != 1 {
                    return Err("timer.tick() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!(
                    "execute if score {} temp matches 1.. run scoreboard players remove {} temp 1",
                    name, name
                )
            }
            "done" => {
                if args.len() != 1 {
                    return Err("timer.done() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                let flag = format!("{}_done", name);
                self.push_current_command(format!("scoreboard players set {} temp 0", flag))?;
                format!(
                    "execute if score {} temp matches ..0 run scoreboard players set {} temp 1",
                    name, flag
                )
            }
            "reset" => {
                if args.len() != 1 {
                    return Err("timer.reset() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!("scoreboard players reset {} temp", name)
            }
            _ => return Err(format!("Unknown timer function: timer.{}", method).into()),
        };
        self.push_current_command(command)
    }
}
