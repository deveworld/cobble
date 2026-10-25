use crate::ast::Expression;
use crate::transpiler::{TranspileError, TranspileErrorKind, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_resource_pack_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if !self.experimental_resource_pack {
            return Err(TranspileError::new(
                TranspileErrorKind::ResourcePackNotEnabled,
                "resource_pack.* requires --experimental-resource-pack or [experimental] resource_pack = true",
            ));
        }

        match method {
            "item_model" => self.add_resource_pack_model("item", args),
            "block_model" => self.add_resource_pack_model("block", args),
            "lang" => self.add_resource_pack_lang(args),
            _ => Err(format!("Unknown resource_pack function: resource_pack.{}", method).into()),
        }
    }

    pub(in crate::transpiler) fn add_resource_pack_model(
        &mut self,
        model_type: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err(format!("resource_pack.{}_model() takes 2 arguments", model_type).into());
        }

        let (namespace, name) = self
            .expr_to_resource_id(&args[0], "model name")
            .map_err(|error| error.classify_semantic(TranspileErrorKind::ResourcePackBadId))?;
        let json = self.expr_to_json_value(&args[1])?;
        if !json.is_object() {
            return Err(TranspileError::new(
                TranspileErrorKind::ResourcePackNonObject,
                format!(
                    "resource_pack.{}_model() JSON value must be an object",
                    model_type
                ),
            ));
        }
        Self::validate_resource_pack_model_json(model_type, &json)?;
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode resource-pack model JSON: {}", e))?;
        let namespace = namespace.unwrap_or_else(|| self.data_pack.namespace.clone());
        self.data_pack
            .add_resource_pack_model_in_namespace_with_source(
                namespace.clone(),
                format!("models/{}/{}", model_type, name),
                content,
                self.current_statement_source.clone(),
            )
            .map_err(|message| {
                TranspileError::new(TranspileErrorKind::ResourcePackDuplicate, message)
            })?;
        // Vanilla items already have definitions, some with states and tints
        // (a bow's pulling, a potion's color) that a plain one would drop, so
        // overriding a `minecraft:` model keeps them.
        if model_type != "item" || namespace == "minecraft" {
            return Ok(());
        }

        // Since Minecraft 1.21.4 an item finds its model through an item model
        // definition in `items/`, so write the plain one that points at the
        // new model. A static `assets/<namespace>/items/<name>.json` replaces
        // it (see `DataPack::remove_item_definitions_replaced_by`).
        let definition = serde_json::json!({
            "model": {
                "type": "minecraft:model",
                "model": format!("{}:item/{}", namespace, name),
            }
        });
        let definition = serde_json::to_string_pretty(&definition)
            .map_err(|e| format!("Failed to encode item model definition JSON: {}", e))?;
        self.data_pack
            .add_resource_pack_model_in_namespace_with_source(
                namespace,
                format!("items/{}", name),
                definition,
                self.current_statement_source.clone(),
            )
            .map_err(|message| {
                TranspileError::new(TranspileErrorKind::ResourcePackDuplicate, message)
            })
    }

    pub(in crate::transpiler) fn validate_resource_pack_model_json(
        model_type: &str,
        json: &serde_json::Value,
    ) -> Result<(), TranspileError> {
        let object = json.as_object().ok_or_else(|| {
            TranspileError::new(
                TranspileErrorKind::ResourcePackNonObject,
                format!(
                    "resource_pack.{}_model() JSON value must be an object",
                    model_type
                ),
            )
        })?;
        Self::require_optional_json_string(
            object,
            "parent",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        Self::require_optional_json_string(
            object,
            "gui_light",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        Self::require_optional_json_string(
            object,
            "credit",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        Self::require_optional_json_object(
            object,
            "display",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        Self::require_optional_json_array(
            object,
            "elements",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        Self::require_optional_json_array(
            object,
            "overrides",
            &format!("resource_pack.{}_model()", model_type),
        )?;
        if let Some(textures) = object.get("textures") {
            let Some(textures) = textures.as_object() else {
                return Err(format!(
                    "resource_pack.{}_model() field 'textures' must be an object",
                    model_type
                )
                .into());
            };
            for (texture_key, texture_value) in textures {
                if !texture_value.is_string() {
                    return Err(format!(
                        "resource_pack.{}_model() texture '{}' must be a string",
                        model_type, texture_key
                    )
                    .into());
                }
            }
        }
        Ok(())
    }

    pub(in crate::transpiler) fn require_optional_json_string(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("{context} field '{field}' must be a string").into());
        }
        Ok(())
    }

    pub(in crate::transpiler) fn require_optional_json_object(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_object()) {
            return Err(format!("{context} field '{field}' must be an object").into());
        }
        Ok(())
    }

    pub(in crate::transpiler) fn require_optional_json_array(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_array()) {
            return Err(format!("{context} field '{field}' must be an array").into());
        }
        Ok(())
    }

    pub(in crate::transpiler) fn add_resource_pack_lang(
        &mut self,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err("resource_pack.lang() takes 2 arguments".into());
        }

        let (namespace, locale) = self
            .expr_to_resource_id(&args[0], "language locale")
            .map_err(|error| error.classify_semantic(TranspileErrorKind::ResourcePackBadId))?;
        if locale.contains('/') {
            return Err(TranspileError::new(
                TranspileErrorKind::ResourcePackBadId,
                format!(
                    "Invalid language locale '{}': use locale names such as 'en_us'",
                    locale
                ),
            ));
        }

        let json = self.expr_to_json_value(&args[1])?;
        let Some(object) = json.as_object() else {
            return Err(TranspileError::new(
                TranspileErrorKind::ResourcePackNonObject,
                "resource_pack.lang() JSON value must be an object",
            ));
        };
        for (key, value) in object {
            if !value.is_string() {
                return Err(
                    format!("resource_pack.lang() value for '{}' must be a string", key).into(),
                );
            }
        }

        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode resource-pack lang JSON: {}", e))?;
        let namespace = namespace.unwrap_or_else(|| self.data_pack.namespace.clone());
        self.data_pack
            .add_resource_pack_lang_in_namespace_with_source(
                namespace,
                locale,
                content,
                self.current_statement_source.clone(),
            )
            .map_err(|message| {
                TranspileError::new(TranspileErrorKind::ResourcePackDuplicate, message)
            })
    }
}
