use crate::ast::Expression;
use crate::transpiler::{resource_format, TranspileError, TranspileErrorKind, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_datapack_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        match method {
            "function_tag" => self.add_datapack_tag_resource("function", args),
            "block_tag" => self.add_datapack_tag_resource("block", args),
            "item_tag" => self.add_datapack_tag_resource("item", args),
            "entity_type_tag" => self.add_datapack_tag_resource("entity_type", args),
            "dialog_tag" => self.add_datapack_tag_resource("dialog", args),
            "predicate" => self.add_datapack_json_resource("predicate", args),
            "advancement" => self.add_datapack_json_resource("advancement", args),
            "loot_table" => self.add_datapack_json_resource("loot_table", args),
            "recipe" => self.add_datapack_json_resource("recipe", args),
            "item_modifier" => self.add_datapack_json_resource("item_modifier", args),
            "dialog" => self.add_datapack_json_resource("dialog", args),
            _ => Err(format!("Unknown datapack function: datapack.{}", method).into()),
        }
    }

    pub(in crate::transpiler) fn add_datapack_tag_resource(
        &mut self,
        tag_type: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if !(2..=3).contains(&args.len()) {
            return Err(format!("datapack.{}_tag() takes 2 or 3 arguments", tag_type).into());
        }

        let (namespace, name) = self.expr_to_resource_id(&args[0], "tag name")?;
        let values = self.expr_to_json_value(&args[1])?;
        if !values.is_array() {
            return Err("Tag values must be an array".into());
        }
        if let Some(values) = values.as_array() {
            for value in values {
                self.validate_datapack_tag_value(value)?;
                let id = match value.get("required") {
                    Some(required) if required == false => None,
                    _ => value.as_str().or_else(|| value.get("id")?.as_str()),
                };
                if let Some(problem) =
                    id.and_then(|id| crate::validator::registries::tag_entry_problem(tag_type, id))
                {
                    return Err(problem.into());
                }
            }
        }

        let mut json = serde_json::Map::new();
        json.insert("values".to_string(), values);
        if let Some(replace_arg) = args.get(2) {
            json.insert(
                "replace".to_string(),
                serde_json::Value::Bool(self.expr_to_bool(replace_arg, "tag replace")?),
            );
        }
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode tag JSON: {}", e))?;
        let namespace = namespace.unwrap_or_else(|| self.data_pack.namespace.clone());
        if tag_type == "function" && namespace == "minecraft" && (name == "load" || name == "tick")
        {
            let event = if name == "load" {
                crate::stdlib::EventType::Load
            } else {
                crate::stdlib::EventType::Tick
            };
            let listeners = self
                .data_pack
                .stdlib
                .get_event_handlers(&event)
                .iter()
                .filter(|handler| handler.as_str() != crate::transpiler::INIT_FUNCTION)
                .count();
            let values = json["values"].as_array().cloned().unwrap_or_default();
            self.data_pack
                .event_tag_declarations
                .entry(format!("minecraft:{name}"))
                .or_default()
                .push((listeners, values));
        }
        self.data_pack
            .add_json_resource_in_namespace_with_source(
                namespace,
                format!("tags/{}/{}", tag_type, name),
                content,
                self.current_statement_source.clone(),
            )
            .map_err(TranspileError::from)
    }

    pub(in crate::transpiler) fn validate_datapack_tag_value(
        &self,
        value: &serde_json::Value,
    ) -> Result<(), TranspileError> {
        // Tag values may reference another tag of the same type (`#minecraft:logs`).
        if let Some(id) = value.as_str() {
            self.plain_resource_id_to_parts(id.strip_prefix('#').unwrap_or(id), "tag value")?;
            return Ok(());
        }

        let Some(object) = value.as_object() else {
            return Err(
                "Tag values must be string resource IDs or objects with string \"id\" fields"
                    .to_string()
                    .into(),
            );
        };

        for key in object.keys() {
            if key != "id" && key != "required" {
                return Err(format!(
                    "Tag object values may only contain \"id\" and \"required\" fields; found \"{}\"",
                    key
                ).into());
            }
        }

        let Some(id) = object.get("id").and_then(|value| value.as_str()) else {
            return Err("Tag object values must include a string \"id\" resource ID"
                .to_string()
                .into());
        };
        self.plain_resource_id_to_parts(id.strip_prefix('#').unwrap_or(id), "tag value")?;

        if object
            .get("required")
            .is_some_and(|value| !value.is_boolean())
        {
            return Err("Tag object value \"required\" must be a boolean"
                .to_string()
                .into());
        }

        Ok(())
    }

    pub(in crate::transpiler) fn add_datapack_json_resource(
        &mut self,
        resource_type: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err(format!("datapack.{}() takes 2 arguments", resource_type).into());
        }

        let (namespace, name) = self.expr_to_resource_id(&args[0], "resource name")?;
        let json = self.expr_to_json_value(&args[1])?;
        if !json.is_object() && !(resource_type == "item_modifier" && json.is_array()) {
            return Err(
                format!("datapack.{}() JSON value must be an object", resource_type).into(),
            );
        }
        let resource_namespace = namespace
            .clone()
            .unwrap_or_else(|| self.data_pack.namespace.clone());
        self.check_datapack_resource_format(
            resource_type,
            &format!("{resource_namespace}:{name}"),
            &json,
        )?;
        // A file can't hold a list, so a list becomes the sequence it means.
        let mut json = match json {
            serde_json::Value::Array(functions) => {
                serde_json::json!({"type": "minecraft:sequence", "functions": functions})
            }
            json => json,
        };
        // `temp` names the pack's objective here too, as in commands.
        crate::transpiler::vars_objective::map_json(&mut json, &self.data_pack.vars_objective());
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode resource JSON: {}", e))?;
        let namespace = namespace.unwrap_or_else(|| self.data_pack.namespace.clone());
        self.data_pack
            .add_json_resource_in_namespace_with_source(
                namespace,
                format!("{}/{}", resource_type, name),
                content,
                self.current_statement_source.clone(),
            )
            .map_err(TranspileError::from)
    }

    /// Minecraft rejects data pack resources written in pre-26.3 shapes or
    /// without the fields they need, and a server whose packs fail to load
    /// refuses to start, so these are reported at compile time. Fields
    /// Minecraft ignores are warnings. The resources the declaration `id`
    /// names are checked once every file is compiled.
    pub(in crate::transpiler) fn check_datapack_resource_format(
        &mut self,
        resource_type: &str,
        id: &str,
        json: &serde_json::Value,
    ) -> Result<(), TranspileError> {
        let mut problems = resource_format::Problems::default();
        match resource_type {
            "predicate" => resource_format::check_predicate(json, "", &mut problems),
            "item_modifier" => match json.as_array() {
                Some(items) => items.iter().enumerate().for_each(|(index, item)| {
                    let path = format!("[{index}]");
                    if !item.is_object() {
                        problems.push(format!("`{path}` must be an object; rejected"));
                    }
                    resource_format::check_item_modifier(item, &path, &mut problems);
                }),
                None => resource_format::check_item_modifier(json, "", &mut problems),
            },
            "loot_table" => resource_format::check_loot_table(json, &mut problems),
            "advancement" => resource_format::check_advancement(json, &mut problems),
            "recipe" => resource_format::check_recipe(json, &mut problems),
            "dialog" => resource_format::check_dialog(json, &mut problems),
            _ => {}
        }
        let resource_format::Problems {
            errors,
            warnings,
            references,
        } = problems;
        for warning in warnings {
            self.warn(
                "datapack-resource-format",
                format!("datapack.{resource_type}(): {warning}"),
                None,
            );
        }
        let declaration = format!("{} `{id}`", resource_type.replace('_', " "));
        for reference in references {
            self.resource_references.push(
                crate::transpiler::resource_references::DeclaredReference {
                    declaration: declaration.clone(),
                    source: self.current_statement_source.clone(),
                    reference,
                },
            );
        }
        if errors.is_empty() {
            return Ok(());
        }
        let mut message = format!(
            "datapack.{}() uses fields Minecraft {} rejects or ignores (a server refuses to start when a pack fails to load):\n{}",
            resource_type,
            crate::pack_format::SUPPORTED_MINECRAFT_VERSION,
            errors
                .iter()
                .map(|problem| format!("  - {}", problem))
                .collect::<Vec<_>>()
                .join("\n")
        );
        if let Some(source) = &self.current_statement_source {
            message.push_str(&format!("\n  location: {}:{}", source.line, source.column));
        }
        Err(TranspileError::new(
            TranspileErrorKind::DatapackResourceFormat,
            message,
        ))
    }

    pub(in crate::transpiler) fn expr_to_resource_id(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<(Option<String>, String), TranspileError> {
        // A plain name is a variable, whose value isn't known when the pack
        // is built; the file would be named after the variable instead. A
        // bare `namespace:path` is an id, not a variable.
        if let Some(name) = match expr {
            Expression::Identifier(name) if !name.contains(':') => Some(name),
            _ => None,
        } {
            return Err(format!(
                "Identifier '{name}' cannot be used as the {label} here; the {label} must be known at compile time, so write it as a string literal"
            )
            .into());
        }
        let id = self.expr_to_plain_arg(expr, label)?;
        self.plain_resource_id_to_parts(&id, label)
    }

    pub(in crate::transpiler) fn plain_resource_id_to_parts(
        &self,
        id: &str,
        label: &str,
    ) -> Result<(Option<String>, String), TranspileError> {
        let (namespace, path) = if let Some((namespace, path)) = id.split_once(':') {
            if path.contains(':') {
                return Err(format!(
                    "Invalid {} '{}': resource IDs may contain at most one ':' separator",
                    label, id
                )
                .into());
            }
            (Some(namespace.to_string()), path.to_string())
        } else {
            (None, id.to_string())
        };

        if let Some(namespace) = &namespace {
            let invalid_namespace = namespace.is_empty()
                || namespace.chars().any(|c| {
                    !(c.is_ascii_lowercase()
                        || c.is_ascii_digit()
                        || c == '_'
                        || c == '-'
                        || c == '.')
                });
            if invalid_namespace {
                return Err(format!(
                    "Invalid {} '{}': use lowercase namespace:path resource IDs",
                    label, id
                )
                .into());
            }
        }

        if namespace.is_none() {
            if let Some((prefix, suffix)) = path.split_once('/') {
                if matches!(prefix, "minecraft") {
                    return Err(format!(
                        "Invalid {} '{}': '{}' looks like a namespace. Use '{}:{}' instead of a slash-separated namespace prefix.",
                        label, id, prefix, prefix, suffix
                    ).into());
                }
            }
        }

        if let Some((index, c)) = path.char_indices().find(|(_, c)| c.is_ascii_uppercase()) {
            return Err(format!(
                "Invalid {} '{}': uppercase character '{}' at position {}. Use lowercase resource paths or namespace:path IDs.",
                label,
                id,
                c,
                index + 1
            ).into());
        }

        if path.contains('\\') {
            return Err(format!(
                "Invalid {} '{}': use '/' path separators, not '\\'",
                label, id
            )
            .into());
        }

        let has_invalid_segment = path
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..");
        let has_invalid_char = path.chars().any(|c| {
            !(c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '_'
                || c == '-'
                || c == '.'
                || c == '/')
        });
        if path.is_empty() || path.contains('\\') || has_invalid_segment || has_invalid_char {
            return Err(format!(
                "Invalid {} '{}': use lowercase resource paths or namespace:path IDs with letters, digits, '/', '_', '-', or '.', and no empty, '.', or '..' segments",
                label, id
            ).into());
        }
        Ok((namespace, path))
    }
}
