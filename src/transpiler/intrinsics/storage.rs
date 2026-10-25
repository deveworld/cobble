use crate::ast::Expression;
use crate::transpiler::storage_paths::StorageValueOps;
use crate::transpiler::{GeneratedCommandKind, StoragePathPlan, TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn expr_to_storage_path_plan(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<StoragePathPlan, TranspileError> {
        match expr {
            Expression::String(s) | Expression::Identifier(s) => {
                Ok(StoragePathPlan::static_path(s.clone()))
            }
            Expression::Number(n) if n.fract() == 0.0 => {
                Ok(StoragePathPlan::static_path((*n as i32).to_string()))
            }
            Expression::Number(n) => Ok(StoragePathPlan::static_path(n.to_string())),
            Expression::Call(func, args) => {
                if let Some(value) = self.storage_path_helper_plan(func, args)? {
                    Ok(value)
                } else {
                    Err(format!("{} must be a storage path", label).into())
                }
            }
            _ => Err(format!("{} must be a storage path", label).into()),
        }
    }

    pub(in crate::transpiler) fn storage_path_helper_string(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<String>, TranspileError> {
        let Some(plan) = self.storage_path_helper_plan(func, args)? else {
            return Ok(None);
        };
        match plan {
            StoragePathPlan::Static(path) => Ok(Some(path)),
            StoragePathPlan::Dynamic { .. } => Err(
                "Dynamic storage paths can only be used by storage helpers that emit Minecraft commands"
                    .into(),
            ),
        }
    }

    pub(in crate::transpiler) fn storage_path_helper_plan(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<StoragePathPlan>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "storage" {
            return Ok(None);
        }

        self.require_stdlib_module("storage")?;
        let path = match method.as_str() {
            "path" => {
                if args.is_empty() {
                    return Err("storage.path() takes at least 1 argument"
                        .to_string()
                        .into());
                }
                StoragePathPlan::static_path(
                    args.iter()
                        .map(|arg| {
                            self.expr_to_storage_path_component(arg, "storage path component")
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("."),
                )
            }
            "child" => {
                if args.len() != 2 {
                    return Err("storage.child() takes 2 arguments".into());
                }
                let base = self.expr_to_storage_path_plan(&args[0], "storage base path")?;
                let child = self.expr_to_storage_path_component(&args[1], "storage child path")?;
                base.append_field(&child)
            }
            "index" => {
                if args.len() != 2 {
                    return Err("storage.index() takes 2 arguments".into());
                }
                let base = self.expr_to_storage_path_plan(&args[0], "storage base path")?;
                if let Ok(index) = self.expr_to_i32(&args[1], "storage list index") {
                    base.append_static_index(index)
                } else {
                    base.append_dynamic_index(args[1].clone())
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(path))
    }

    pub(in crate::transpiler) fn expr_to_storage_path_component(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        let component = self.expr_to_plain_arg(expr, label)?;
        if component.is_empty() {
            return Err(format!("{} cannot be empty", label).into());
        }
        if component
            .chars()
            .any(|ch| ch == '.' || ch == '[' || ch == ']')
        {
            return Err(format!("{} must be a single unqualified path component", label).into());
        }
        Ok(component)
    }

    pub(in crate::transpiler) fn process_storage_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let commands = match method {
            "set" => self.storage_set(args)?,
            "merge" | "append" | "prepend" => {
                check_storage_arg_count(method, args, 2)?;
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                self.modify_storage_with_value(&path, method, &args[1])?
            }
            "insert" => {
                check_storage_arg_count(method, args, 3)?;
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                let index = self.expr_to_i32(&args[1], "storage insert index")?;
                self.modify_storage_with_value(&path, &format!("insert {}", index), &args[2])?
            }
            "remove" | "get" => {
                check_storage_arg_count(method, args, 1)?;
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                let namespace = self.data_pack.namespace.clone();
                self.command_at_storage_path(&path, |path| {
                    format!("data {} storage {}:global {}", method, namespace, path)
                })?
            }
            "copy" => self.storage_copy(args)?,
            "read_score" => self.storage_read_score(args)?,
            "copy_from" => self.storage_copy_from(args)?,
            "path" | "child" | "index" => {
                let func = Expression::Attribute(
                    Box::new(Expression::Identifier("storage".to_string())),
                    method.to_string(),
                );
                if self.storage_path_helper_plan(&func, args)?.is_some() {
                    return Err(format!(
                        "storage.{}() returns a storage path string; pass it to a helper that accepts a storage path",
                        method
                    ).into());
                }
                return Err(format!("Unknown storage function: storage.{}", method).into());
            }
            _ => return Err(format!("Unknown storage function: storage.{}", method).into()),
        };

        self.push_current_commands(commands)
    }

    /// A command on the storage path `path`, with `command(path_text)`
    /// giving its text. A path computed at runtime runs it as a macro.
    fn command_at_storage_path(
        &mut self,
        path: &StoragePathPlan,
        command: impl Fn(&str) -> String,
    ) -> Result<Vec<String>, TranspileError> {
        match path.as_static() {
            Some(static_path) => Ok(vec![command(static_path)]),
            None => self.dynamic_storage_command(path, GeneratedCommandKind::StdLib, |template| {
                vec![format!("${}", command(template))]
            }),
        }
    }

    /// `data modify storage <path> <operation> ...` with `value`: inline as
    /// `value <snbt>` when it is a literal, otherwise copied `from storage` a
    /// scratch path that is removed afterwards.
    fn modify_storage_with_value(
        &mut self,
        path: &StoragePathPlan,
        operation: &str,
        value: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        if let Ok(snbt) = self.serialize_to_snbt(value) {
            return self.command_at_storage_path(path, |path| {
                format!(
                    "data modify storage {}:global {} {} value {}",
                    namespace, path, operation, snbt
                )
            });
        }
        let (mut commands, value_path) = self.materialize_storage_value(value)?;
        commands.extend(self.command_at_storage_path(path, |path| {
            format!(
                "data modify storage {}:global {} {} from storage {}:global {}",
                namespace, path, operation, namespace, value_path
            )
        })?);
        commands.push(format!(
            "data remove storage {}:global {}",
            namespace, value_path
        ));
        Ok(commands)
    }

    fn storage_set(&mut self, args: &[Expression]) -> Result<Vec<String>, TranspileError> {
        check_storage_arg_count("set", args, 2)?;
        let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
        match path.as_static() {
            Some(static_path) => self.storage_value_commands(static_path, &args[1]),
            None => self.modify_storage_with_value(&path, "set", &args[1]),
        }
    }

    fn storage_copy(&mut self, args: &[Expression]) -> Result<Vec<String>, TranspileError> {
        check_storage_arg_count("copy", args, 2)?;
        let namespace = self.data_pack.namespace.clone();
        let dst = self.expr_to_storage_path_plan(&args[0], "destination storage path")?;
        let src = self.expr_to_storage_path_plan(&args[1], "source storage path")?;

        // A runtime source path is copied to a fixed scratch path first, so
        // the destination command needs at most one macro path.
        let mut commands = Vec::new();
        let scratch_source = src.as_static().is_none();
        let source_path = match src.as_static() {
            Some(static_src) => static_src.to_string(),
            None => {
                let temp_path = format!("_cobble_copy_source_{}", self.next_temp_id());
                let ns = namespace.clone();
                commands.extend(self.dynamic_storage_command(
                    &src,
                    GeneratedCommandKind::StdLib,
                    |template| {
                        vec![
                            format!("data remove storage {}:global {}", ns, temp_path),
                            format!(
                                "$data modify storage {}:global {} set from storage {}:global {}",
                                ns, temp_path, ns, template
                            ),
                        ]
                    },
                )?);
                temp_path
            }
        };

        commands.extend(self.command_at_storage_path(&dst, |path| {
            format!(
                "data modify storage {}:global {} set from storage {}:global {}",
                namespace, path, namespace, source_path
            )
        })?);
        if scratch_source {
            commands.push(format!(
                "data remove storage {}:global {}",
                namespace, source_path
            ));
        }
        Ok(commands)
    }

    fn storage_read_score(&mut self, args: &[Expression]) -> Result<Vec<String>, TranspileError> {
        if !(2..=3).contains(&args.len()) {
            return Err("storage.read_score() takes 2 or 3 arguments"
                .to_string()
                .into());
        }
        self.data_pack.track_objective("temp");
        let namespace = self.data_pack.namespace.clone();
        let score = self.expr_to_plain_arg(&args[0], "target score")?;
        let path = self.expr_to_storage_path_plan(&args[1], "storage path")?;
        // No default scale: `data get` with a scale accepts only
        // numbers, while the bare form also yields a list's or
        // compound's size and a string's length (numbers floor the
        // same either way).
        let scale = if args.len() == 3 {
            format!(" {}", self.expr_to_plain_arg(&args[2], "data get scale")?)
        } else {
            String::new()
        };
        match path.as_static() {
            Some(static_path) => Ok(vec![format!(
                "execute store result score {} temp run data get storage {}:global {}{}",
                score, namespace, static_path, scale
            )]),
            None => self.dynamic_storage_command(&path, GeneratedCommandKind::StdLib, |template| {
                vec![
                    format!("scoreboard players set {} temp 0", score),
                    format!(
                        "$execute store result score {} temp run data get storage {}:global {}{}",
                        score, namespace, template, scale
                    ),
                ]
            }),
        }
    }

    fn storage_copy_from(&mut self, args: &[Expression]) -> Result<Vec<String>, TranspileError> {
        check_storage_arg_count("copy_from", args, 4)?;
        let namespace = self.data_pack.namespace.clone();
        let dst = self.expr_to_storage_path_plan(&args[0], "destination storage path")?;
        let source_type = self.expr_to_plain_arg(&args[1], "source type")?;
        let source_id = if source_type == "entity" {
            self.expr_to_selector_arg(&args[2], "source id")?
        } else {
            self.expr_to_plain_arg(&args[2], "source id")?
        };
        let source_path = self.expr_to_plain_arg(&args[3], "source path")?;
        if !matches!(source_type.as_str(), "entity" | "block" | "storage") {
            return Err(
                "storage.copy_from() source type must be entity, block, or storage"
                    .to_string()
                    .into(),
            );
        }
        self.command_at_storage_path(&dst, |path| {
            format!(
                "data modify storage {}:global {} set from {} {} {}",
                namespace, path, source_type, source_id, source_path
            )
        })
    }
}

fn check_storage_arg_count(
    method: &str,
    args: &[Expression],
    count: usize,
) -> Result<(), TranspileError> {
    if args.len() == count {
        return Ok(());
    }
    let noun = if count == 1 { "argument" } else { "arguments" };
    Err(format!("storage.{}() takes {} {}", method, count, noun).into())
}
