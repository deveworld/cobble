use super::DataPack;
use std::collections::HashSet;

impl DataPack {
    pub(in crate::transpiler::data_pack) fn is_safe_namespace_path(namespace: &str) -> bool {
        !namespace.is_empty()
            && namespace != "."
            && namespace != ".."
            && namespace.chars().all(|c| {
                c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.'
            })
    }

    fn is_safe_resource_path(path: &str) -> bool {
        !path.is_empty()
            && !path.contains('\\')
            && !path.contains(':')
            && path.split('/').all(|segment| {
                !segment.is_empty()
                    && segment != "."
                    && segment != ".."
                    && segment.chars().all(|c| {
                        c.is_ascii_lowercase()
                            || c.is_ascii_digit()
                            || c == '_'
                            || c == '-'
                            || c == '.'
                    })
            })
    }

    fn invalid_path_error(message: impl Into<String>) -> std::io::Error {
        std::io::Error::new(std::io::ErrorKind::InvalidInput, message.into())
    }

    pub(in crate::transpiler::data_pack) fn validate_namespace_for_write(
        namespace: &str,
        label: &str,
    ) -> std::io::Result<()> {
        if Self::is_safe_namespace_path(namespace) {
            Ok(())
        } else {
            Err(Self::invalid_path_error(format!(
                "Invalid {label} namespace `{namespace}`: use lowercase letters, digits, underscores, hyphens, and dots only"
            )))
        }
    }

    pub(in crate::transpiler::data_pack) fn validate_resource_path_for_write(
        path: &str,
        label: &str,
    ) -> std::io::Result<()> {
        if Self::is_safe_resource_path(path) {
            Ok(())
        } else {
            Err(Self::invalid_path_error(format!(
                "Invalid {label} path `{path}`: use lowercase resource paths with '/', '_', '-', or '.', and no empty, '.', or '..' segments"
            )))
        }
    }

    pub(in crate::transpiler::data_pack) fn validate_write_paths(&self) -> std::io::Result<()> {
        Self::validate_namespace_for_write(&self.namespace, "data pack")?;

        for function_name in self.functions.keys() {
            Self::validate_resource_path_for_write(function_name, "function")?;
        }

        for tag_name in self.tags.keys() {
            let (namespace, path) = tag_name
                .split_once(':')
                .unwrap_or((&self.namespace, tag_name.as_str()));
            Self::validate_namespace_for_write(namespace, "function tag")?;
            Self::validate_resource_path_for_write(path, "function tag")?;
        }

        for (label, resources) in [
            ("advancement", &self.advancements),
            ("loot table", &self.loot_tables),
            ("recipe", &self.recipes),
            ("predicate", &self.predicates),
            ("item modifier", &self.item_modifiers),
        ] {
            for name in resources.keys() {
                Self::validate_resource_path_for_write(name, label)?;
            }
        }

        for key in self.json_resources.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid JSON resource key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "JSON resource")?;
            Self::validate_resource_path_for_write(relative_path, "JSON resource")?;
        }

        for key in self.resource_pack_models.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid resource-pack model key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "resource-pack model")?;
            Self::validate_resource_path_for_write(relative_path, "resource-pack model")?;
        }

        for key in self.resource_pack_langs.keys() {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid resource-pack language key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "resource-pack language")?;
            Self::validate_resource_path_for_write(relative_path, "resource-pack language")?;
        }

        for key in &self.resource_pack_static_assets {
            let Some((namespace, relative_path)) = Self::split_json_resource_key(key) else {
                return Err(Self::invalid_path_error(format!(
                    "Invalid resource-pack static asset key `{key}`"
                )));
            };
            Self::validate_namespace_for_write(namespace, "resource-pack static asset")?;
            Self::validate_resource_path_for_write(relative_path, "resource-pack static asset")?;
        }

        for namespace in self.generated_namespaces() {
            Self::validate_namespace_for_write(&namespace, "generated")?;
        }
        for namespace in self.generated_asset_namespaces() {
            Self::validate_namespace_for_write(&namespace, "generated asset")?;
        }

        Ok(())
    }

    pub(in crate::transpiler::data_pack) fn json_resource_key(
        namespace: &str,
        relative_path: &str,
    ) -> String {
        format!("{}/{}", namespace, relative_path)
    }

    pub(in crate::transpiler::data_pack) fn split_json_resource_key(
        key: &str,
    ) -> Option<(&str, &str)> {
        key.split_once('/')
    }

    pub(in crate::transpiler::data_pack) fn generated_namespaces(&self) -> Vec<String> {
        let mut namespaces = HashSet::new();
        namespaces.insert(self.namespace.clone());
        namespaces.insert("minecraft".to_string());
        for key in self.json_resources.keys() {
            if let Some((namespace, _)) = Self::split_json_resource_key(key) {
                namespaces.insert(namespace.to_string());
            }
        }
        for tag_name in self.tags.keys() {
            if let Some((namespace, _)) = tag_name.split_once(':') {
                namespaces.insert(namespace.to_string());
            }
        }
        let mut namespaces = namespaces.into_iter().collect::<Vec<_>>();
        namespaces.sort();
        namespaces
    }

    pub(in crate::transpiler::data_pack) fn generated_asset_namespaces(&self) -> Vec<String> {
        let mut namespaces = HashSet::new();
        for key in self
            .resource_pack_models
            .keys()
            .chain(self.resource_pack_langs.keys())
        {
            if let Some((namespace, _)) = Self::split_json_resource_key(key) {
                namespaces.insert(namespace.to_string());
            }
        }
        let mut namespaces = namespaces.into_iter().collect::<Vec<_>>();
        namespaces.sort();
        namespaces
    }
}
