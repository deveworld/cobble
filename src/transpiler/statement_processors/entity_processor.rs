use crate::ast::{EntityDef, Expression};
use crate::transpiler::{TranspileError, Transpiler};

impl Transpiler {
    pub(in crate::transpiler) fn process_create_entity(
        &mut self,
        name: &str,
    ) -> Result<(), TranspileError> {
        if let Some(template) = self.symbols.entity_templates.get(name) {
            // Extract type from selector (e.g., @e[type=zombie])
            // This is a crude extraction, but sufficient for now
            let type_str = if let Some(start) = template.selector.find("type=") {
                let rest = &template.selector[start + 5..];
                let end = rest.find([',', ']']).unwrap_or(rest.len());
                &rest[..end]
            } else {
                "minecraft:armor_stand" // Default if no type specified? Or error?
            };

            let entity_type = if type_str.contains(':') {
                type_str.to_string()
            } else {
                format!("minecraft:{}", type_str)
            };

            // The selector's `tag=` values go into `Tags`, so the alias
            // matches the entities it creates.
            let nbt = Self::with_selector_tags(&template.nbt, &template.selector);
            let nbt_str = self.serialize_to_snbt(&nbt)?;

            // Generate summon command
            if let Some(ref mut commands) = self.current_function {
                commands.push(format!("summon {} ~ ~ ~ {}", entity_type, nbt_str));
            }
        } else {
            return Err(format!(
                "Unknown entity template '{}'. Define it first using 'define {} = ...'",
                name, name
            )
            .into());
        }
        Ok(())
    }

    /// Adds each positive `tag=` argument of `selector` to the `Tags` list of
    /// an entity template payload. Negated (`tag=!x`) and empty (`tag=`)
    /// arguments are skipped, as are tags the payload already lists.
    pub(in crate::transpiler) fn with_selector_tags(
        nbt: &Expression,
        selector: &str,
    ) -> Expression {
        let Some(args) = selector
            .find('[')
            .and_then(|start| selector[start + 1..].strip_suffix(']'))
        else {
            return nbt.clone();
        };
        let tags: Vec<String> = Self::split_selector_args(args)
            .into_iter()
            .filter_map(|arg| {
                let (key, value) = arg.split_once('=')?;
                if key.trim() != "tag" {
                    return None;
                }
                let value = value.trim().trim_matches('"');
                (!value.is_empty() && !value.starts_with('!')).then(|| value.to_string())
            })
            .collect();
        if tags.is_empty() {
            return nbt.clone();
        }

        let mut entries = match nbt {
            Expression::Map(entries) => entries.clone(),
            _ => return nbt.clone(),
        };
        match entries.iter_mut().find(|(key, _)| key == "Tags") {
            Some((_, Expression::Array(existing))) => {
                for tag in tags {
                    let tag = Expression::String(tag);
                    if !existing.contains(&tag) {
                        existing.push(tag);
                    }
                }
            }
            Some(_) => {}
            None => entries.push((
                "Tags".to_string(),
                Expression::Array(tags.into_iter().map(Expression::String).collect()),
            )),
        }
        Expression::Map(entries)
    }

    /// Splits selector arguments on top-level commas, keeping nested
    /// brackets (`scores={...}`, `nbt={...}`) and quoted strings intact.
    pub(in crate::transpiler) fn split_selector_args(args: &str) -> Vec<&str> {
        let mut parts = Vec::new();
        let mut start = 0;
        let mut depth = 0usize;
        let mut quote: Option<char> = None;
        let mut escaped = false;
        for (index, ch) in args.char_indices() {
            if let Some(active) = quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == active {
                    quote = None;
                }
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                '[' | '{' | '(' => depth += 1,
                ']' | '}' | ')' => depth = depth.saturating_sub(1),
                ',' if depth == 0 => {
                    parts.push(args[start..index].trim());
                    start = index + 1;
                }
                _ => {}
            }
        }
        parts.push(args[start..].trim());
        parts
    }

    pub(in crate::transpiler) fn process_entity_def(
        &mut self,
        entity_def: &EntityDef,
    ) -> Result<(), TranspileError> {
        self.symbols
            .entity_templates
            .insert(entity_def.name.clone(), entity_def.clone());
        Ok(())
    }
}
