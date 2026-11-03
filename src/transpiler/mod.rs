mod calls;
mod capture;
mod command_processor;
mod condition_lowering;
mod condition_translator;
mod const_eval;
mod container_writes;
mod data_pack;
mod desugar;
mod dynamic_intrinsics;
mod error;
mod expression_evaluator;
mod expressions;
mod function_context;
mod intrinsics;
mod module_init;
mod numbers;
mod numeric_runtime;
mod placeholders;
pub(crate) mod resource_format;
mod resource_references;
pub(crate) mod scoped;
mod score_holders;
mod snbt_text;
mod source_map;
mod statement_processors;
mod stdlib_modules;
mod storage_paths;
mod storage_variables;
mod symbols;
mod text_escape;
mod types;
mod value_builders;
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
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use stdlib_modules::StdlibState;
use symbols::{FunctionTable, Symbols};

pub(crate) use condition_lowering::{
    mask_minecraft_conditions, merged_execute_in_condition, minecraft_condition_expression,
    minecraft_condition_ranges, raw_minecraft_condition_verdict,
};
pub use function_context::module_variables_made_local;
pub(crate) use function_context::{function_locals, global_assignments, module_loop_variables};

pub(in crate::transpiler) use capture::FunctionCapture;
pub(in crate::transpiler) use const_eval::const_mod;
pub(in crate::transpiler) use function_context::{FunctionContext, ReturnContext};
pub(crate) use module_init::module_statements;
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
    storage_variables: storage_variables::StorageVariables,
    functions: FunctionTable,
    /// Module-level assignments in source order, replayed in `_cobble_init`.
    module_level_vars: Vec<ModuleLevelAssignment>,
    /// Prelude helpers already in the pack.
    real_variables: HashSet<String>,
    prelude_compiled: HashSet<String>,
    prelude_sources: HashMap<&'static str, crate::ast::Program>,
    /// Whether the statements being lowered come from the prelude, so they
    /// have no source lines of their own.
    in_prelude: bool,
    imports: ImportState,
    source_locations: SourceLocations,
    /// The references between `datapack.*` resources, checked once every
    /// file is compiled (see [`Transpiler::check_resource_references`]).
    resource_references: Vec<resource_references::DeclaredReference>,
    stdlib: StdlibState,
    /// Whether experimental resource-pack output is enabled for this build.
    experimental_resource_pack: bool,
    unroll: UnrollState,
    /// Module-level loop variables that are real module variables (see
    /// `function_context::module_loop_variables`).
    module_loop_variables: std::collections::BTreeSet<String>,
    /// The source location of each statement of the copies an expanded
    /// loop lowers, by statement address, while the copy is lowered.
    unrolled_sources: HashMap<usize, Option<SourceLocation>>,
    /// Sources recorded while lowering a public entry, then reused by its
    /// safe text entry without consuming another function's source queues.
    replay_sources: Option<HashMap<usize, Option<SourceLocation>>>,
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
    /// Names the files declare `const` at module level.
    module_consts: HashSet<String>,
    /// The runtime list loops being lowered whose body may rebind their
    /// list, innermost last.
    list_loops: Vec<statement_processors::ActiveListLoop>,
    /// Module variables a runtime loop walks while its body calls a
    /// function, which may rebind the variable. Every rebind of such a
    /// variable tells a running loop over it.
    signalled_list_roots: std::collections::BTreeSet<String>,
    /// Whether the statements being lowered are module-level code that
    /// runs in `_cobble_init` (see `Transpiler::process_module_code`).
    in_module_code: bool,
    loop_depth: usize,
}

/// The expressions `statement` itself evaluates, without its body.
fn statement_expressions(statement: &Statement) -> Vec<&Expression> {
    match statement {
        Statement::Assignment(assign) => {
            let mut parts = vec![&assign.value];
            parts.extend(&assign.target_expr);
            parts
        }
        Statement::MultiAssign(multi) => multi.targets.iter().chain(&multi.values).collect(),
        Statement::Expression(expr) | Statement::Return(Some(expr)) => vec![expr],
        Statement::If(if_stmt) => std::iter::once(&if_stmt.condition)
            .chain(if_stmt.elif_branches.iter().map(|(condition, _)| condition))
            .collect(),
        Statement::For(for_loop) => std::iter::once(&for_loop.iter)
            .chain(&for_loop.step)
            .collect(),
        Statement::While(while_loop) => vec![&while_loop.condition],
        Statement::Match(match_stmt) => vec![&match_stmt.value],
        _ => Vec::new(),
    }
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
    pub command_limit: Option<u32>,
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
            command_limit: None,
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
        data_pack.command_limit = options.command_limit;
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
            storage_variables: storage_variables::StorageVariables::default(),
            functions: FunctionTable::default(),
            module_level_vars: Vec::new(),
            real_variables: HashSet::new(),
            prelude_compiled: HashSet::new(),
            prelude_sources: HashMap::new(),
            in_prelude: false,
            imports: ImportState::default(),
            source_locations: SourceLocations::default(),
            resource_references: Vec::new(),
            stdlib: StdlibState::default(),
            experimental_resource_pack: options.experimental_resource_pack,
            unroll: UnrollState::default(),
            module_loop_variables: Default::default(),
            unrolled_sources: HashMap::new(),
            replay_sources: None,
            execute_condition_slots: Vec::new(),
            warnings: RefCell::new(Vec::new()),
            unwarned_shadows: Vec::new(),
            list_loops: Vec::new(),
            signalled_list_roots: Default::default(),
            in_module_code: false,
            loop_depth: 0,
            module_consts: HashSet::new(),
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
        self.storage_variables.restore(&mut self.data_pack);
        let result = self.transpile_program(program);
        // Tags merged by the file's declarations reach `json_resources`.
        let flushed = self.data_pack.flush_tags().map_err(TranspileError::from);
        let result = result.and(flushed);
        if result.is_ok() {
            self.storage_variables.lower(&mut self.data_pack);
        }
        result
    }

    fn transpile_program(&mut self, program: &Program) -> Result<(), TranspileError> {
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

        // Before the imports, which read the file's writes too.
        self.record_container_writes(program);
        self.record_potential_real_globals(program);
        self.hold_later_variables(program);
        self.process_imports(program)?;

        // PASS 1: Register all function signatures first (to handle forward references)
        self.register_program_function_metadata(program);

        // PASS 2: Process all statements (function bodies, etc.)
        for statement in &program.statements {
            self.process_top_level_statement(statement)?;
        }
        self.release_later_variables(program);

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

    /// Records the values `program` writes into lists and maps, so a read
    /// of an item or a field knows when its type can change. `transpile`
    /// records its own program and its imports. A build of several files
    /// records every file first, so a read sees the writes of a file
    /// compiled after it.
    pub fn record_container_writes(&mut self, program: &Program) {
        container_writes::collect_container_writes(
            &program.statements,
            &mut self.symbols.container_writes,
        );
        container_writes::collect_root_values(&program.statements, &mut self.symbols.root_values);
        // A loop over a list that calls a function, which may rebind the
        // list, needs every rebind of it to tell the loop.
        let modules: Vec<&str> = program
            .imports
            .iter()
            .map(|import| import.module.as_str())
            .collect();
        let is_module = |name: &str| modules.contains(&name);
        let mut loops = Vec::new();
        function_context::collect_all_for_loops(&program.statements, &mut loops);
        for for_loop in loops {
            let mut root = &for_loop.iter;
            while let Expression::Attribute(base, _) | Expression::Subscript(base, _) = root {
                root = base;
            }
            if let Expression::Identifier(name) = root {
                if statement_processors::calls_user_function(&for_loop.body, &is_module) {
                    self.signalled_list_roots.insert(name.clone());
                }
            }
        }
    }

    /// Lowers one statement of a file's module level: a declaration, or
    /// code that runs in `_cobble_init`.
    fn process_top_level_statement(&mut self, statement: &Statement) -> Result<(), TranspileError> {
        if self.is_module_code(statement) {
            self.process_module_code(statement)
        } else {
            self.process_statement(statement)
        }
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
        let statement_source = if self.unroll.depth > 0
            || self.current_context.safe_plain_text
            || self.in_prelude
            || self
                .unrolled_sources
                .contains_key(&function_context::statement_address(statement))
        {
            self.unrolled_sources
                .get(&function_context::statement_address(statement))
                .cloned()
                .flatten()
                .or_else(|| previous_statement_source.clone())
        } else {
            self.take_statement_source(statement)
                .or_else(|| previous_statement_source.clone())
        };
        self.current_statement_source = statement_source.clone();
        if let Some(source) = self
            .replay_sources
            .as_mut()
            .and_then(|sources| sources.get_mut(&function_context::statement_address(statement)))
        {
            *source = statement_source.clone();
        }
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

    /// A variable that holds a score and a storage copy, such as a copy of a
    /// parameter, becomes a plain number when a raw command writes its
    /// score: later reads use the score instead of the stale copy.
    fn forget_storage_copies(&mut self, commands: &[String]) {
        // Commands take the pack's objective when their function is added.
        let objective = vars_objective::SOURCE_OBJECTIVE;
        for command in commands {
            for holder in score_holders::written_holders(command, objective) {
                if self.symbols.scoreboard_variables.contains(holder)
                    && self.symbols.variable_storage_paths.remove(holder).is_some()
                {
                    self.symbols
                        .variable_types
                        .insert(holder.to_string(), CobbleType::Integer);
                }
            }
        }
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
        if let Statement::Execute(block) = statement {
            if let Some(statements) = self.execute_format_expressions(block)? {
                for statement in &statements {
                    self.process_statement_inner(statement)?;
                }
                return Ok(());
            }
        }
        if let Statement::MinecraftCommand(command) = statement {
            if let Some(statements) = self.command_format_expressions(command)? {
                for statement in &statements {
                    self.process_statement_inner(statement)?;
                }
                return Ok(());
            }
        }
        for expr in statement_expressions(statement) {
            storage_paths::StoragePathOps::check_subscript_keys(self, expr)?;
        }
        let targets: Vec<&Expression> = match statement {
            Statement::Assignment(assign) if !assign.assigns_name() => {
                assign.target_expr.iter().collect()
            }
            Statement::MultiAssign(multi) => multi.targets.iter().collect(),
            _ => Vec::new(),
        };
        for target in targets {
            storage_paths::StoragePathOps::check_assignment_target(self, target)?;
        }
        self.check_later_variables(statement)?;
        if self.in_module_code {
            for expr in statement_expressions(statement) {
                let assigned_name = match statement {
                    Statement::Assignment(assign) => {
                        assign.assigns_name() && assign.target_expr.as_ref() == Some(expr)
                    }
                    Statement::MultiAssign(multi) => {
                        multi.targets.contains(expr) && matches!(expr, Expression::Identifier(_))
                    }
                    _ => false,
                };
                if !assigned_name {
                    self.check_module_level_reads(expr)?;
                }
            }
        }
        if let Some(lowered) = self.desugar_statement(statement)? {
            for statement in &lowered {
                self.process_statement_inner(statement)?;
            }
            return Ok(());
        }
        self.rebind_signal_commands(statement)?;
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
            // Lowered to assignments above.
            Statement::MultiAssign(_) => {}
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
            Statement::Break | Statement::Continue => {
                if self.loop_depth == 0 {
                    return Err("break and continue can only be used inside a loop".into());
                }
                let context = self.current_return.as_ref().expect("loop control context");
                let flag = if matches!(statement, Statement::Break) {
                    3
                } else {
                    2
                };
                self.extend_current_function(vec![
                    format!("scoreboard players set {} temp 0", context.value_score),
                    format!("scoreboard players set {} temp {flag}", context.flag_score),
                    "return 0".into(),
                ]);
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
                if self.in_module_code {
                    for piece in placeholders::command_pieces(cmd) {
                        if let placeholders::Piece::Field { name, .. } = piece {
                            self.check_module_level_reads(&Expression::Identifier(
                                name.to_string(),
                            ))?;
                        }
                    }
                }
                // Strip leading slash and process variable/parameter substitution
                let clean_cmd = Self::strip_command_prefix(cmd);
                let source_location = if self.unroll.depth > 0 || self.in_prelude {
                    self.current_statement_source.clone()
                } else {
                    if self.current_context.safe_plain_text {
                        self.current_statement_source.clone()
                    } else {
                        self.take_raw_command_source(clean_cmd.trim())
                    }
                };
                if let Some(source) = self.replay_sources.as_mut().and_then(|sources| {
                    sources.get_mut(&function_context::statement_address(statement))
                }) {
                    *source = source_location.clone();
                }
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
                    safe_plain_text: self.current_context.safe_plain_text,
                    symbols: &self.symbols,
                };

                let say_macro = processor
                    .say_macro(&clean_cmd)
                    .map_err(|error| error.with_location_if_missing(source_location.clone()))?;
                if let Some(say_macro) = say_macro {
                    return self.emit_say_macro(say_macro, source_location);
                }
                let mut processed = processor
                    .text_copy_commands(&clean_cmd)
                    .map_err(|error| error.with_location_if_missing(source_location.clone()))?;
                let processed_cmd = processor
                    .process_command_string(&clean_cmd)
                    .map_err(|error| error.with_location_if_missing(source_location.clone()))?;

                if !processed.is_empty() {
                    self.data_pack.track_objective("temp");
                }
                self.forget_storage_copies(std::slice::from_ref(&processed_cmd));
                processed.extend(self.quoted_hover_commands(&clean_cmd, processed_cmd));
                if let Some(ref mut commands) = self.current_function {
                    for command in processed {
                        let command_index = commands.len();
                        commands.push(command);
                        if let Some(ref mut metadata) = self.current_function_metadata {
                            metadata.insert(
                                command_index,
                                GeneratedCommand::new(
                                    commands[command_index].clone(),
                                    source_location.clone(),
                                    GeneratedCommandKind::UserCommand,
                                ),
                            );
                        }
                    }
                } else {
                    return Err("Minecraft command outside of function context"
                        .to_string()
                        .into());
                }
            }
            Statement::Execute(exec_block) => {
                let start = self.current_function.as_ref().map_or(0, Vec::len);
                self.process_execute_block(exec_block)?;
                // A `store ... score name` header writes the score.
                let written: Vec<String> = self
                    .current_function
                    .as_ref()
                    .map(|commands| commands[start.min(commands.len())..].to_vec())
                    .unwrap_or_default();
                self.forget_storage_copies(&written);
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
                name == "addEventListener"
                    || name == "stdlib.addEventListener"
                    || name.starts_with("_cobble_timer_done")
                    || name.starts_with("_cobble_storage_get")
                    || name.starts_with("_cobble_command_")
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
        // A helper argument that is a parameter reads `$(name)`, so the
        // command runs as a macro line.
        let command = if !command.starts_with('$')
            && self
                .current_context
                .params
                .iter()
                .any(|param| command.contains(&format!("$({})", param)))
        {
            format!("${}", command)
        } else {
            command
        };
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
