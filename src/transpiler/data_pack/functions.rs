use super::{DataPack, GeneratedCommand, GeneratedCommandKind};
use std::collections::HashMap;

impl DataPack {
    pub(super) fn metadata_for_commands(
        commands: &[String],
        kind: GeneratedCommandKind,
    ) -> HashMap<usize, GeneratedCommand> {
        commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(command.clone(), None, kind.clone()),
                )
            })
            .collect()
    }

    pub(super) fn complete_metadata(
        commands: &[String],
        mut metadata: HashMap<usize, GeneratedCommand>,
        default_kind: GeneratedCommandKind,
    ) -> HashMap<usize, GeneratedCommand> {
        for (index, command) in commands.iter().enumerate() {
            metadata.entry(index).or_insert_with(|| {
                GeneratedCommand::new(command.clone(), None, default_kind.clone())
            });
        }
        metadata
    }

    pub fn add_function(&mut self, name: String, commands: Vec<String>) {
        self.add_function_with_kind(name, commands, GeneratedCommandKind::ControlFlow);
    }

    pub fn add_function_with_kind(
        &mut self,
        name: String,
        commands: Vec<String>,
        kind: GeneratedCommandKind,
    ) {
        let metadata = Self::metadata_for_commands(&commands, kind);
        self.command_metadata.insert(name.clone(), metadata);
        self.functions.insert(name, commands);
    }

    pub fn add_function_with_metadata(
        &mut self,
        name: String,
        commands: Vec<String>,
        metadata: HashMap<usize, GeneratedCommand>,
    ) {
        let metadata =
            Self::complete_metadata(&commands, metadata, GeneratedCommandKind::ControlFlow);
        self.command_metadata.insert(name.clone(), metadata);
        self.functions.insert(name, commands);
    }

    pub fn insert_function_commands_with_metadata(
        &mut self,
        name: &str,
        index: usize,
        new_commands: &[String],
        new_metadata: HashMap<usize, GeneratedCommand>,
    ) {
        if new_commands.is_empty() {
            return;
        }

        let Some(commands) = self.functions.get_mut(name) else {
            return;
        };
        let insert_index = index.min(commands.len());
        for (offset, command) in new_commands.iter().enumerate() {
            commands.insert(insert_index + offset, command.clone());
        }

        let existing_metadata = self.command_metadata.remove(name).unwrap_or_default();
        let mut updated_metadata = HashMap::new();
        for (old_index, metadata) in existing_metadata {
            let new_index = if old_index >= insert_index {
                old_index + new_commands.len()
            } else {
                old_index
            };
            updated_metadata.insert(new_index, metadata);
        }

        for (offset, command) in new_commands.iter().enumerate() {
            let generated = new_metadata.get(&offset).cloned().unwrap_or_else(|| {
                GeneratedCommand::new(command.clone(), None, GeneratedCommandKind::ControlFlow)
            });
            updated_metadata.insert(insert_index + offset, generated);
        }

        let completed = Self::complete_metadata(
            commands,
            updated_metadata,
            GeneratedCommandKind::ControlFlow,
        );
        self.command_metadata.insert(name.to_string(), completed);
    }
}
