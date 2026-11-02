//! Storage is authoritative for source-level scalar variables. Score commands
//! are an intermediate representation and a compatibility surface for raw
//! `temp` commands. Lower their reads and writes after local names are scoped.

use super::score_holders::{holder_refs, tokenize};
use super::{DataPack, GeneratedCommand, GeneratedCommandKind, Transpiler};
use crate::ast::CobbleType;
use std::collections::{BTreeMap, BTreeSet, HashMap};

#[derive(Default)]
pub(super) struct StorageVariables {
    bindings: BTreeMap<String, String>,
    original: Option<OriginalFunctions>,
}

struct OriginalFunctions {
    functions: HashMap<String, Vec<String>>,
    metadata: HashMap<String, HashMap<usize, GeneratedCommand>>,
    created: Vec<String>,
}

impl StorageVariables {
    pub(super) fn restore(&mut self, pack: &mut DataPack) {
        if let Some(original) = self.original.take() {
            pack.functions = original.functions;
            pack.command_metadata = original.metadata;
            pack.created_functions = original.created;
        }
    }

    pub(super) fn bind(&mut self, holder: String, path: String) {
        self.bindings.insert(holder, path);
    }

    pub(super) fn lower(&mut self, pack: &mut DataPack) {
        if self.bindings.is_empty() {
            return;
        }
        self.original = Some(OriginalFunctions {
            functions: pack.functions.clone(),
            metadata: pack.command_metadata.clone(),
            created: pack.created_functions.clone(),
        });
        let mut lowering = Lowering {
            namespace: pack.namespace.clone(),
            objective: pack.vars_objective(),
            bindings: &self.bindings,
            helpers: Vec::new(),
        };
        let mut names: Vec<_> = pack.functions.keys().cloned().collect();
        names.sort();
        for name in names {
            let original = pack.functions[&name].clone();
            let metadata = pack
                .command_metadata
                .get(&name)
                .cloned()
                .unwrap_or_default();
            let mut commands = Vec::new();
            let mut mapped = HashMap::new();
            for (index, command) in original.iter().enumerate() {
                let origin = metadata.get(&index).cloned().unwrap_or_else(|| {
                    GeneratedCommand::new(command.clone(), None, GeneratedCommandKind::ControlFlow)
                });
                for text in lowering.command(command, false, &origin) {
                    mapped.insert(
                        commands.len(),
                        GeneratedCommand::new(
                            text.clone(),
                            origin.source.clone(),
                            origin.kind.clone(),
                        ),
                    );
                    commands.push(text);
                }
            }
            pack.functions.insert(name.clone(), commands);
            pack.command_metadata.insert(name, mapped);
        }
        for (name, commands, origin) in lowering.helpers {
            let metadata = commands
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    (
                        index,
                        GeneratedCommand::new(
                            text.clone(),
                            origin.source.clone(),
                            origin.kind.clone(),
                        ),
                    )
                })
                .collect();
            pack.add_function_with_metadata(name, commands, metadata);
        }
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn bind_scalar_storage(&mut self, name: &str) {
        if !self.uses_storage_numbers() || name.starts_with('#') {
            return;
        }
        if !matches!(
            self.symbols.variable_types.get(name),
            Some(CobbleType::Integer | CobbleType::Boolean)
        ) && !self.current_context.score_writes.contains(name)
        {
            return;
        }
        let holder = match self.current_function_name.as_deref() {
            Some(function) if self.current_context.is_local(name) => {
                format!("#{function}.{name}")
            }
            _ => name.to_string(),
        };
        self.data_pack.track_objective("temp");
        let path = self.variable_storage_path(name);
        if name.starts_with("_cobble_hold_") {
            self.storage_variables.bind(name.to_string(), path.clone());
        }
        self.storage_variables.bind(holder, path);
    }
}

struct Lowering<'a> {
    namespace: String,
    objective: String,
    bindings: &'a BTreeMap<String, String>,
    helpers: Vec<(String, Vec<String>, GeneratedCommand)>,
}

impl Lowering<'_> {
    fn loads(&self, command: &str, include_written: bool) -> Vec<String> {
        let names: BTreeSet<_> = holder_refs(command, &self.objective)
            .into_iter()
            .filter(|reference| include_written || !reference.written)
            .map(|reference| &command[reference.start..reference.end])
            .filter(|name| self.bindings.contains_key(*name))
            .collect();
        names.into_iter().flat_map(|name| {
            let path = &self.bindings[name];
            vec![
                format!("scoreboard players reset {name} {}", self.objective),
                format!("execute if data storage {}:global {path} store result score {name} {} run data get storage {}:global {path}", self.namespace, self.objective, self.namespace),
            ]
        }).collect()
    }

    fn stores(&self, command: &str) -> Vec<String> {
        let names: BTreeSet<_> = holder_refs(command, &self.objective)
            .into_iter()
            .filter(|reference| reference.written)
            .map(|reference| &command[reference.start..reference.end])
            .filter(|name| self.bindings.contains_key(*name))
            .collect();
        names.into_iter().flat_map(|name| {
            let path = &self.bindings[name];
            vec![
                format!("execute unless score {name} {} matches -2147483648.. run data remove storage {}:global {path}", self.objective, self.namespace),
                format!("execute if score {name} {} matches -2147483648.. store result storage {}:global {path} int 1 run scoreboard players get {name} {}", self.objective, self.namespace, self.objective),
            ]
        }).collect()
    }

    fn command(&mut self, command: &str, nested: bool, origin: &GeneratedCommand) -> Vec<String> {
        let macro_line = command.starts_with('$');
        let source = command.strip_prefix('$').unwrap_or(command);
        let tokens = tokenize(source);
        let words: Vec<_> = tokens
            .iter()
            .map(|&(start, end)| &source[start..end])
            .collect();
        // Store callbacks write storage directly, including failed commands
        // and returns. This preserves execute's conditional-store semantics.
        let mut edits = Vec::new();
        if words.first() == Some(&"execute") {
            for i in 1..words.len().saturating_sub(4) {
                if words[i] == "run" {
                    break;
                }
                if words[i] == "store"
                    && matches!(words[i + 1], "result" | "success")
                    && words[i + 2] == "score"
                    && words[i + 4] == self.objective
                {
                    if let Some(path) = self.bindings.get(words[i + 3]) {
                        // A generated mirror loads the score from the canonical value.
                        // Redirecting it back to storage would narrow a float loop item.
                        let mirror = format!("data get storage {}:global {path}", self.namespace);
                        let tail = source.split_once(" run ").map(|(_, tail)| tail);
                        if !matches!(origin.kind, GeneratedCommandKind::UserCommand)
                            && words[i + 1] == "result"
                            && tail
                                .is_some_and(|tail| tail == mirror || tail == format!("{mirror} 1"))
                        {
                            continue;
                        }
                        edits.push((
                            tokens[i + 2].0,
                            tokens[i + 4].1,
                            format!("storage {}:global {path} int 1", self.namespace),
                        ));
                    }
                }
            }
        }
        let mut prepared = source.to_string();
        for (start, end, replacement) in edits.into_iter().rev() {
            prepared.replace_range(start..end, &replacement);
        }
        let tokens = tokenize(&prepared);
        let words: Vec<_> = tokens
            .iter()
            .map(|&(start, end)| &prepared[start..end])
            .collect();
        let run = if words.first() == Some(&"execute") {
            words.iter().position(|word| *word == "run")
        } else if words.first() == Some(&"return") && words.get(1) == Some(&"run") {
            Some(1)
        } else {
            None
        };
        if let Some(run) = run {
            if let Some(&(start, _)) = tokens.get(run + 1) {
                let mut out = self.loads(&prepared[..start], false);
                let mut child = self.command(&prepared[start..], true, origin);
                let tail = child.pop().expect("a command always has a tail");
                out.extend(child);
                let combined = format!("{}{}", &prepared[..start], tail.trim_start_matches('$'));
                out.push(if macro_line || combined.contains("$(") {
                    format!("${combined}")
                } else {
                    combined
                });
                return out;
            }
        }
        let stores = self.stores(&prepared);
        if stores.is_empty() {
            let mut out = self.loads(&prepared, false);
            out.push(if macro_line {
                format!("${prepared}")
            } else {
                prepared
            });
            return out;
        }
        let mut commands = self.loads(&prepared, true);
        if !nested {
            commands.push(if macro_line {
                format!("${prepared}")
            } else {
                prepared
            });
            commands.extend(stores);
            return commands;
        }
        let id = self.helpers.len();
        let name = format!("_cobble_scalar_write_{id}");
        let ok = format!("#scalar_ok_{id}");
        let result = format!("#scalar_result_{id}");
        let capture = format!(
            "execute store success score {ok} {} store result score {result} {} run {prepared}",
            self.objective, self.objective
        );
        commands.push(if prepared.contains("$(") {
            format!("${capture}")
        } else {
            capture
        });
        commands.extend(stores);
        commands.push(format!(
            "execute if score {ok} {} matches 0 run return fail",
            self.objective
        ));
        commands.push(format!(
            "return run scoreboard players get {result} {}",
            self.objective
        ));
        let fields = macro_fields(&prepared);
        self.helpers.push((name.clone(), commands, origin.clone()));
        let call = if fields.is_empty() {
            format!("function {}:{name}", self.namespace)
        } else {
            let args = fields
                .into_iter()
                .map(|field| format!("{field}:$({field})"))
                .collect::<Vec<_>>()
                .join(",");
            format!("$function {}:{name} {{{args}}}", self.namespace)
        };
        vec![call]
    }
}

fn macro_fields(command: &str) -> BTreeSet<&str> {
    let mut fields = BTreeSet::new();
    let mut remaining = command;
    while let Some(open) = remaining.find("$(") {
        remaining = &remaining[open + 2..];
        let Some(close) = remaining.find(')') else {
            break;
        };
        fields.insert(&remaining[..close]);
        remaining = &remaining[close + 1..];
    }
    fields
}
