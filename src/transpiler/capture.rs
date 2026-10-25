use crate::ast::Statement;
use crate::transpiler::symbols::Symbols;
use crate::transpiler::{
    FunctionContext, GeneratedCommand, GeneratedCommandKind, TranspileError, Transpiler,
};
use std::collections::HashMap;

pub(in crate::transpiler) struct FunctionCapture {
    pub(in crate::transpiler) commands: Vec<String>,
    pub(in crate::transpiler) metadata: HashMap<usize, GeneratedCommand>,
}

pub(in crate::transpiler) struct SavedFunctionCapture {
    function: Option<Vec<String>>,
    metadata: Option<HashMap<usize, GeneratedCommand>>,
}

pub(in crate::transpiler) struct SavedCompilerState {
    current_context: FunctionContext,
    symbols: Symbols,
}

impl FunctionCapture {
    pub(in crate::transpiler) fn requires_macro_context(&self) -> bool {
        self.commands
            .iter()
            .any(|command| command.trim_start().starts_with('$'))
    }
}

impl Transpiler {
    pub(in crate::transpiler) fn capture_statements(
        &mut self,
        statements: &[Statement],
    ) -> Result<FunctionCapture, TranspileError> {
        self.capture_commands(|transpiler| {
            for statement in statements {
                transpiler.process_statement(statement)?;
            }
            Ok(())
        })
    }

    pub(in crate::transpiler) fn capture_statements_isolated(
        &mut self,
        statements: &[Statement],
    ) -> Result<FunctionCapture, TranspileError> {
        let saved_state = self.save_compiler_state();
        let result = self.capture_statements(statements);
        self.restore_compiler_state(saved_state);
        result
    }

    pub(in crate::transpiler) fn capture_commands<F>(
        &mut self,
        body: F,
    ) -> Result<FunctionCapture, TranspileError>
    where
        F: FnOnce(&mut Self) -> Result<(), TranspileError>,
    {
        let saved = self.begin_function_capture();
        if let Err(error) = body(self) {
            self.restore_function_capture(saved);
            return Err(error);
        }
        Ok(self.finish_function_capture(saved))
    }

    pub(in crate::transpiler) fn capture_commands_with_result<T, F>(
        &mut self,
        body: F,
    ) -> Result<(T, FunctionCapture), TranspileError>
    where
        F: FnOnce(&mut Self) -> Result<T, TranspileError>,
    {
        let saved = self.begin_function_capture();
        let result = match body(self) {
            Ok(result) => result,
            Err(error) => {
                self.restore_function_capture(saved);
                return Err(error);
            }
        };
        let capture = self.finish_function_capture(saved);
        Ok((result, capture))
    }

    pub(in crate::transpiler) fn begin_function_capture(&mut self) -> SavedFunctionCapture {
        let saved = SavedFunctionCapture {
            function: self.current_function.take(),
            metadata: self.current_function_metadata.take(),
        };
        self.current_function = Some(Vec::new());
        self.current_function_metadata = Some(HashMap::new());
        saved
    }

    pub(in crate::transpiler) fn finish_function_capture(
        &mut self,
        saved: SavedFunctionCapture,
    ) -> FunctionCapture {
        self.annotate_recent_commands(
            0,
            self.current_statement_source.clone(),
            GeneratedCommandKind::ControlFlow,
        );
        let capture = FunctionCapture {
            commands: self.current_function.take().unwrap_or_default(),
            metadata: self.current_function_metadata.take().unwrap_or_default(),
        };
        self.restore_function_capture(saved);
        capture
    }

    pub(in crate::transpiler) fn restore_function_capture(&mut self, saved: SavedFunctionCapture) {
        self.current_function = saved.function;
        self.current_function_metadata = saved.metadata;
    }

    pub(in crate::transpiler) fn save_compiler_state(&self) -> SavedCompilerState {
        SavedCompilerState {
            current_context: self.current_context.clone(),
            symbols: self.symbols.clone(),
        }
    }

    pub(in crate::transpiler) fn restore_compiler_state(&mut self, saved: SavedCompilerState) {
        self.current_context = saved.current_context;
        self.symbols = saved.symbols;
    }

    pub(in crate::transpiler) fn add_captured_function(
        &mut self,
        name: String,
        capture: FunctionCapture,
    ) {
        self.data_pack
            .add_function_with_metadata(name, capture.commands, capture.metadata);
    }

    pub(in crate::transpiler) fn function_call_command(
        &self,
        name: &str,
        with_storage_args: bool,
    ) -> String {
        if with_storage_args {
            format!(
                "function {}:{} with storage {}:global args",
                self.data_pack.namespace, name, self.data_pack.namespace
            )
        } else {
            format!("function {}:{}", self.data_pack.namespace, name)
        }
    }

    pub(in crate::transpiler) fn append_transformed_capture<F>(
        &mut self,
        capture: FunctionCapture,
        mut transform: F,
    ) -> Result<(), TranspileError>
    where
        F: FnMut(&str) -> String,
    {
        let Some(ref mut commands) = self.current_function else {
            return Err(
                "Cannot append generated commands outside of function context"
                    .to_string()
                    .into(),
            );
        };

        for (source_index, command) in capture.commands.into_iter().enumerate() {
            let transformed = transform(&command);
            let target_index = commands.len();
            commands.push(transformed.clone());

            if let Some(ref mut metadata) = self.current_function_metadata {
                let generated = capture
                    .metadata
                    .get(&source_index)
                    .cloned()
                    .map(|mut generated| {
                        generated.text = transformed.clone();
                        generated
                    })
                    .unwrap_or_else(|| {
                        GeneratedCommand::new(transformed, None, GeneratedCommandKind::ControlFlow)
                    });
                metadata.insert(target_index, generated);
            }
        }

        Ok(())
    }
}
