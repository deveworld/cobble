use crate::ast::Expression;
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_entity_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "tag_add" | "tag_remove" => {
                if args.len() != 2 {
                    return Err(format!("entity.{}() takes 2 arguments", method).into());
                }
                let target = self.expr_to_selector_arg(&args[0], "entity target")?;
                let tag = self.expr_to_plain_arg(&args[1], "entity tag")?;
                let action = method.strip_prefix("tag_").unwrap();
                format!("tag {} {} {}", target, action, tag)
            }
            "effect_give" => {
                if !(2..=5).contains(&args.len()) {
                    return Err("entity.effect_give() takes 2 to 5 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "effect target")?;
                let effect = self.expr_to_plain_arg(&args[1], "effect id")?;
                let mut command = format!("effect give {} {}", target, effect);
                if args.len() >= 3 {
                    command.push(' ');
                    command.push_str(&self.expr_to_plain_arg(&args[2], "effect duration")?);
                }
                if args.len() >= 4 {
                    command.push(' ');
                    command.push_str(&self.expr_to_int_arg(&args[3], "effect amplifier")?);
                }
                if args.len() == 5 {
                    command.push(' ');
                    command.push_str(&self.expr_to_bool_arg(&args[4], "effect hide particles")?);
                }
                command
            }
            "effect_clear" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("entity.effect_clear() takes 1 or 2 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "effect target")?;
                if args.len() == 2 {
                    let effect = self.expr_to_plain_arg(&args[1], "effect id")?;
                    format!("effect clear {} {}", target, effect)
                } else {
                    format!("effect clear {}", target)
                }
            }
            "attribute_get" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("entity.attribute_get() takes 2 or 3 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "attribute target")?;
                let attribute = self.expr_to_plain_arg(&args[1], "attribute id")?;
                if args.len() == 3 {
                    let scale = self.expr_to_plain_arg(&args[2], "attribute scale")?;
                    format!("attribute {} {} get {}", target, attribute, scale)
                } else {
                    format!("attribute {} {} get", target, attribute)
                }
            }
            "attribute_base_set" => {
                if args.len() != 3 {
                    return Err("entity.attribute_base_set() takes 3 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "attribute target")?;
                let attribute = self.expr_to_plain_arg(&args[1], "attribute id")?;
                let value = self.expr_to_plain_arg(&args[2], "attribute value")?;
                format!("attribute {} {} base set {}", target, attribute, value)
            }
            "teleport" => {
                if args.len() != 2 {
                    return Err("entity.teleport() takes 2 arguments".into());
                }
                let target = self.expr_to_selector_arg(&args[0], "teleport target")?;
                let destination = self.expr_to_plain_arg(&args[1], "teleport destination")?;
                format!("tp {} {}", target, destination)
            }
            _ => return Err(format!("Unknown entity function: entity.{}", method).into()),
        };
        self.push_current_command(command)
    }
}
