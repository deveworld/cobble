mod calls;
mod capture;
mod command_processor;
mod condition_lowering;
mod condition_translator;
mod const_eval;
mod data_pack;
mod error;
mod expression_evaluator;
mod expressions;
mod function_context;
mod intrinsics;
mod module_init;
mod resource_format;
mod score_holders;
mod source_map;
mod statement_processors;
mod stdlib_modules;
mod storage_paths;
mod symbols;
mod types;
pub mod vars_objective;

pub use data_pack::{
    BuildManifest, BuildManifestGenerated, BuildManifestInput, BuildManifestResourceEntry,
    BuildManifestValidation, DataPack, GeneratedCommand, GeneratedCommandKind, SourceLocation,
    SourceMap, SourceMapEntry, INIT_FUNCTION,
};
pub use error::{TranspileError, TranspileErrorKind, TranspileWarning};

use crate::ast::*;
use crate::pack_format::PackFormat;
use crate::stdlib::EventType;
use command_processor::CommandProcessor;
use source_map::SourceLocations;
use statement_processors::{ImportState, UnrollState};
use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;
use stdlib_modules::StdlibState;
use symbols::{FunctionTable, Symbols};

pub(crate) use condition_lowering::raw_minecraft_condition_verdict;
pub(crate) use function_context::function_locals;
pub use function_context::module_variables_made_local;

pub(in crate::transpiler) use capture::FunctionCapture;
pub(in crate::transpiler) use const_eval::const_mod;
pub(in crate::transpiler) use function_context::{FunctionContext, ReturnContext};
pub(in crate::transpiler) use module_init::ModuleLevelAssignment;
pub(in crate::transpiler) use storage_paths::StoragePathPlan;
pub(in crate::transpiler) use types::FunctionReturnKind;

/// Escapes text for a double-quoted JSON or SNBT string. Newlines, tabs, and
/// carriage returns become escapes, so a string never splits a command line.
pub(in crate::transpiler) fn escape_quoted(text: &str) -> String {
    let mut escaped = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '\\' => escaped.push_str("\\\\"),
            '"' => escaped.push_str("\\\""),
            '\n' => escaped.push_str("\\n"),
            '\r' => escaped.push_str("\\r"),
            '\t' => escaped.push_str("\\t"),
            _ => escaped.push(ch),
        }
    }
    escaped
}

/// Maximum exponent for compile-time power expansion (`x^n` emits `n - 1` multiplications).
pub(in crate::transpiler) const MAX_POWER_EXPONENT: i32 = 100;

/// Maximum import depth before a build is rejected. Shared source of truth
/// for the transpiler and the diagnostics import scanner
/// (`src/diagnostics/imports.rs` derives its copy from this constant).
pub(crate) const MAX_IMPORT_DEPTH: usize = 50;

/// Stdlib module names that can be imported via `from stdlib import <name>`.
///
/// `import stdlib` (without `from`) activates every module for backward
/// compatibility with 0.7.x. See `docs/src/stdlib/modules.md`. The canonical
/// table lives in `crate::stdlib::STDLIB_MODULES`.
pub(crate) use crate::stdlib::STDLIB_MODULES as ALL_STDLIB_MODULES;

pub struct Transpiler {
    pub data_pack: DataPack,
    /// Commands of the function being generated, and their source metadata.
    current_function: Option<Vec<String>>,
    current_function_metadata: Option<HashMap<usize, GeneratedCommand>>,
    current_context: FunctionContext,
    current_return: Option<ReturnContext>,
    /// Name of the user function whose body is currently being lowered.
    current_function_name: Option<String>,
    current_statement_source: Option<SourceLocation>,
    temp_counter: u32,
    symbols: Symbols,
    functions: FunctionTable,
    /// Module-level assignments in source order, replayed in `_cobble_init`.
    module_level_vars: Vec<ModuleLevelAssignment>,
    imports: ImportState,
    source_locations: SourceLocations,
    stdlib: StdlibState,
    /// Whether experimental resource-pack output is enabled for this build.
    experimental_resource_pack: bool,
    unroll: UnrollState,
    /// Stack of per-site condition holder slots for nested `execute` blocks.
    /// One frame per active `process_execute_block`; the frame counts the
    /// condition holders scoped to `@s` so their objectives can be pooled by
    /// (nesting depth, slot) instead of allocated per call site.
    execute_condition_slots: Vec<usize>,
    /// Warnings found since the last [`Transpiler::take_warnings`]. Constant
    /// folding finds some of them through `&self`.
    warnings: RefCell<Vec<TranspileWarning>>,
    /// Locals of the function being lowered that have the name of a module
    /// variable and haven't warned yet. The first assignment warns. Block
    /// helpers restore `current_context`, so this lives outside it.
    unwarned_shadows: Vec<String>,
}

fn record_warning(warnings: &RefCell<Vec<TranspileWarning>>, warning: TranspileWarning) {
    // Some expressions are evaluated more than once.
    let mut warnings = warnings.borrow_mut();
    if !warnings.contains(&warning) {
        warnings.push(warning);
    }
}

/// The warning for storing `value` in a score, when a score can't hold it.
/// Scores are 32-bit integers.
fn score_value_warning(value: f64, location: Option<SourceLocation>) -> Option<TranspileWarning> {
    let (kind, message, help) = if value > i32::MAX as f64 || value < i32::MIN as f64 {
        (
            "number-out-of-range",
            format!(
                "{value} is outside the scoreboard range, so it is stored as {}.",
                value as i32
            ),
            Some("Scores hold integers from -2147483648 to 2147483647.".to_string()),
        )
    } else if value.fract() != 0.0 {
        (
            "fractional-number",
            format!(
                "Scores hold integers, so {value} is stored as {}.",
                value as i32
            ),
            None,
        )
    } else {
        return None;
    };
    Some(TranspileWarning {
        kind,
        message,
        help,
        location,
    })
}

/// Build settings for a [`Transpiler`]. [`TranspilerOptions::new`] gives the
/// defaults; change the fields that differ.
pub struct TranspilerOptions {
    pub namespace: String,
    pub output_dir: PathBuf,
    /// `pack.mcmeta` description. `None` writes "Generated by Cobble".
    pub description: Option<String>,
    pub pack_format: PackFormat,
    /// `[stdlib] version` from `cobble.toml`. `None` uses version 2 and
    /// leaves the version out of the build manifest.
    pub stdlib_version: Option<u8>,
    pub experimental_resource_pack: bool,
    /// Folder that source paths in the manifest and source map are relative to.
    pub source_display_root: Option<PathBuf>,
    /// Project root marker and project ID that mark the output as this
    /// project's.
    pub project_identity: Option<(String, String)>,
    pub build_input: Option<BuildManifestInput>,
}

impl TranspilerOptions {
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self {
            namespace,
            output_dir,
            description: None,
            pack_format: crate::pack_format::SUPPORTED_PACK_FORMAT,
            stdlib_version: None,
            experimental_resource_pack: false,
            source_display_root: None,
            project_identity: None,
            build_input: None,
        }
    }
}

impl Transpiler {
    /// A transpiler with the default [`TranspilerOptions`].
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self::with_options(TranspilerOptions::new(namespace, output_dir))
    }

    pub fn with_options(options: TranspilerOptions) -> Self {
        let mut data_pack = DataPack::new(options.namespace, options.output_dir);
        if let Some(description) = options.description {
            data_pack.set_description(description);
        }
        data_pack.set_pack_format(options.pack_format);
        if let Some(root) = options.source_display_root {
            data_pack.set_source_display_root(root);
        }
        if let Some((project_root, project_id)) = options.project_identity {
            data_pack.set_project_identity(project_root, project_id);
        }
        if let Some(input) = options.build_input {
            data_pack.set_build_input(input);
        }
        if options.experimental_resource_pack {
            data_pack
                .experimental_features
                .push("resource_pack".to_string());
        }
        let mut transpiler = Self {
            data_pack,
            current_function: None,
            current_function_metadata: None,
            current_context: FunctionContext::new(),
            current_return: None,
            current_function_name: None,
            current_statement_source: None,
            temp_counter: 0,
            symbols: Symbols::default(),
            functions: FunctionTable::default(),
            module_level_vars: Vec::new(),
            imports: ImportState::default(),
            source_locations: SourceLocations::default(),
            stdlib: StdlibState::default(),
            experimental_resource_pack: options.experimental_resource_pack,
            unroll: UnrollState::default(),
            execute_condition_slots: Vec::new(),
            warnings: RefCell::new(Vec::new()),
            unwarned_shadows: Vec::new(),
        };
        if let Some(version) = options.stdlib_version {
            transpiler.set_stdlib_version(version);
        }
        transpiler
    }

    /// The warnings found since the last call, in the order they were
    /// found.
    pub fn take_warnings(&mut self) -> Vec<TranspileWarning> {
        std::mem::take(self.warnings.get_mut())
    }

    /// Records a warning about the statement being lowered.
    pub(in crate::transpiler) fn warn(
        &self,
        kind: &'static str,
        message: impl Into<String>,
        help: Option<&str>,
    ) {
        self.warn_at(kind, message, help, self.current_statement_source.clone());
    }

    pub(in crate::transpiler) fn warn_at(
        &self,
        kind: &'static str,
        message: impl Into<String>,
        help: Option<&str>,
        location: Option<SourceLocation>,
    ) {
        record_warning(
            &self.warnings,
            TranspileWarning {
                kind,
                message: message.into(),
                help: help.map(str::to_string),
                location,
            },
        );
    }

    /// Warns when `value` is stored in a score that can't hold it.
    pub(in crate::transpiler) fn warn_score_value(&self, value: f64) {
        if let Some(warning) = score_value_warning(value, self.current_statement_source.clone()) {
            record_warning(&self.warnings, warning);
        }
    }

    fn is_current_macro_param(&self, name: &str) -> bool {
        self.current_context.is_param(name)
    }

    pub fn transpile(&mut self, program: &Program) -> Result<(), TranspileError> {
        // Emit the stdlib v1 deprecation warning once per build.
        self.maybe_emit_stdlib_v1_warning();

        // When building multiple files, clear the import stack for each new file
        // The current file should already be in the stack from set_current_file()
        // We only keep the current file to properly detect circular imports within its imports
        if self.imports.stack.len() > 1 {
            // Keep only the last entry (the current file being transpiled)
            if let Some(current_file) = self.imports.stack.last() {
                let current = current_file.clone();
                self.imports.stack.clear();
                self.imports.stack.push(current);
            }
        }

        self.process_imports(program)?;

        // PASS 1: Register all function signatures first (to handle forward references)
        self.register_program_function_metadata(program);

        // PASS 2: Process all statements (function bodies, etc.)
        for statement in &program.statements {
            self.process_statement(statement)?;
        }

        // Direct self-calls error at the call site; this rejects mutual
        // recursion (a -> b -> a) across everything lowered so far.

        let module_init = if self.module_level_vars.is_empty() {
            None
        } else {
            Some(self.build_module_init_commands()?)
        };

        // Ensure objectives are initialized after module init commands have had
        // a chance to register every objective they need.
        self.data_pack.ensure_init_function();

        // Then initialize module-level variables in the init function, after
        // the runtime setup.
        if let Some((init_commands, init_metadata)) = module_init {
            if !init_commands.is_empty() {
                if !self.data_pack.functions.contains_key(INIT_FUNCTION) {
                    self.data_pack.add_function_with_metadata(
                        INIT_FUNCTION.to_string(),
                        Vec::new(),
                        HashMap::new(),
                    );
                    self.data_pack
                        .stdlib
                        .add_first_event_listener(EventType::Load, INIT_FUNCTION.to_string());
                }
                let setup_end = self
                    .data_pack
                    .runtime_setup_end_index(&self.data_pack.functions[INIT_FUNCTION]);
                self.data_pack.insert_function_commands_with_metadata(
                    INIT_FUNCTION,
                    setup_end,
                    &init_commands,
                    init_metadata,
                );
            }

            self.module_level_vars.clear();
        }

        // Sync active stdlib modules into the data pack for manifest output.
        self.sync_stdlib_modules_to_manifest();

        Ok(())
    }

    fn strip_command_prefix(cmd: &str) -> String {
        if let Some(stripped) = cmd.strip_prefix('/') {
            stripped.to_string()
        } else {
            cmd.to_string()
        }
    }

    fn process_statement(&mut self, statement: &Statement) -> Result<(), TranspileError> {
        let previous_statement_source = self.current_statement_source.clone();
        // An unrolled loop sets the location of each body statement itself.
        // Substituted text no longer matches its source line and could claim
        // the location of identical text elsewhere in the file.
        let statement_source = if self.unroll.depth > 0 {
            previous_statement_source.clone()
        } else {
            self.take_statement_source(statement)
                .or_else(|| previous_statement_source.clone())
        };
        self.current_statement_source = statement_source.clone();
        if let Some(name) = function_context::direct_binding(statement) {
            if let Some(index) = self.unwarned_shadows.iter().position(|local| local == name) {
                let local = self.unwarned_shadows.remove(index);
                self.warn_shadowed_module_variable(&local, statement_source.clone());
            }
        }

        let start_index = self.current_function.as_ref().map(Vec::len);
        let default_kind = Self::default_generated_kind_for_statement(statement);
        let result = self
            .process_statement_inner(statement)
            .and_then(|()| match &self.current_context.unbound_read {
                Some(read) if read.top_level == function_context::statement_address(statement) => {
                    Err(self.unbound_local_error(&read.name.clone()))
                }
                _ => Ok(()),
            })
            .map_err(|error| error.with_location_if_missing(statement_source.clone()));

        if result.is_ok() {
            if let Some(start_index) = start_index {
                self.call_reassigned_parameter_macros(start_index);
                self.annotate_recent_commands(start_index, statement_source, default_kind);
            }
        }

        self.current_statement_source = previous_statement_source;
        result
    }

    fn unbound_local_error(&self, name: &str) -> TranspileError {
        let function = self.current_function_name.as_deref().unwrap_or_default();
        function_context::unbound_local_message(name, function).into()
    }

    fn process_statement_inner(&mut self, statement: &Statement) -> Result<(), TranspileError> {
        if let Some(read) = &self.current_context.unbound_read {
            if read.statement == function_context::statement_address(statement) {
                return Err(self.unbound_local_error(&read.name.clone()));
            }
        }
        match statement {
            Statement::Import(import) => {
                self.process_import(import)?;
            }
            Statement::FunctionDef(func) => {
                self.process_function_def(func)?;
            }
            Statement::Assignment(assign) => {
                self.process_assignment(assign)?;
            }
            Statement::ConstAssignment(const_assign) => {
                self.process_const_assignment(const_assign)?;
            }
            Statement::Expression(expr) => {
                self.process_expression(expr)?;
            }
            Statement::If(if_stmt) => {
                self.process_if(if_stmt)?;
            }
            Statement::For(for_loop) => {
                self.process_for(for_loop)?;
            }
            Statement::While(while_loop) => {
                self.process_while(while_loop)?;
            }
            Statement::Match(match_stmt) => {
                self.process_match(match_stmt)?;
            }
            Statement::Return(expr) => {
                self.process_return(expr.as_ref())?;
            }
            Statement::Pass => {
                // Pass is a no-op
            }
            Statement::Global(vars) => {
                // Mark variables as global in current function
                for var in vars {
                    if self.current_context.is_param(var) {
                        return Err(format!(
                            "`{}` is a parameter of this function, so it can't also be declared `global`. Rename the parameter to use the module variable.",
                            var
                        )
                        .into());
                    }
                    self.symbols.global_variables.insert(var.clone());
                }
            }
            Statement::MinecraftCommand(cmd) => {
                // Strip leading slash and process variable/parameter substitution
                let clean_cmd = Self::strip_command_prefix(cmd);
                let source_location = if self.unroll.depth > 0 {
                    self.current_statement_source.clone()
                } else {
                    self.take_raw_command_source(clean_cmd.trim())
                };
                if Self::is_raw_return_command(cmd) && self.in_recursive_function() {
                    let function = self.current_function_name.as_deref().unwrap_or_default();
                    return Err(TranspileError::from(format!(
                        "A raw `/return` in the recursive function `{function}` would skip restoring the caller's variables. Use Cobble's `return` instead."
                    ))
                    .with_location_if_missing(source_location));
                }
                let processor = CommandProcessor {
                    namespace: self.data_pack.namespace.clone(),
                    current_params: &self.current_context.params,
                    symbols: &self.symbols,
                };

                let say_macro = processor
                    .say_macro(&clean_cmd)
                    .map_err(|error| error.with_location_if_missing(source_location.clone()))?;
                if let Some(say_macro) = say_macro {
                    return self.emit_say_macro(say_macro, source_location);
                }
                let processed_cmd = processor
                    .process_command_string(&clean_cmd)
                    .map_err(|error| error.with_location_if_missing(source_location.clone()))?;

                if let Some(ref mut commands) = self.current_function {
                    let command_index = commands.len();
                    commands.push(processed_cmd);
                    if let Some(ref mut metadata) = self.current_function_metadata {
                        metadata.insert(
                            command_index,
                            GeneratedCommand::new(
                                commands[command_index].clone(),
                                source_location,
                                GeneratedCommandKind::UserCommand,
                            ),
                        );
                    }
                } else {
                    return Err("Minecraft command outside of function context"
                        .to_string()
                        .into());
                }
            }
            Statement::Execute(exec_block) => {
                self.process_execute_block(exec_block)?;
            }
            Statement::SelectorDef(selector_def) => {
                self.process_selector_def(selector_def)?;
            }
            Statement::EntityDef(entity_def) => {
                self.process_entity_def(entity_def)?;
            }
            Statement::CreateEntity(name) => {
                self.process_create_entity(name)?;
            }
        }
        Ok(())
    }

    fn default_generated_kind_for_statement(statement: &Statement) -> GeneratedCommandKind {
        match statement {
            Statement::MinecraftCommand(_) => GeneratedCommandKind::UserCommand,
            Statement::Expression(expr) if Self::is_stdlib_expression(expr) => {
                GeneratedCommandKind::StdLib
            }
            _ => GeneratedCommandKind::ControlFlow,
        }
    }

    fn is_stdlib_expression(expr: &Expression) -> bool {
        let Expression::Call(func, _) = expr else {
            return false;
        };

        match &**func {
            Expression::Attribute(obj, _) => {
                let Some(module_path) = Self::expression_path(obj) else {
                    return false;
                };
                let root = module_path.split('.').next().unwrap_or_default();
                matches!(
                    root,
                    "math"
                        | "text"
                        | "score"
                        | "random"
                        | "timer"
                        | "storage"
                        | "item_component"
                        | "schedule"
                        | "selector"
                        | "position"
                        | "bossbar"
                        | "team"
                        | "entity"
                        | "datapack"
                        | "resource_pack"
                )
            }
            Expression::Identifier(name) => {
                name == "addEventListener" || name == "stdlib.addEventListener"
            }
            _ => false,
        }
    }

    fn expression_path(expr: &Expression) -> Option<String> {
        match expr {
            Expression::Identifier(name) => Some(name.clone()),
            Expression::Attribute(obj, attr) => {
                Some(format!("{}.{}", Self::expression_path(obj)?, attr))
            }
            _ => None,
        }
    }

    fn push_current_command(&mut self, command: String) -> Result<(), TranspileError> {
        if let Some(ref mut commands) = self.current_function {
            let command_index = commands.len();
            commands.push(command);
            if let Some(ref mut metadata) = self.current_function_metadata {
                metadata.insert(
                    command_index,
                    GeneratedCommand::new(
                        commands[command_index].clone(),
                        None,
                        GeneratedCommandKind::StdLib,
                    ),
                );
            }
            Ok(())
        } else {
            Err("Stdlib helper called outside of function context"
                .to_string()
                .into())
        }
    }

    fn push_current_commands(&mut self, commands: Vec<String>) -> Result<(), TranspileError> {
        for command in commands {
            self.push_current_command(command)?;
        }
        Ok(())
    }

    fn next_temp_id(&mut self) -> u32 {
        let id = self.temp_counter;
        self.temp_counter += 1;
        id
    }

    pub fn write_data_pack(&self) -> std::io::Result<()> {
        self.data_pack.write()
    }
}
