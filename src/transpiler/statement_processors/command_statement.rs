use crate::transpiler::command_processor::{SayMacro, SayMacroSource};
use crate::transpiler::storage_paths::StoragePathOps;
use crate::transpiler::{
    GeneratedCommand, GeneratedCommandKind, SourceLocation, TranspileError, Transpiler,
};

impl Transpiler {
    /// A separate macro call lets us escape hover strings before Minecraft
    /// instantiates the text command, while leaving `args` unchanged.
    pub(in crate::transpiler) fn quoted_hover_commands(
        &mut self,
        original: &str,
        command: String,
    ) -> Vec<String> {
        // Explicit macro syntax is the author's literal command text.
        if original.contains("$(") {
            return vec![command];
        }
        let Some(plan) = crate::transpiler::command_processor::quoted_hover_macro(&command) else {
            return vec![command];
        };
        crate::transpiler::text_escape::add_hover_functions(&mut self.data_pack);
        let ns = self.data_pack.namespace.clone();
        let id = self.next_temp_id();
        let path = format!("_cobble_quoted_args_{id}");
        let storage = format!("storage {ns}:global");
        let mut commands = vec![format!(
            "data modify {storage} {path} set from {storage} args"
        )];
        for (name, escaped) in plan.arguments {
            commands.extend([
                format!("data remove {storage} _cobble_esc.key"),
                format!("data modify {storage} _cobble_esc.key set from {storage} args.{name}"),
                format!(
                    "function {ns}:{}",
                    crate::transpiler::text_escape::ESCAPE_HOVER
                ),
                format!(
                    "data modify {storage} {path}.{escaped} set from {storage} _cobble_esc.done"
                ),
            ]);
        }
        let helper = format!("_cobble_hover_{id}");
        self.add_generated_helper_function(
            helper.clone(),
            vec![plan.command],
            GeneratedCommandKind::UserCommand,
        );
        commands.push(format!("function {ns}:{helper} with {storage} {path}"));
        commands
    }

    /// A macro line reads the arguments the function was called with, so
    /// after the function assigns a parameter, `$(name)` would still give the
    /// old value. Lines from `start_index` on that read a reassigned
    /// parameter move into a helper called with the current `args`.
    /// `return run` keeps the command's result for `execute store`.
    pub(in crate::transpiler) fn call_reassigned_parameter_macros(&mut self, start_index: usize) {
        if self.current_context.reassigned_params.is_empty() {
            return;
        }
        let Some(commands) = self.current_function.as_ref() else {
            return;
        };
        let stale: Vec<(usize, String)> = commands
            .iter()
            .enumerate()
            .skip(start_index)
            .filter(|(_, command)| {
                command.starts_with('$')
                    && !Self::is_raw_return_command(command)
                    && self
                        .current_context
                        .reassigned_params
                        .iter()
                        .any(|param| command.contains(&format!("$({})", param)))
            })
            .map(|(index, command)| (index, command.clone()))
            .collect();
        for (index, command) in stale {
            let helper_name = format!("_cobble_macro_{}", self.next_temp_id());
            self.add_generated_helper_function(
                helper_name.clone(),
                vec![format!("$return run {}", &command[1..])],
                GeneratedCommandKind::UserCommand,
            );
            let call = self.function_call_command(&helper_name, true);
            if let Some(commands) = self.current_function.as_mut() {
                commands[index] = call.clone();
            }
            if let Some(generated) = self
                .current_function_metadata
                .as_mut()
                .and_then(|metadata| metadata.get_mut(&index))
            {
                generated.text = call;
            }
        }
    }

    /// Lowers `say` text with variables: copy each value into
    /// `<namespace>:global _say_args`, then run the text as a macro `say` in
    /// a helper, so the output keeps `say`'s `[name]` prefix and selector
    /// names.
    pub(in crate::transpiler) fn emit_say_macro(
        &mut self,
        say_macro: SayMacro,
        source: Option<SourceLocation>,
    ) -> Result<(), TranspileError> {
        if self.current_function.is_none() {
            return Err("Minecraft command outside of function context".into());
        }
        let namespace = self.data_pack.namespace.clone();
        let storage = format!("{namespace}:global");
        let mut commands = Vec::new();
        for argument in &say_macro.arguments {
            let name = &argument.name;
            commands.push(match &argument.source {
                SayMacroSource::Score { objective } => format!(
                    "execute store result storage {storage} _say_args.{name} int 1 run scoreboard players get {name} {objective}"
                ),
                SayMacroSource::Storage { path } => format!(
                    "data modify storage {storage} _say_args.{name} set from storage {storage} {path}"
                ),
                SayMacroSource::Parameter => format!(
                    "data modify storage {storage} _say_args.{name} set from storage {storage} args.{name}"
                ),
            });
        }

        let helper = format!("_cobble_say_{}", self.next_temp_id());
        commands.push(format!(
            "function {namespace}:{helper} with storage {storage} _say_args"
        ));
        // Text substituted by an unrolled loop has no raw location of its own.
        let source = source.or_else(|| self.current_statement_source.clone());
        // `return 1` gives the call the result `say` itself has, for `store`
        // blocks and `execute if function`.
        let helper_commands = vec![say_macro.command, "return 1".to_string()];
        let helper_metadata = helper_commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(
                        command.clone(),
                        source.clone(),
                        GeneratedCommandKind::UserCommand,
                    ),
                )
            })
            .collect();
        self.data_pack
            .add_function_with_metadata(helper, helper_commands, helper_metadata);

        let function = self
            .current_function
            .as_mut()
            .expect("checked above that a function is being lowered");
        let start = function.len();
        function.extend(commands);
        if let Some(ref mut metadata) = self.current_function_metadata {
            for (index, command) in function.iter().enumerate().skip(start) {
                metadata.insert(
                    index,
                    GeneratedCommand::new(
                        command.clone(),
                        source.clone(),
                        GeneratedCommandKind::UserCommand,
                    ),
                );
            }
        }
        Ok(())
    }
}
