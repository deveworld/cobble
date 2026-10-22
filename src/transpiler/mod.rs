// Module declarations
mod command_processor;
mod condition_translator;
mod data_pack;
mod error;
mod expression_evaluator;
mod resource_format;
mod source_map;
mod statement_processors;
mod storage_paths;
pub mod vars_objective;

// Public exports
pub use data_pack::{
    BuildManifest, BuildManifestGenerated, BuildManifestInput, BuildManifestResourceEntry,
    BuildManifestValidation, DataPack, GeneratedCommand, GeneratedCommandKind, SourceLocation,
    SourceMap, SourceMapEntry,
};
pub use error::{TranspileError, TranspileErrorKind};

use crate::ast::*;
use crate::stdlib::EventType;
use command_processor::CommandProcessor;
use condition_translator::{ConditionTranslator, TranslatedCondition};
use expression_evaluator::ExpressionEvaluator;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};
use storage_paths::{StoragePathOps, StorageValueOps};

/// Maximum exponent for compile-time power expansion (`x^n` emits `n - 1` multiplications).
pub(in crate::transpiler) const MAX_POWER_EXPONENT: i32 = 100;

/// Maximum import depth before a build is rejected. Shared source of truth
/// for the transpiler and the diagnostics import scanner
/// (`src/diagnostics/imports.rs` derives its copy from this constant).
pub(crate) const MAX_IMPORT_DEPTH: usize = 50;

/// Execute-condition keywords that start a raw Minecraft condition
/// (`entity @a[...]`, `score x temp matches 5`, `data storage ...`).
const RAW_CONDITION_KEYWORDS: [&str; 11] = [
    "score ",     // "score <target> <objective> matches/</>/>=/<=/= ..."
    "entity ",    // "entity <selector>"
    "block ",     // "block <pos> <block>"
    "blocks ",    // "blocks <start> <end> <dest> <mode>"
    "biome ",     // "biome <pos> <biome>"
    "dimension ", // "dimension <dimension>"
    "predicate ", // "predicate <predicate>"
    "data ",      // "data <source> <path>"
    "items ",     // "items <source> <slots> <item>"
    "function ",  // "function <function/tag>"
    "loaded ",    // "loaded <pos>"
];

/// `Some(true)` for a raw Minecraft condition, `Some(false)` for a Python
/// expression that merely starts with a keyword-named variable
/// (`score >= 10`), and `None` when the condition starts with no keyword.
/// Shared with the diagnostics layer so both agree on which `if` headers
/// carry raw syntax rather than variable references.
pub(crate) fn raw_minecraft_condition_verdict(condition: &str) -> Option<bool> {
    let rest = RAW_CONDITION_KEYWORDS
        .iter()
        .find_map(|keyword| condition.strip_prefix(keyword))?;
    let next_token = rest.split_whitespace().next().unwrap_or("");
    Some(!matches!(
        next_token,
        "==" | "!=" | ">=" | "<=" | ">" | "<" | "and" | "or" | "not" | "+" | "-" | "*" | "/" | "%"
    ))
}

/// Exact integer value of a compile-time number, if it has no fractional part.
fn exact_integer(value: f64) -> Option<i64> {
    (value.is_finite() && value.fract() == 0.0 && value.abs() < 9.0e15).then_some(value as i64)
}

/// `Mth.floorDiv`: integer division rounding toward negative infinity.
fn floor_div_i64(left: i64, right: i64) -> i64 {
    let quotient = left / right;
    if left % right != 0 && ((left < 0) != (right < 0)) {
        quotient - 1
    } else {
        quotient
    }
}

/// Compile-time `/` with Minecraft semantics. Integer operands use
/// `Mth.floorDiv`, exactly like scoreboard `/=` at runtime (`-7 / 2` is -4,
/// `1 / 3 * 3` is 0). Fractional constants can never exist at runtime, so
/// they keep real division and are truncated where the result is used.
/// Returns `None` for a zero divisor.
pub(in crate::transpiler) fn const_div(left: f64, right: f64) -> Option<f64> {
    if right == 0.0 {
        return None;
    }
    match (exact_integer(left), exact_integer(right)) {
        (Some(left), Some(right)) => Some(floor_div_i64(left, right) as f64),
        _ => Some(left / right),
    }
}

/// Compile-time `%` with Minecraft semantics: `Mth.positiveModulo`, a floor
/// modulus whose sign follows the divisor, like scoreboard `%=` at runtime
/// (`-7 % 3` is 2). Fractional operands use the same floor-modulus rule.
/// Returns `None` for a zero divisor.
pub(in crate::transpiler) fn const_mod(left: f64, right: f64) -> Option<f64> {
    if right == 0.0 {
        return None;
    }
    match (exact_integer(left), exact_integer(right)) {
        (Some(left), Some(right)) => Some((left - floor_div_i64(left, right) * right) as f64),
        _ => Some(left - (left / right).floor() * right),
    }
}

/// Context for tracking function parameters and scope
#[derive(Clone)]
struct FunctionContext {
    params: Vec<String>,
}

#[derive(Clone)]
struct ReturnContext {
    value_score: String,
    flag_score: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum FunctionReturnKind {
    None,
    Integer,
    NonInteger,
}

#[derive(Clone)]
pub(in crate::transpiler) enum ModuleLevelAssignment {
    Variable {
        target: String,
        value: Expression,
        source: Option<SourceLocation>,
        persist: bool,
    },
    StorageTarget {
        target_expr: Expression,
        value: Expression,
        source: Option<SourceLocation>,
    },
}

pub(in crate::transpiler) struct FunctionCapture {
    pub(in crate::transpiler) commands: Vec<String>,
    pub(in crate::transpiler) metadata: HashMap<usize, GeneratedCommand>,
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) struct DynamicStoragePathArg {
    pub(in crate::transpiler) placeholder: String,
    pub(in crate::transpiler) expr: Expression,
    pub(in crate::transpiler) kind: DynamicStoragePathArgKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::transpiler) enum DynamicStoragePathArgKind {
    Index,
    Key,
}

#[derive(Debug, Clone)]
pub(in crate::transpiler) enum StoragePathPlan {
    Static(String),
    Dynamic {
        template: String,
        args: Vec<DynamicStoragePathArg>,
    },
}

impl StoragePathPlan {
    pub(in crate::transpiler) fn static_path(path: impl Into<String>) -> Self {
        Self::Static(path.into())
    }

    pub(in crate::transpiler) fn as_static(&self) -> Option<&str> {
        match self {
            Self::Static(path) => Some(path),
            Self::Dynamic { .. } => None,
        }
    }

    pub(in crate::transpiler) fn template(&self) -> &str {
        match self {
            Self::Static(path) => path,
            Self::Dynamic { template, .. } => template,
        }
    }

    pub(in crate::transpiler) fn dynamic_args(&self) -> &[DynamicStoragePathArg] {
        match self {
            Self::Static(_) => &[],
            Self::Dynamic { args, .. } => args,
        }
    }

    pub(in crate::transpiler) fn append_field(self, field: &str) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}.{}", path, field)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}.{}", template, field),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_static_index(self, index: i32) -> Self {
        match self {
            Self::Static(path) => Self::Static(format!("{}[{}]", path, index)),
            Self::Dynamic { template, args } => Self::Dynamic {
                template: format!("{}[{}]", template, index),
                args,
            },
        }
    }

    pub(in crate::transpiler) fn append_dynamic_index(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("idx{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Index,
        });
        Self::Dynamic {
            template: format!("{}[$({})]", template, placeholder),
            args,
        }
    }

    pub(in crate::transpiler) fn append_dynamic_key(self, expr: Expression) -> Self {
        let (template, mut args) = match self {
            Self::Static(path) => (path, Vec::new()),
            Self::Dynamic { template, args } => (template, args),
        };
        let placeholder = format!("key{}", args.len());
        args.push(DynamicStoragePathArg {
            placeholder: placeholder.clone(),
            expr,
            kind: DynamicStoragePathArgKind::Key,
        });
        Self::Dynamic {
            template: format!("{}.$({})", template, placeholder),
            args,
        }
    }
}

struct SavedFunctionCapture {
    function: Option<Vec<String>>,
    metadata: Option<HashMap<usize, GeneratedCommand>>,
}

struct SavedCompilerState {
    current_context: FunctionContext,
    variables: HashMap<String, Expression>,
    variable_objectives: HashMap<String, String>,
    scoreboard_variables: HashSet<String>,
    global_variables: HashSet<String>,
    compile_time_constants: HashMap<String, f64>,
    selector_aliases: HashMap<String, String>,
    entity_templates: HashMap<String, EntityDef>,
    variable_storage_paths: HashMap<String, String>,
    variable_types: HashMap<String, crate::ast::CobbleType>,
}

impl FunctionContext {
    fn new() -> Self {
        Self { params: Vec::new() }
    }

    fn with_params(params: Vec<String>) -> Self {
        Self { params }
    }

    fn is_param(&self, name: &str) -> bool {
        self.params.iter().any(|p| p == name)
    }
}

impl FunctionCapture {
    pub(in crate::transpiler) fn requires_macro_context(&self) -> bool {
        self.commands
            .iter()
            .any(|command| command.trim_start().starts_with('$'))
    }
}

fn validate_text_color(color: &str) -> Result<(), TranspileError> {
    const NAMED_COLORS: &[&str] = &[
        "black",
        "dark_blue",
        "dark_green",
        "dark_aqua",
        "dark_red",
        "dark_purple",
        "gold",
        "gray",
        "dark_gray",
        "blue",
        "green",
        "aqua",
        "red",
        "light_purple",
        "yellow",
        "white",
    ];

    let valid_hex = color.len() == 7
        && color.starts_with('#')
        && color.chars().skip(1).all(|c| c.is_ascii_hexdigit());
    if NAMED_COLORS.contains(&color) || valid_hex {
        Ok(())
    } else {
        Err(format!(
            "Invalid text color '{}': use a Minecraft named color or #RRGGBB",
            color
        )
        .into())
    }
}

/// Stdlib module names that can be imported via `from stdlib import <name>`.
///
/// `import stdlib` (without `from`) activates every module for backward
/// compatibility with 0.7.x. See `docs/src/stdlib/modules.md`. The canonical
/// table lives in `crate::stdlib::STDLIB_MODULES`.
pub(crate) use crate::stdlib::STDLIB_MODULES as ALL_STDLIB_MODULES;

pub struct Transpiler {
    pub data_pack: DataPack,
    current_function: Option<Vec<String>>,
    current_function_metadata: Option<HashMap<usize, GeneratedCommand>>,
    current_context: FunctionContext,
    current_return: Option<ReturnContext>,
    variables: HashMap<String, Expression>,
    temp_counter: u32,
    variable_objectives: HashMap<String, String>, // Track which objective each variable uses
    scoreboard_variables: HashSet<String>, // Track variables backed by scoreboard (not constants)
    function_params: HashMap<String, Vec<String>>, // Track function parameter names
    function_returns: HashMap<String, FunctionReturnKind>, // Track value-call return eligibility
    qualified_function_aliases: HashMap<String, String>, // Track import-module calls (module.func -> func)
    global_variables: HashSet<String>, // Track global variables declared in current function
    module_level_vars: Vec<ModuleLevelAssignment>, // Store module-level assignments (Vec for ordering)
    compile_time_constants: HashMap<String, f64>,  // Store compile-time constant values
    selector_aliases: HashMap<String, String>,     // Store selector definitions (@Name -> @a[...])
    entity_templates: HashMap<String, EntityDef>,  // Store entity templates
    variable_storage_paths: HashMap<String, String>, // Track storage path for variables (e.g., "list" -> "vars.list")
    imported_files: HashSet<PathBuf>, // Track imported files to prevent circular dependencies
    import_stack: Vec<PathBuf>,       // Track current import chain for circular detection
    current_file_path: Option<PathBuf>,
    current_file_dir: PathBuf, // Current file's directory for resolving relative imports
    raw_command_locations: HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    statement_locations: HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    current_statement_source: Option<SourceLocation>,
    variable_types: HashMap<String, crate::ast::CobbleType>, // Track type of each variable for type checking
    /// Stdlib version declared in `cobble.toml`. `1` keeps 0.7.x behavior
    /// (all modules always active). `2` (default) requires per-module opt-in
    /// via `from stdlib import ...`.
    stdlib_version: u8,
    /// Stdlib modules activated by imports. When `stdlib_version == 1` or
    /// `import stdlib` is used, every module in `ALL_STDLIB_MODULES` is
    /// inserted here.
    active_stdlib_modules: HashSet<String>,
    /// Whether `[stdlib] version = 1` deprecation warning was already emitted
    /// for this build. The warning is emitted once per build.
    stdlib_v1_warning_emitted: bool,
    /// Whether experimental resource-pack output is enabled for this build.
    experimental_resource_pack: bool,
    /// Current nesting depth while compile-time `for` unrolling is active.
    unroll_depth: usize,
    /// Product of literal iteration counts across the active unroll stack.
    unroll_iteration_factor: usize,
    /// Generated command count at the start of the outermost unroll.
    unroll_command_baseline: Option<usize>,
    /// Stack of per-site condition holder slots for nested `execute` blocks.
    /// One frame per active `process_execute_block`; the frame counts the
    /// condition holders scoped to `@s` so their objectives can be pooled by
    /// (nesting depth, slot) instead of allocated per call site.
    execute_condition_slots: Vec<usize>,
    /// Name of the user function whose body is currently being lowered.
    /// Used to reject recursive calls at the call site.
    current_function_name: Option<String>,
    /// Caller -> callees edges for every lowered user-function call, used to
    /// reject mutually recursive call cycles at the end of `transpile()`.
    /// BTree containers keep the reported cycle deterministic.
    function_call_edges: BTreeMap<String, BTreeSet<String>>,
}

impl Transpiler {
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self {
            data_pack: DataPack::new(namespace, output_dir),
            current_function: None,
            current_function_metadata: None,
            current_context: FunctionContext::new(),
            current_return: None,
            variables: HashMap::new(),
            temp_counter: 0,
            variable_objectives: HashMap::new(),
            scoreboard_variables: HashSet::new(),
            function_params: HashMap::new(),
            function_returns: HashMap::new(),
            qualified_function_aliases: HashMap::new(),
            global_variables: HashSet::new(),
            module_level_vars: Vec::new(),
            compile_time_constants: HashMap::new(),
            selector_aliases: HashMap::new(),
            entity_templates: HashMap::new(),
            variable_storage_paths: HashMap::new(),
            imported_files: HashSet::new(),
            import_stack: Vec::new(),
            current_file_path: None,
            current_file_dir: PathBuf::from("."),
            raw_command_locations: HashMap::new(),
            statement_locations: HashMap::new(),
            current_statement_source: None,
            variable_types: HashMap::new(),
            stdlib_version: 2,
            active_stdlib_modules: HashSet::new(),
            stdlib_v1_warning_emitted: false,
            experimental_resource_pack: false,
            unroll_depth: 0,
            unroll_iteration_factor: 1,
            unroll_command_baseline: None,
            execute_condition_slots: Vec::new(),
            current_function_name: None,
            function_call_edges: BTreeMap::new(),
        }
    }

    /// Infer the type of an expression
    /// Evaluate a constant expression to a number if possible
    fn try_eval_const(&self, expr: &Expression) -> Option<f64> {
        match expr {
            Expression::Number(n) => Some(*n),
            Expression::Identifier(name) => self.compile_time_constants.get(name).copied(),
            Expression::Boolean(b) => Some(if *b { 1.0 } else { 0.0 }),
            Expression::Binary(left, op, right) => {
                let left_val = self.try_eval_const(left)?;
                let right_val = self.try_eval_const(right)?;

                use crate::ast::BinaryOp;
                match op {
                    BinaryOp::Add => Some(left_val + right_val),
                    BinaryOp::Sub => Some(left_val - right_val),
                    BinaryOp::Mul => Some(left_val * right_val),
                    BinaryOp::Div => const_div(left_val, right_val),
                    BinaryOp::Mod => const_mod(left_val, right_val),
                    BinaryOp::Pow => {
                        let base = left_val as i32;
                        let exp = right_val as i32;
                        if exp < 0 {
                            None
                        } else {
                            // Use checked_pow to detect overflow
                            match base.checked_pow(exp as u32) {
                                Some(result) => Some(result as f64),
                                None => {
                                    eprintln!("Warning: Power operation {}^{} would overflow i32, clamping to i32::MAX", base, exp);
                                    Some(i32::MAX as f64)
                                }
                            }
                        }
                    }
                    BinaryOp::Eq => Some((left_val == right_val) as i32 as f64),
                    BinaryOp::NotEq => Some((left_val != right_val) as i32 as f64),
                    BinaryOp::Lt => Some((left_val < right_val) as i32 as f64),
                    BinaryOp::LtEq => Some((left_val <= right_val) as i32 as f64),
                    BinaryOp::Gt => Some((left_val > right_val) as i32 as f64),
                    BinaryOp::GtEq => Some((left_val >= right_val) as i32 as f64),
                    BinaryOp::And => Some(((left_val != 0.0) && (right_val != 0.0)) as i32 as f64),
                    BinaryOp::Or => Some(((left_val != 0.0) || (right_val != 0.0)) as i32 as f64),
                }
            }
            _ => None,
        }
    }

    fn infer_type(&self, expr: &Expression) -> crate::ast::CobbleType {
        use crate::ast::CobbleType;

        match expr {
            Expression::Number(_) => CobbleType::Integer,
            Expression::Boolean(_) => CobbleType::Boolean,
            Expression::String(_) => CobbleType::String,
            Expression::Array(_) => CobbleType::List,
            Expression::Map(_) => CobbleType::Map,
            Expression::Identifier(name) => {
                // Look up variable type
                self.variable_types
                    .get(name)
                    .cloned()
                    .unwrap_or(CobbleType::Unknown)
            }
            Expression::Binary(left, op, right) => {
                use crate::ast::BinaryOp;

                let _left_type = self.infer_type(left);
                let _right_type = self.infer_type(right);

                match op {
                    // Arithmetic operations return integers
                    BinaryOp::Add
                    | BinaryOp::Sub
                    | BinaryOp::Mul
                    | BinaryOp::Div
                    | BinaryOp::Mod
                    | BinaryOp::Pow => CobbleType::Integer,

                    // Comparison operations return booleans
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq => CobbleType::Boolean,

                    // Logical operations return booleans
                    BinaryOp::And | BinaryOp::Or => CobbleType::Boolean,
                }
            }
            Expression::Unary(op, _) => {
                use crate::ast::UnaryOp;

                match op {
                    UnaryOp::Not => CobbleType::Boolean,
                    UnaryOp::Neg | UnaryOp::Pos => CobbleType::Integer,
                }
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(name) if name == "len") => {
                CobbleType::Integer
            }
            Expression::Call(func, _)
                if self
                    .resolve_user_function_call_name(func)
                    .is_some_and(|name| {
                        matches!(
                            self.function_returns.get(name),
                            Some(FunctionReturnKind::Integer)
                        )
                    }) =>
            {
                CobbleType::Integer
            }
            Expression::Call(func, _) if matches!(&**func, Expression::Attribute(obj, _) if matches!(&**obj, Expression::Identifier(module) if module == "math")) => {
                CobbleType::Integer
            }
            Expression::Call(_, _) => CobbleType::Unknown,
            _ => CobbleType::Unknown,
        }
    }

    fn analyze_function_return_kind(body: &[Statement]) -> FunctionReturnKind {
        body.iter()
            .fold(FunctionReturnKind::None, |acc, statement| {
                Self::merge_return_kind(acc, Self::statement_return_kind(statement))
            })
    }

    fn statement_return_kind(statement: &Statement) -> FunctionReturnKind {
        match statement {
            Statement::Return(expr) => expr
                .as_ref()
                .map(Self::expression_return_kind)
                .unwrap_or(FunctionReturnKind::Integer),
            Statement::If(if_stmt) => {
                let mut kind = Self::analyze_function_return_kind(&if_stmt.then_branch);
                for (_, branch) in &if_stmt.elif_branches {
                    kind =
                        Self::merge_return_kind(kind, Self::analyze_function_return_kind(branch));
                }
                if let Some(branch) = &if_stmt.else_branch {
                    kind =
                        Self::merge_return_kind(kind, Self::analyze_function_return_kind(branch));
                }
                kind
            }
            Statement::For(for_loop) => Self::analyze_function_return_kind(&for_loop.body),
            Statement::While(while_loop) => Self::analyze_function_return_kind(&while_loop.body),
            Statement::Match(match_stmt) => {
                match_stmt
                    .cases
                    .iter()
                    .fold(FunctionReturnKind::None, |acc, case| {
                        Self::merge_return_kind(acc, Self::analyze_function_return_kind(&case.body))
                    })
            }
            Statement::Execute(exec_block) => Self::analyze_function_return_kind(&exec_block.body),
            _ => FunctionReturnKind::None,
        }
    }

    fn expression_return_kind(expr: &Expression) -> FunctionReturnKind {
        match expr {
            Expression::String(_)
            | Expression::Array(_)
            | Expression::Map(_)
            | Expression::None => FunctionReturnKind::NonInteger,
            _ => FunctionReturnKind::Integer,
        }
    }

    fn merge_return_kind(
        left: FunctionReturnKind,
        right: FunctionReturnKind,
    ) -> FunctionReturnKind {
        match (left, right) {
            (FunctionReturnKind::NonInteger, _) | (_, FunctionReturnKind::NonInteger) => {
                FunctionReturnKind::NonInteger
            }
            (FunctionReturnKind::Integer, _) | (_, FunctionReturnKind::Integer) => {
                FunctionReturnKind::Integer
            }
            _ => FunctionReturnKind::None,
        }
    }

    /// Check if a type assignment is valid
    fn check_type_assignment(
        &self,
        var_name: &str,
        new_type: &crate::ast::CobbleType,
    ) -> Result<(), TranspileError> {
        use crate::ast::CobbleType;

        if let Some(existing_type) = self.variable_types.get(var_name) {
            if existing_type != new_type
                && *existing_type != CobbleType::Unknown
                && *new_type != CobbleType::Unknown
            {
                return Err(format!(
                    "Type mismatch for variable '{}'.\n\n\
                    Variable was previously defined as type: {}\n\
                    Cannot reassign to type: {}\n\n\
                    In Cobble, all variables have immutable types.\n\
                    Once a variable is assigned a value, its type cannot change.\n\n\
                    Solutions:\n\
                    1. Use a different variable name for the different type\n\
                    2. Ensure all assignments to '{}' use the same type",
                    var_name,
                    existing_type.name(),
                    new_type.name(),
                    var_name
                )
                .into());
            }
        }

        Ok(())
    }

    pub fn set_description(&mut self, desc: String) {
        self.data_pack.set_description(desc);
    }

    pub fn set_pack_format(&mut self, format: crate::pack_format::PackFormat) {
        self.data_pack.set_pack_format(format);
    }

    fn is_current_macro_param(&self, name: &str) -> bool {
        self.current_context.is_param(name)
    }

    pub fn set_build_input(&mut self, input: BuildManifestInput) {
        self.data_pack.set_build_input(input);
    }

    pub fn set_project_identity(&mut self, project_root: String, project_id: String) {
        self.data_pack
            .set_project_identity(project_root, project_id);
    }

    pub fn set_source_display_root(&mut self, root: PathBuf) {
        self.data_pack.set_source_display_root(root);
    }

    pub fn set_validation_summary(&mut self, validation: Option<BuildManifestValidation>) {
        self.data_pack.set_validation_summary(validation);
    }

    /// Enable or disable experimental resource-pack output.
    pub fn set_experimental_resource_pack(&mut self, enabled: bool) {
        self.experimental_resource_pack = enabled;
        self.data_pack
            .experimental_features
            .retain(|feature| feature != "resource_pack");
        if enabled {
            self.data_pack
                .experimental_features
                .push("resource_pack".to_string());
        }
        self.data_pack.experimental_features.sort();
    }

    /// Set the stdlib version from `cobble.toml` (`[stdlib] version`).
    ///
    /// `version = 1` keeps 0.7.x behavior where all modules are always
    /// active. `version = 2` (default) requires per-module opt-in.
    pub fn set_stdlib_version(&mut self, version: u8) {
        self.stdlib_version = version;
        if version == 1 {
            self.active_stdlib_modules.clear();
            for module in ALL_STDLIB_MODULES {
                self.active_stdlib_modules.insert((*module).to_string());
            }
        }
        self.data_pack.stdlib_version = Some(version);
    }

    /// Returns the active stdlib version.
    pub fn stdlib_version(&self) -> u8 {
        self.stdlib_version
    }

    /// Returns the set of modules activated by stdlib imports.
    pub fn active_stdlib_modules(&self) -> &HashSet<String> {
        &self.active_stdlib_modules
    }

    /// Sync the active stdlib module set into the data pack so it is
    /// recorded in the build manifest. Called after imports are processed
    /// and before the data pack is written.
    pub fn sync_stdlib_modules_to_manifest(&mut self) {
        let mut modules: Vec<String> = self.active_stdlib_modules.iter().cloned().collect();
        modules.sort();
        self.data_pack.active_stdlib_modules = modules;
    }

    pub fn transpile(&mut self, program: &Program) -> Result<(), TranspileError> {
        // Emit the stdlib v1 deprecation warning once per build.
        self.maybe_emit_stdlib_v1_warning();

        // When building multiple files, clear the import stack for each new file
        // The current file should already be in the stack from set_current_file()
        // We only keep the current file to properly detect circular imports within its imports
        if self.import_stack.len() > 1 {
            // Keep only the last entry (the current file being transpiled)
            if let Some(current_file) = self.import_stack.last() {
                let current = current_file.clone();
                self.import_stack.clear();
                self.import_stack.push(current);
            }
        }

        // Process imports
        for import in &program.imports {
            self.process_import(import)?;
        }

        // PASS 1: Register all function signatures first (to handle forward references)
        self.register_program_function_metadata(program);

        // PASS 2: Process all statements (function bodies, etc.)
        for statement in &program.statements {
            self.process_statement(statement)?;
        }

        // Direct self-calls error at the call site; this rejects mutual
        // recursion (a -> b -> a) across everything lowered so far.
        self.check_recursive_call_cycles()?;

        let module_init = if self.module_level_vars.is_empty() {
            None
        } else {
            Some(self.build_module_init_commands()?)
        };

        // Ensure objectives are initialized after module init commands have had
        // a chance to register every objective they need.
        self.data_pack.ensure_init_function();

        // Then initialize module-level variables in the init function.
        if let Some((init_commands, init_metadata)) = module_init {
            // Add these commands after gamerule and objective creation
            if !init_commands.is_empty() {
                if let Some(existing_init) = self.data_pack.functions.get("_cobble_init") {
                    // Find the position after gamerule and all objectives
                    let setup_end = self.data_pack.runtime_setup_end_index(existing_init);

                    self.data_pack.insert_function_commands_with_metadata(
                        "_cobble_init",
                        setup_end,
                        &init_commands,
                        init_metadata,
                    );
                } else {
                    // Find the load handler function
                    let load_handlers = self.data_pack.stdlib.get_event_handlers(&EventType::Load);
                    if let Some(handler_name) = load_handlers.first() {
                        if let Some(handler_func) = self.data_pack.functions.get(handler_name) {
                            // Find the position after gamerule and all objectives
                            let setup_end = self.data_pack.runtime_setup_end_index(handler_func);

                            self.data_pack.insert_function_commands_with_metadata(
                                handler_name,
                                setup_end,
                                &init_commands,
                                init_metadata,
                            );
                        }
                    } else {
                        // No _cobble_init and no load handler - create _cobble_init
                        self.data_pack.add_function_with_metadata(
                            "_cobble_init".to_string(),
                            init_commands,
                            init_metadata,
                        );
                        self.data_pack
                            .stdlib
                            .add_event_listener(EventType::Load, "_cobble_init".to_string());
                    }
                }
            }

            self.module_level_vars.clear();
        }

        // Sync active stdlib modules into the data pack for manifest output.
        self.sync_stdlib_modules_to_manifest();

        Ok(())
    }

    fn build_module_init_commands(
        &mut self,
    ) -> Result<(Vec<String>, HashMap<usize, GeneratedCommand>), TranspileError> {
        let mut init_commands = Vec::new();
        let mut init_metadata = HashMap::new();

        for assignment in self.module_level_vars.clone() {
            match assignment {
                ModuleLevelAssignment::Variable {
                    target,
                    value,
                    source,
                    persist,
                } => {
                    let previous_source = self.current_statement_source.clone();
                    self.current_statement_source = source.clone();
                    let generated = self.module_variable_init_commands(&target, &value);
                    self.current_statement_source = previous_source;
                    let mut generated = generated?;
                    if persist {
                        generated = self.guard_persistent_init(&target, generated, &source);
                    }
                    Self::append_module_init_commands(
                        &mut init_commands,
                        &mut init_metadata,
                        generated,
                        source,
                    );
                }
                ModuleLevelAssignment::StorageTarget {
                    target_expr,
                    value,
                    source,
                } => {
                    let previous_source = self.current_statement_source.clone();
                    self.current_statement_source = source.clone();
                    let generated = self.storage_target_assignment_commands(&target_expr, &value);
                    self.current_statement_source = previous_source;
                    let generated = generated?;
                    Self::append_module_init_commands(
                        &mut init_commands,
                        &mut init_metadata,
                        generated,
                        source,
                    );
                }
            }
        }

        Ok((init_commands, init_metadata))
    }

    /// Wraps a `persist` variable's initializer so it only runs while the
    /// value is unset. A multi-command initializer moves into a helper
    /// function behind the same guard.
    fn guard_persistent_init(
        &mut self,
        name: &str,
        commands: Vec<String>,
        source: &Option<SourceLocation>,
    ) -> Vec<String> {
        let namespace = self.data_pack.namespace.clone();
        let guard = match self.variable_storage_paths.get(name) {
            Some(path) => format!("unless data storage {namespace}:global {path}"),
            None => format!("unless score {name} temp matches -2147483648.."),
        };
        if let [command] = commands.as_slice() {
            return vec![format!("execute {guard} run {command}")];
        }

        let helper = format!("_cobble_persist_{}", self.next_temp_id());
        let metadata = commands
            .iter()
            .enumerate()
            .map(|(index, command)| {
                (
                    index,
                    GeneratedCommand::new(
                        command.clone(),
                        source.clone(),
                        GeneratedCommandKind::ControlFlow,
                    ),
                )
            })
            .collect();
        self.data_pack
            .add_function_with_metadata(helper.clone(), commands, metadata);
        vec![format!("execute {guard} run function {namespace}:{helper}")]
    }

    /// Whether `value` can initialize a `persist` variable: a compile-time
    /// number or boolean, or a string, list, or map literal of constants.
    fn is_persist_initializer(&self, value: &Expression) -> bool {
        if self.try_eval_const(value).is_some() {
            return true;
        }
        match value {
            Expression::String(_) => true,
            Expression::Array(items) => items.iter().all(|item| self.is_persist_initializer(item)),
            Expression::Map(entries) => entries
                .iter()
                .all(|(_, item)| self.is_persist_initializer(item)),
            _ => false,
        }
    }

    fn append_module_init_commands(
        init_commands: &mut Vec<String>,
        init_metadata: &mut HashMap<usize, GeneratedCommand>,
        commands: Vec<String>,
        source: Option<SourceLocation>,
    ) {
        for command in commands {
            let index = init_commands.len();
            init_metadata.insert(
                index,
                GeneratedCommand::new(
                    command.clone(),
                    source.clone(),
                    GeneratedCommandKind::ControlFlow,
                ),
            );
            init_commands.push(command);
        }
    }

    fn module_variable_init_commands(
        &mut self,
        var_name: &str,
        value: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(const_value) = self.try_eval_const(value) {
            let truncated = const_value as i32;

            if const_value > i32::MAX as f64 || const_value < i32::MIN as f64 {
                eprintln!(
                    "⚠️  Warning: Module-level value {} for variable '{}' exceeds Minecraft scoreboard range.\n\
                    Scoreboard range: {} to {}\n\
                    Value will be clamped to: {}",
                    const_value,
                    var_name,
                    i32::MIN,
                    i32::MAX,
                    if const_value > i32::MAX as f64 {
                        i32::MAX
                    } else {
                        i32::MIN
                    }
                );
            }

            if const_value.fract() != 0.0 {
                eprintln!(
                    "⚠️  Warning: Module-level value {} for variable '{}' will lose precision.\n\
                    Scoreboard only supports integers. Fractional part will be truncated to: {}",
                    const_value, var_name, truncated
                );
            }

            return Ok(vec![format!(
                "scoreboard players set {} temp {}",
                var_name, truncated
            )]);
        }

        match value {
            Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                let storage_path = format!("vars.{}", var_name);
                self.variable_storage_paths
                    .insert(var_name.to_string(), storage_path.clone());
                self.storage_value_commands(&storage_path, value)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
            _ => {
                self.data_pack.track_objective("temp");
                self.variable_objectives
                    .insert(var_name.to_string(), "temp".to_string());
                self.scoreboard_variables.insert(var_name.to_string());
                self.evaluate_expression_to_target(value, var_name)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
        }
    }

    fn process_import(&mut self, import: &Import) -> Result<(), TranspileError> {
        // Handle stdlib imports
        if import.module == "stdlib" {
            if import.items.is_empty() {
                // `import stdlib` activates every module (0.7.x compatibility).
                self.activate_all_stdlib_modules();
            } else {
                // `from stdlib import text, score, ...` activates listed modules.
                for item in &import.items {
                    self.activate_stdlib_module(item)?;
                }
            }
            return Ok(());
        }

        // Security: Validate module name to prevent path traversal
        if import.module.contains("..")
            || import.module.contains('/')
            || import.module.contains('\\')
        {
            return Err(format!(
                "Invalid module name '{}': Cannot contain path separators or '..'\n\
                Module names must be simple identifiers (e.g., 'utils', 'helpers')",
                import.module
            )
            .into());
        }

        // Security: Limit import depth to prevent stack overflow
        if self.import_stack.len() >= MAX_IMPORT_DEPTH {
            return Err(format!(
                "Maximum import depth exceeded ({}).\n\
                This usually indicates a circular or excessively deep import chain.",
                MAX_IMPORT_DEPTH
            )
            .into());
        }

        // Handle file imports
        let import_path = self.current_file_dir.join(format!("{}.cbl", import.module));

        // Canonicalize path for accurate comparison
        let canonical_path = import_path.canonicalize().unwrap_or(import_path.clone());

        if self.import_stack.contains(&canonical_path) {
            return Err(format!(
                "Circular import detected: {}\n\
                Import cycles are not supported because they make initialization order ambiguous.",
                self.format_import_chain(&canonical_path)
            )
            .into());
        }

        // Check if file exists
        if !canonical_path.exists() && !import_path.exists() {
            return Err(format!(
                "Cannot import '{}' from '{}': file '{}' not found",
                import.module,
                self.current_importer_display(),
                import_path.display()
            )
            .into());
        }

        // A later `import module` may follow an earlier `from module import ...`.
        // Re-read just enough metadata to enable qualified `module.func()` calls
        // without processing the imported statements twice.
        if self.imported_files.contains(&canonical_path) {
            if import.items.is_empty() {
                let source = std::fs::read_to_string(&import_path).map_err(|e| {
                    format!(
                        "Failed to read import file '{}': {}",
                        import_path.display(),
                        e
                    )
                })?;
                let located =
                    crate::parser::parse_located_with_diagnostics(&source).map_err(|errors| {
                        format!(
                            "Parse failed for '{}': {}",
                            import.module,
                            errors
                                .iter()
                                .map(|error| format!(
                                    "{}:{} {}",
                                    error.span.line, error.span.column, error.message
                                ))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    })?;
                self.register_qualified_function_aliases(&import.module, &located.program);
            }
            return Ok(()); // Already imported, skip to avoid duplicate generated output
        }

        // Add to import stack and mark as imported
        self.import_stack.push(canonical_path.clone());
        self.imported_files.insert(canonical_path.clone());

        // Read the file
        let source = std::fs::read_to_string(&import_path).map_err(|e| {
            format!(
                "Failed to read import file '{}': {}",
                import_path.display(),
                e
            )
        })?;

        // Parse the imported file
        let located = crate::parser::parse_located_with_diagnostics(&source).map_err(|errors| {
            format!(
                "Parse failed for '{}': {}",
                import.module,
                errors
                    .iter()
                    .map(|error| {
                        format!(
                            "{}:{} {}",
                            error.span.line, error.span.column, error.message
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
        let crate::parser::LocatedProgram { program, spans } = located;

        // Save current file dir
        let saved_dir = self.current_file_dir.clone();
        let saved_file = self.current_file_path.clone();

        // Set directory for nested imports
        if let Some(parent) = import_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }
        self.current_file_path = Some(canonical_path.clone());
        self.register_source_locations_from_spans(&import_path, &source, &spans);

        // Process nested imports first
        for import in &program.imports {
            self.process_import(import)?;
        }

        // Then process imported program statements
        self.register_program_function_metadata(&program);
        if import.items.is_empty() {
            self.register_qualified_function_aliases(&import.module, &program);
        }
        for statement in &program.statements {
            self.process_statement(statement)?;
        }

        // Restore previous directory
        self.current_file_dir = saved_dir;
        self.current_file_path = saved_file;

        // Pop from import stack after processing
        self.import_stack.pop();

        Ok(())
    }

    fn current_importer_display(&self) -> String {
        self.import_stack
            .last()
            .map(|path| path.display().to_string())
            .unwrap_or_else(|| self.current_file_dir.display().to_string())
    }

    fn format_import_chain(&self, next_path: &Path) -> String {
        let mut chain: Vec<String> = self
            .import_stack
            .iter()
            .map(|path| path.display().to_string())
            .collect();
        chain.push(next_path.display().to_string());
        chain.join(" -> ")
    }

    fn register_program_function_metadata(&mut self, program: &Program) {
        for statement in &program.statements {
            if let Statement::FunctionDef(func) = statement {
                let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
                self.function_params.insert(func.name.clone(), param_names);
                self.function_returns.insert(
                    func.name.clone(),
                    Self::analyze_function_return_kind(&func.body),
                );
            }
        }
    }

    fn register_qualified_function_aliases(&mut self, module: &str, program: &Program) {
        for statement in &program.statements {
            if let Statement::FunctionDef(func) = statement {
                self.qualified_function_aliases
                    .insert(format!("{}.{}", module, func.name), func.name.clone());
            }
        }
    }

    fn resolve_user_function_call_name<'a>(&'a self, func: &'a Expression) -> Option<&'a str> {
        match func {
            Expression::Identifier(name) if self.function_params.contains_key(name) => {
                Some(name.as_str())
            }
            Expression::Attribute(obj, method) => {
                let module_name = Self::expression_path(obj)?;
                let key = format!("{}.{}", module_name, method);
                self.qualified_function_aliases
                    .get(&key)
                    .map(String::as_str)
            }
            _ => None,
        }
    }

    /// Activate every stdlib module. Used by `import stdlib` and by
    /// `[stdlib] version = 1`.
    fn activate_all_stdlib_modules(&mut self) {
        for module in ALL_STDLIB_MODULES {
            self.active_stdlib_modules.insert((*module).to_string());
        }
    }

    /// Activate a single stdlib module by name. Returns an error if the
    /// module name is not recognized.
    fn activate_stdlib_module(&mut self, name: &str) -> Result<(), TranspileError> {
        if !ALL_STDLIB_MODULES.contains(&name) {
            return Err(TranspileError::new(
                TranspileErrorKind::UnknownStdlibModule,
                format!(
                    "Unknown stdlib module '{}'. Available modules: {}",
                    name,
                    ALL_STDLIB_MODULES.join(", ")
                ),
            ));
        }
        self.active_stdlib_modules.insert(name.to_string());
        Ok(())
    }

    /// Check that a stdlib module is active before calling one of its
    /// helpers. Returns an error in `version = 2` when the module was not
    /// imported. In `version = 1` every module is always active so this
    /// always succeeds.
    fn require_stdlib_module(&self, module: &str) -> Result<(), TranspileError> {
        if self.active_stdlib_modules.contains(module) {
            Ok(())
        } else {
            Err(TranspileError::new(
                TranspileErrorKind::MissingStdlibModule,
                format!(
                    "module '{module}' not imported.\n\
                 Add `from stdlib import {module}` or use `import stdlib` to enable all modules.\n\
                 See https://deveworld.github.io/cobble/docs/stdlib/modules.html for the import and versioning model."
                ),
            ))
        }
    }

    /// Emit the `[stdlib] version = 1` deprecation warning once per build.
    fn maybe_emit_stdlib_v1_warning(&mut self) {
        if self.stdlib_version == 1 && !self.stdlib_v1_warning_emitted {
            self.stdlib_v1_warning_emitted = true;
            eprintln!(
                "warning: [stdlib] version = 1 is deprecated.\n\
                 Migrate to version = 2 and use `from stdlib import ...` for per-module opt-in.\n\
                 See https://deveworld.github.io/cobble/docs/stdlib/modules.html for how to migrate."
            );
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
        let statement_source = self
            .take_statement_source(statement)
            .or_else(|| previous_statement_source.clone());
        self.current_statement_source = statement_source.clone();

        let start_index = self.current_function.as_ref().map(Vec::len);
        let default_kind = Self::default_generated_kind_for_statement(statement);
        let result = self.process_statement_inner(statement);

        if result.is_ok() {
            if let Some(start_index) = start_index {
                self.annotate_recent_commands(start_index, statement_source, default_kind);
            }
        }

        self.current_statement_source = previous_statement_source;
        result
    }

    fn process_statement_inner(&mut self, statement: &Statement) -> Result<(), TranspileError> {
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
                    self.global_variables.insert(var.clone());
                }
            }
            Statement::MinecraftCommand(cmd) => {
                // Strip leading slash and process variable/parameter substitution
                let clean_cmd = Self::strip_command_prefix(cmd);
                let source_location = self.take_raw_command_source(clean_cmd.trim());
                let processor = CommandProcessor::new(
                    self.data_pack.namespace.clone(),
                    &self.current_context.params,
                    &self.scoreboard_variables,
                    &self.variables,
                    &self.variable_objectives,
                    &self.variable_storage_paths,
                    &self.selector_aliases,
                    &self.compile_time_constants,
                );

                let processed_cmd = processor.process_command_string(&clean_cmd)?;

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

    fn begin_function_capture(&mut self) -> SavedFunctionCapture {
        let saved = SavedFunctionCapture {
            function: self.current_function.take(),
            metadata: self.current_function_metadata.take(),
        };
        self.current_function = Some(Vec::new());
        self.current_function_metadata = Some(HashMap::new());
        saved
    }

    fn finish_function_capture(&mut self, saved: SavedFunctionCapture) -> FunctionCapture {
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

    fn restore_function_capture(&mut self, saved: SavedFunctionCapture) {
        self.current_function = saved.function;
        self.current_function_metadata = saved.metadata;
    }

    fn save_compiler_state(&self) -> SavedCompilerState {
        SavedCompilerState {
            current_context: self.current_context.clone(),
            variables: self.variables.clone(),
            variable_objectives: self.variable_objectives.clone(),
            scoreboard_variables: self.scoreboard_variables.clone(),
            global_variables: self.global_variables.clone(),
            compile_time_constants: self.compile_time_constants.clone(),
            selector_aliases: self.selector_aliases.clone(),
            entity_templates: self.entity_templates.clone(),
            variable_storage_paths: self.variable_storage_paths.clone(),
            variable_types: self.variable_types.clone(),
        }
    }

    fn restore_compiler_state(&mut self, saved: SavedCompilerState) {
        self.current_context = saved.current_context;
        self.variables = saved.variables;
        self.variable_objectives = saved.variable_objectives;
        self.scoreboard_variables = saved.scoreboard_variables;
        self.global_variables = saved.global_variables;
        self.compile_time_constants = saved.compile_time_constants;
        self.selector_aliases = saved.selector_aliases;
        self.entity_templates = saved.entity_templates;
        self.variable_storage_paths = saved.variable_storage_paths;
        self.variable_types = saved.variable_types;
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

    fn process_create_entity(&mut self, name: &str) -> Result<(), TranspileError> {
        if let Some(template) = self.entity_templates.get(name) {
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

            // Serialize NBT
            let nbt_str = self.serialize_to_snbt(&template.nbt)?;

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

    fn process_entity_def(&mut self, entity_def: &EntityDef) -> Result<(), TranspileError> {
        self.entity_templates
            .insert(entity_def.name.clone(), entity_def.clone());
        Ok(())
    }

    fn process_function_def(&mut self, func: &FunctionDef) -> Result<(), TranspileError> {
        // Save previous function context
        let previous_function = self.current_function.take();
        let previous_metadata = self.current_function_metadata.take();
        let previous_context = self.current_context.clone();
        let previous_return = self.current_return.clone();
        let previous_function_name = self.current_function_name.take();
        let previous_globals = self.global_variables.clone();
        let previous_variables = self.variables.clone();
        let previous_variable_objectives = self.variable_objectives.clone();
        let previous_scoreboard_variables = self.scoreboard_variables.clone();
        let previous_variable_storage_paths = self.variable_storage_paths.clone();
        let previous_variable_types = self.variable_types.clone();

        // Extract parameter names from Parameter structs
        let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();

        // Set up new function context
        self.current_context = FunctionContext::with_params(param_names.clone());
        self.current_function = Some(Vec::new());
        self.current_function_metadata = Some(HashMap::new());
        self.current_function_name = Some(func.name.clone());
        let return_kind = Self::analyze_function_return_kind(&func.body);
        // Only functions that actually contain a `return` statement need the
        // return-flag protocol. Everything else skips the prologue and the
        // `temp` objective so simple packs stay free of `_cobble_init`.
        if matches!(return_kind, FunctionReturnKind::None) {
            self.current_return = None;
        } else {
            let return_id = self.next_temp_id();
            self.current_return = Some(ReturnContext {
                value_score: format!("#return_value_{}", return_id),
                flag_score: format!("#return_flag_{}", return_id),
            });
            self.data_pack.track_objective("temp");
            if let (Some(commands), Some(return_context)) =
                (&mut self.current_function, &self.current_return)
            {
                commands.push(format!(
                    "scoreboard players set {} temp 0",
                    return_context.flag_score
                ));
            }
        }
        self.global_variables.clear();

        // Store function parameters
        self.function_params.insert(func.name.clone(), param_names);
        self.function_returns.insert(func.name.clone(), return_kind);

        // Process function body
        for statement in &func.body {
            self.process_statement(statement)?;
        }

        // Get the generated commands
        if let Some(mut commands) = self.current_function.take() {
            if matches!(return_kind, FunctionReturnKind::Integer) {
                if let Some(return_context) = &self.current_return {
                    commands.push(format!(
                        "execute unless score {} temp matches 1 run return 0",
                        return_context.flag_score
                    ));
                }
            }
            let metadata = self.current_function_metadata.take().unwrap_or_default();
            self.data_pack
                .add_function_with_metadata(func.name.clone(), commands, metadata);
        }

        // Restore previous context
        self.current_function = previous_function;
        self.current_function_metadata = previous_metadata;
        self.current_context = previous_context;
        self.current_return = previous_return;
        self.current_function_name = previous_function_name;
        self.global_variables = previous_globals;
        self.variables = previous_variables;
        self.variable_objectives = previous_variable_objectives;
        self.scoreboard_variables = previous_scoreboard_variables;
        self.variable_storage_paths = previous_variable_storage_paths;
        self.variable_types = previous_variable_types;

        Ok(())
    }

    /// Helper method to evaluate a complex expression into a target variable
    /// Returns the commands needed to compute the expression
    fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, TranspileError> {
        let mut evaluator = ExpressionEvaluator::new(
            &mut self.data_pack,
            &self.variable_objectives,
            &self.variable_storage_paths,
            &self.compile_time_constants,
            &self.function_params,
            &self.function_returns,
            &self.qualified_function_aliases,
            &self.current_context.params,
            &self.variable_types,
            &mut self.temp_counter,
            self.current_statement_source.clone(),
            self.current_function_name.as_deref(),
            &mut self.function_call_edges,
        );
        evaluator.evaluate_expression_to_target(expr, target)
    }

    fn process_expression(&mut self, expr: &Expression) -> Result<(), TranspileError> {
        match expr {
            Expression::Call(func, args) => {
                // Handle both Identifier and Attribute (for stdlib.addEventListener)
                match &**func {
                    Expression::Identifier(func_name) => {
                        // Check for standalone addEventListener
                        if func_name == "addEventListener" {
                            self.require_stdlib_module("event")?;
                            self.process_add_event_listener(args)?;
                        } else if func_name.contains('.') {
                            // Method call on module (e.g., stdlib.addEventListener)
                            let parts: Vec<&str> = func_name.split('.').collect();
                            if parts.len() == 2
                                && parts[0] == "stdlib"
                                && parts[1] == "addEventListener"
                            {
                                self.require_stdlib_module("event")?;
                                self.process_add_event_listener(args)?;
                            } else {
                                return Err(format!("Unknown module method: {}", func_name).into());
                            }
                        } else {
                            // Regular function call - generate Minecraft function call
                            self.generate_function_call(func_name, args)?;
                        }
                    }
                    Expression::Attribute(obj, method) => {
                        // Handle attribute access like stdlib.addEventListener
                        if let Some(module_name) = Self::expression_path(obj) {
                            if module_name == "stdlib" && method == "addEventListener" {
                                self.require_stdlib_module("event")?;
                                self.process_add_event_listener(args)?;
                            } else if module_name == "math" {
                                self.require_stdlib_module("math")?;
                                self.process_math_intrinsic(method, args)?;
                            } else if module_name == "text" {
                                self.require_stdlib_module("text")?;
                                self.process_text_intrinsic(method, args)?;
                            } else if module_name == "score" {
                                self.require_stdlib_module("score")?;
                                self.process_score_intrinsic(method, args)?;
                            } else if module_name == "score.objective" {
                                self.require_stdlib_module("score.objective")?;
                                self.process_score_objective_intrinsic(method, args)?;
                            } else if module_name == "random" {
                                self.require_stdlib_module("random")?;
                                self.process_random_intrinsic(method, args)?;
                            } else if module_name == "timer" {
                                self.require_stdlib_module("timer")?;
                                self.process_timer_intrinsic(method, args)?;
                            } else if module_name == "storage" {
                                self.require_stdlib_module("storage")?;
                                self.process_storage_intrinsic(method, args)?;
                            } else if module_name == "item_component" {
                                self.require_stdlib_module("item_component")?;
                                self.process_item_component_intrinsic(method, args)?;
                            } else if module_name == "schedule" {
                                self.require_stdlib_module("schedule")?;
                                self.process_schedule_intrinsic(method, args)?;
                            } else if module_name == "selector" {
                                self.require_stdlib_module("selector")?;
                                self.process_selector_intrinsic(method, args)?;
                            } else if module_name == "position" {
                                self.require_stdlib_module("position")?;
                                self.process_position_intrinsic(method, args)?;
                            } else if module_name == "bossbar" {
                                self.require_stdlib_module("bossbar")?;
                                self.process_bossbar_intrinsic(method, args)?;
                            } else if module_name == "team" {
                                self.require_stdlib_module("team")?;
                                self.process_team_intrinsic(method, args)?;
                            } else if module_name == "entity" {
                                self.require_stdlib_module("entity")?;
                                self.process_entity_intrinsic(method, args)?;
                            } else if module_name == "datapack" {
                                self.require_stdlib_module("datapack")?;
                                self.process_datapack_intrinsic(method, args)?;
                            } else if module_name == "resource_pack" {
                                self.require_stdlib_module("resource_pack")?;
                                self.process_resource_pack_intrinsic(method, args)?;
                            } else if let Some(func_name) = self
                                .resolve_user_function_call_name(func)
                                .map(str::to_string)
                            {
                                self.generate_function_call(&func_name, args)?;
                            } else {
                                return Err(format!(
                                    "Unknown module method: {}.{}",
                                    module_name, method
                                )
                                .into());
                            }
                        } else {
                            return Err("Complex attribute access not supported"
                                .to_string()
                                .into());
                        }
                    }
                    _ => {
                        return Err("Unsupported function call expression".into());
                    }
                }
            }
            _ => {
                // Other expressions (like standalone identifiers for docstrings) don't generate commands
            }
        }
        Ok(())
    }

    fn process_return(&mut self, expr: Option<&Expression>) -> Result<(), TranspileError> {
        if self.current_function.is_none() {
            return Err("return can only be used inside a function"
                .to_string()
                .into());
        }

        let mut commands = Vec::new();
        let context = self.current_return.clone();
        if let Some(expr) = expr {
            match self.infer_type(expr) {
                CobbleType::String | CobbleType::List | CobbleType::Map => {
                    return Err(
                        "return values must be integer scoreboard expressions; use storage/output variables for strings, lists, and maps."
                            .into(),
                    );
                }
                _ => {}
            }
            let temp_var = context
                .as_ref()
                .map(|context| context.value_score.clone())
                .unwrap_or_else(|| {
                    let temp_var = format!("#return_value_{}", self.next_temp_id());
                    temp_var
                });
            commands.extend(self.evaluate_expression_to_target(expr, &temp_var)?);
            if let Some(context) = &context {
                commands.push(format!(
                    "scoreboard players set {} temp 1",
                    context.flag_score
                ));
            }
            commands.push(format!(
                "return run scoreboard players get {} temp",
                temp_var
            ));
        } else {
            if let Some(context) = &context {
                commands.push(format!(
                    "scoreboard players set {} temp 0",
                    context.value_score
                ));
                commands.push(format!(
                    "scoreboard players set {} temp 1",
                    context.flag_score
                ));
            }
            commands.push("return 0".to_string());
        }

        if let Some(ref mut function_commands) = self.current_function {
            function_commands.extend(commands);
        }

        Ok(())
    }

    /// Record a lowered user-function call edge for cycle detection and reject
    /// direct self-recursion at the call site. Module-level calls (no enclosing
    /// user function) record nothing.
    pub(in crate::transpiler) fn record_user_function_call(
        &mut self,
        callee: &str,
    ) -> Result<(), TranspileError> {
        let Some(caller) = self.current_function_name.clone() else {
            return Ok(());
        };
        if caller == callee {
            return Err(format!(
                "Recursive call to '{}' is not supported. Rewrite the recursion as a loop (while/for) instead.",
                callee
            ).into());
        }
        self.function_call_edges
            .entry(caller)
            .or_default()
            .insert(callee.to_string());
        Ok(())
    }

    /// Reject mutually recursive user-function call cycles. Direct self-calls
    /// error eagerly at the call site; this catches `a -> b -> a` style cycles
    /// once all call edges have been recorded. Recursion cannot compile
    /// correctly because argument frames and return flags are global fake
    /// players, not per-invocation stacks.
    fn check_recursive_call_cycles(&self) -> Result<(), TranspileError> {
        fn visit(
            node: &str,
            edges: &BTreeMap<String, BTreeSet<String>>,
            visiting: &mut Vec<String>,
            done: &mut HashSet<String>,
        ) -> Option<Vec<String>> {
            if let Some(position) = visiting.iter().position(|name| name == node) {
                let mut cycle = visiting[position..].to_vec();
                cycle.push(node.to_string());
                return Some(cycle);
            }
            if done.contains(node) {
                return None;
            }
            visiting.push(node.to_string());
            if let Some(callees) = edges.get(node) {
                for callee in callees {
                    if let Some(cycle) = visit(callee, edges, visiting, done) {
                        return Some(cycle);
                    }
                }
            }
            visiting.pop();
            done.insert(node.to_string());
            None
        }

        let mut done = HashSet::new();
        for node in self.function_call_edges.keys() {
            let mut visiting = Vec::new();
            if let Some(cycle) = visit(node, &self.function_call_edges, &mut visiting, &mut done) {
                return Err(format!(
                    "Recursive function calls are not supported: cycle {}. Rewrite the recursion as a loop (while/for) instead.",
                    cycle.join(" -> ")
                ).into());
            }
        }
        Ok(())
    }

    /// A storage-backed string variable's scoreboard mirror only holds the
    /// string length (`data get` on a string), so reading it as a score in
    /// arithmetic silently computes garbage. Reject it up front.
    pub(in crate::transpiler) fn reject_string_score_operand(
        &self,
        name: &str,
    ) -> Result<(), TranspileError> {
        if self.variable_storage_paths.contains_key(name)
            && matches!(self.variable_types.get(name), Some(CobbleType::String))
        {
            return Err(format!(
                "Variable '{}' holds a storage-backed string and cannot be used in arithmetic; its scoreboard mirror only stores the string length. Use integer values instead.",
                name
            ).into());
        }
        Ok(())
    }

    pub(in crate::transpiler) fn return_propagation_command(&self) -> Option<String> {
        let context = self.current_return.as_ref()?;
        Some(format!(
            "execute if score {} temp matches 1 run return run scoreboard players get {} temp",
            context.flag_score, context.value_score
        ))
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

    fn expr_to_plain_arg(&self, expr: &Expression, label: &str) -> Result<String, TranspileError> {
        match expr {
            Expression::String(s) | Expression::Identifier(s) => Ok(s.clone()),
            Expression::Number(n) if n.fract() == 0.0 => Ok((*n as i32).to_string()),
            Expression::Number(n) => Ok(n.to_string()),
            Expression::Call(func, args) => {
                if let Some(value) = self.storage_path_helper_string(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.selector_helper_string(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.position_helper_string(func, args)? {
                    Ok(value)
                } else {
                    Err(format!("{} must be a string, identifier, or number", label).into())
                }
            }
            _ => Err(format!("{} must be a string, identifier, or number", label).into()),
        }
    }

    fn expr_to_selector_arg(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        let value = self.expr_to_plain_arg(expr, label)?;
        Ok(self.expand_selector_alias_arg(value))
    }

    fn expand_selector_alias_arg(&self, value: String) -> String {
        let Some(alias) = value.strip_prefix('@') else {
            return value;
        };
        self.selector_aliases.get(alias).cloned().unwrap_or(value)
    }

    fn expr_to_i32(&self, expr: &Expression, label: &str) -> Result<i32, TranspileError> {
        match expr {
            Expression::Number(n) => Ok(*n as i32),
            Expression::Identifier(name) => self
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32)
                .ok_or_else(|| format!("{} must be a literal integer or const", label))
                .map_err(TranspileError::from),
            _ => Err(format!("{} must be a literal integer or const", label).into()),
        }
    }

    fn expr_to_bool(&self, expr: &Expression, label: &str) -> Result<bool, TranspileError> {
        match expr {
            Expression::Boolean(value) => Ok(*value),
            _ => Err(format!("{} must be a literal boolean", label).into()),
        }
    }

    fn next_temp_id(&mut self) -> u32 {
        let id = self.temp_counter;
        self.temp_counter += 1;
        id
    }

    fn dynamic_storage_command<F>(
        &mut self,
        plan: &StoragePathPlan,
        kind: GeneratedCommandKind,
        build_helper_commands: F,
    ) -> Result<Vec<String>, TranspileError>
    where
        F: FnOnce(&str) -> Vec<String>,
    {
        let id = self.next_temp_id();
        let (mut commands, args_path) = self.prepare_dynamic_storage_path(id, plan)?;
        let helper_name = format!("_cobble_storage_dyn_{}", id);
        let helper_commands = build_helper_commands(plan.template());
        self.add_generated_helper_function(helper_name.clone(), helper_commands, kind);
        commands.push(format!(
            "function {}:{} with storage {}:global {}",
            self.data_pack.namespace, helper_name, self.data_pack.namespace, args_path
        ));
        commands.push(format!(
            "data remove storage {}:global {}",
            self.data_pack.namespace, args_path
        ));
        Ok(commands)
    }

    fn qualify_namespaced_id(&self, value: &str) -> String {
        if value.contains(':') {
            value.to_string()
        } else {
            format!("{}:{}", self.data_pack.namespace, value)
        }
    }

    fn expr_to_json_value(&self, expr: &Expression) -> Result<serde_json::Value, TranspileError> {
        match expr {
            Expression::Number(n) if n.fract() == 0.0 => Ok(serde_json::json!(*n as i32)),
            Expression::Number(n) => Ok(serde_json::json!(n)),
            Expression::String(s) => Ok(serde_json::json!(s)),
            Expression::Boolean(b) => Ok(serde_json::json!(b)),
            Expression::None => Ok(serde_json::Value::Null),
            Expression::Array(items) => items
                .iter()
                .map(|item| self.expr_to_json_value(item))
                .collect::<Result<Vec<_>, _>>()
                .map(serde_json::Value::Array),
            Expression::Map(entries) => {
                let mut object = serde_json::Map::new();
                for (key, value) in entries {
                    object.insert(key.clone(), self.expr_to_json_value(value)?);
                }
                Ok(serde_json::Value::Object(object))
            }
            Expression::Call(func, args) => {
                if let Some(value) = self.item_component_helper_json(func, args)? {
                    Ok(value)
                } else if let Some(value) = self.text_component_helper_json(func, args)? {
                    Ok(value)
                } else {
                    Err("Only text.* component helpers and item_component.* helpers can be serialized to JSON here".into())
                }
            }
            Expression::Identifier(name) => {
                if let Some(value) = self.compile_time_constants.get(name) {
                    if value.fract() == 0.0 {
                        Ok(serde_json::json!(*value as i32))
                    } else {
                        Ok(serde_json::json!(value))
                    }
                } else {
                    Err(format!(
                        "Identifier '{}' cannot be serialized to JSON here; use a literal or const",
                        name
                    )
                    .into())
                }
            }
            _ => Err(format!("Unsupported expression in JSON value: {:?}", expr).into()),
        }
    }

    fn expr_to_text_component(&self, expr: &Expression) -> Result<String, TranspileError> {
        let value = self.expr_to_text_component_value(expr)?;
        serde_json::to_string(&value)
            .map_err(|e| format!("Failed to encode text component: {}", e))
            .map_err(TranspileError::from)
    }

    fn expr_to_text_component_value(
        &self,
        expr: &Expression,
    ) -> Result<serde_json::Value, TranspileError> {
        match expr {
            Expression::String(s) => Ok(serde_json::json!({ "text": s })),
            _ => self.expr_to_json_value(expr),
        }
    }

    fn text_component_helper_json(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<serde_json::Value>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "text" {
            return Ok(None);
        }

        self.require_stdlib_module("text")?;
        let value = match method.as_str() {
            "plain" => {
                if args.len() != 1 {
                    return Err("text.plain() takes 1 argument".into());
                }
                let text = self.expr_to_plain_arg(&args[0], "text.plain text")?;
                serde_json::json!({ "text": text })
            }
            "colored" => {
                if args.len() != 2 {
                    return Err("text.colored() takes 2 arguments".into());
                }
                let text = self.expr_to_plain_arg(&args[0], "text.colored text")?;
                let color = self.expr_to_plain_arg(&args[1], "text color")?;
                validate_text_color(&color)?;
                serde_json::json!({ "text": text, "color": color })
            }
            "score" => {
                if args.len() != 2 {
                    return Err("text.score() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score component name")?;
                let objective = self.expr_to_plain_arg(&args[1], "score component objective")?;
                serde_json::json!({ "score": { "name": name, "objective": objective } })
            }
            "selector" => {
                if args.len() != 1 {
                    return Err("text.selector() takes 1 argument".into());
                }
                let selector = self.expr_to_selector_arg(&args[0], "selector component")?;
                serde_json::json!({ "selector": selector })
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    fn item_component_helper_json(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<serde_json::Value>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "item_component" {
            return Ok(None);
        }

        self.require_stdlib_module("item_component")?;
        let value = match method.as_str() {
            "components" => {
                if args.is_empty() {
                    return Err("item_component.components() takes at least 1 argument"
                        .to_string()
                        .into());
                }
                let mut merged = serde_json::Map::new();
                for arg in args {
                    let component_map = self.expr_to_item_component_map(arg)?;
                    for (key, value) in component_map {
                        if merged.contains_key(&key) {
                            return Err(format!(
                                "item_component.components() duplicate component '{}'",
                                key
                            )
                            .into());
                        }
                        merged.insert(key, value);
                    }
                }
                serde_json::Value::Object(merged)
            }
            "custom_name" => {
                if args.len() != 1 {
                    return Err("item_component.custom_name() takes 1 argument"
                        .to_string()
                        .into());
                }
                serde_json::json!({
                    "minecraft:custom_name": self.expr_to_text_component_value(&args[0])?
                })
            }
            "lore" => {
                if args.len() != 1 {
                    return Err("item_component.lore() takes 1 argument".into());
                }
                let Expression::Array(entries) = &args[0] else {
                    return Err("item_component.lore() argument must be an array"
                        .to_string()
                        .into());
                };
                let lore = entries
                    .iter()
                    .map(|entry| self.expr_to_text_component_value(entry))
                    .collect::<Result<Vec<_>, _>>()?;
                serde_json::json!({ "minecraft:lore": lore })
            }
            "unbreakable" => {
                if !args.is_empty() {
                    return Err("item_component.unbreakable() takes 0 arguments"
                        .to_string()
                        .into());
                }
                serde_json::json!({ "minecraft:unbreakable": {} })
            }
            _ => return Ok(None),
        };
        Ok(Some(value))
    }

    fn expr_to_item_component_map(
        &self,
        expr: &Expression,
    ) -> Result<serde_json::Map<String, serde_json::Value>, TranspileError> {
        let value = self.expr_to_json_value(expr)?;
        value
            .as_object()
            .cloned()
            .ok_or_else(|| {
                "item_component.components() arguments must be component objects".to_string()
            })
            .map_err(TranspileError::from)
    }

    fn process_item_component_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("item_component".to_string())),
            method.to_string(),
        );
        if self.item_component_helper_json(&func, args)?.is_some() {
            return Err(format!(
                "item_component.{}() returns an item component JSON object; pass it to datapack.item_modifier() or another JSON value",
                method
            ).into());
        }
        Err(format!("Unknown item component function: item_component.{}", method).into())
    }

    fn expr_to_storage_path_plan(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<StoragePathPlan, TranspileError> {
        match expr {
            Expression::String(s) | Expression::Identifier(s) => {
                Ok(StoragePathPlan::static_path(s.clone()))
            }
            Expression::Number(n) if n.fract() == 0.0 => {
                Ok(StoragePathPlan::static_path((*n as i32).to_string()))
            }
            Expression::Number(n) => Ok(StoragePathPlan::static_path(n.to_string())),
            Expression::Call(func, args) => {
                if let Some(value) = self.storage_path_helper_plan(func, args)? {
                    Ok(value)
                } else {
                    Err(format!("{} must be a storage path", label).into())
                }
            }
            _ => Err(format!("{} must be a storage path", label).into()),
        }
    }

    fn storage_path_helper_string(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<String>, TranspileError> {
        let Some(plan) = self.storage_path_helper_plan(func, args)? else {
            return Ok(None);
        };
        match plan {
            StoragePathPlan::Static(path) => Ok(Some(path)),
            StoragePathPlan::Dynamic { .. } => Err(
                "Dynamic storage paths can only be used by storage helpers that emit Minecraft commands"
                    .into(),
            ),
        }
    }

    fn storage_path_helper_plan(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<StoragePathPlan>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "storage" {
            return Ok(None);
        }

        self.require_stdlib_module("storage")?;
        let path = match method.as_str() {
            "path" => {
                if args.is_empty() {
                    return Err("storage.path() takes at least 1 argument"
                        .to_string()
                        .into());
                }
                StoragePathPlan::static_path(
                    args.iter()
                        .map(|arg| {
                            self.expr_to_storage_path_component(arg, "storage path component")
                        })
                        .collect::<Result<Vec<_>, _>>()?
                        .join("."),
                )
            }
            "child" => {
                if args.len() != 2 {
                    return Err("storage.child() takes 2 arguments".into());
                }
                let base = self.expr_to_storage_path_plan(&args[0], "storage base path")?;
                let child = self.expr_to_storage_path_component(&args[1], "storage child path")?;
                base.append_field(&child)
            }
            "index" => {
                if args.len() != 2 {
                    return Err("storage.index() takes 2 arguments".into());
                }
                let base = self.expr_to_storage_path_plan(&args[0], "storage base path")?;
                if let Ok(index) = self.expr_to_i32(&args[1], "storage list index") {
                    base.append_static_index(index)
                } else {
                    base.append_dynamic_index(args[1].clone())
                }
            }
            _ => return Ok(None),
        };
        Ok(Some(path))
    }

    fn expr_to_storage_path_component(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<String, TranspileError> {
        let component = self.expr_to_plain_arg(expr, label)?;
        if component.is_empty() {
            return Err(format!("{} cannot be empty", label).into());
        }
        if component
            .chars()
            .any(|ch| ch == '.' || ch == '[' || ch == ']')
        {
            return Err(format!("{} must be a single unqualified path component", label).into());
        }
        Ok(component)
    }

    fn selector_helper_string(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<String>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "selector" {
            return Ok(None);
        }

        self.require_stdlib_module("selector")?;
        let selector = match method.as_str() {
            "current" => {
                if !args.is_empty() {
                    return Err("selector.current() takes 0 arguments".into());
                }
                "@s".to_string()
            }
            "nearest_player" => {
                if !args.is_empty() {
                    return Err("selector.nearest_player() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@p".to_string()
            }
            "all_players" => {
                if !args.is_empty() {
                    return Err("selector.all_players() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@a".to_string()
            }
            "all_entities" => {
                if !args.is_empty() {
                    return Err("selector.all_entities() takes 0 arguments"
                        .to_string()
                        .into());
                }
                "@e".to_string()
            }
            "tagged" | "players_tagged" => {
                if args.len() != 1 {
                    return Err(format!("selector.{}() takes 1 argument", method).into());
                }
                let tag = self.expr_to_plain_arg(&args[0], "selector tag")?;
                if method == "players_tagged" {
                    format!("@a[tag={}]", tag)
                } else {
                    format!("@e[tag={}]", tag)
                }
            }
            "entity_type" => {
                if args.len() != 1 {
                    return Err("selector.entity_type() takes 1 argument".into());
                }
                let entity_type = self.expr_to_plain_arg(&args[0], "selector entity type")?;
                format!("@e[type={}]", entity_type)
            }
            _ => return Ok(None),
        };
        Ok(Some(selector))
    }

    fn position_helper_string(
        &self,
        func: &Expression,
        args: &[Expression],
    ) -> Result<Option<String>, TranspileError> {
        let Expression::Attribute(obj, method) = func else {
            return Ok(None);
        };
        let Some(module_name) = Self::expression_path(obj) else {
            return Ok(None);
        };
        if module_name != "position" {
            return Ok(None);
        }

        self.require_stdlib_module("position")?;
        let position = match method.as_str() {
            "here" => {
                if !args.is_empty() {
                    return Err("position.here() takes 0 arguments".into());
                }
                "~ ~ ~".to_string()
            }
            "absolute" => self.format_position_args(method, args, "")?,
            "relative" => self.format_position_args(method, args, "~")?,
            "local" => self.format_position_args(method, args, "^")?,
            _ => return Ok(None),
        };
        Ok(Some(position))
    }

    fn format_position_args(
        &self,
        method: &str,
        args: &[Expression],
        prefix: &str,
    ) -> Result<String, TranspileError> {
        if args.len() != 3 {
            return Err(format!("position.{}() takes 3 arguments", method).into());
        }
        args.iter()
            .map(|arg| self.format_position_component(arg, prefix))
            .collect::<Result<Vec<_>, _>>()
            .map(|parts| parts.join(" "))
    }

    fn format_position_component(
        &self,
        expr: &Expression,
        prefix: &str,
    ) -> Result<String, TranspileError> {
        let value = self.expr_to_plain_arg(expr, "position component")?;
        if prefix.is_empty() {
            Ok(value)
        } else if value == "0" {
            Ok(prefix.to_string())
        } else {
            Ok(format!("{}{}", prefix, value))
        }
    }

    fn process_selector_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("selector".to_string())),
            method.to_string(),
        );
        if self.selector_helper_string(&func, args)?.is_some() {
            return Err(format!(
                "selector.{}() returns a selector string; pass it to a helper that accepts a selector",
                method
            ).into());
        }
        Err(format!("Unknown selector function: selector.{}", method).into())
    }

    fn process_position_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let func = Expression::Attribute(
            Box::new(Expression::Identifier("position".to_string())),
            method.to_string(),
        );
        if self.position_helper_string(&func, args)?.is_some() {
            return Err(format!(
                "position.{}() returns a coordinate string; pass it to a helper that accepts a position",
                method
            ).into());
        }
        Err(format!("Unknown position function: position.{}", method).into())
    }

    fn process_text_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if matches!(method, "plain" | "colored" | "score" | "selector") {
            return Err(format!(
                "text.{}() returns a JSON text component; pass it to text.tellraw(), text.title(), or another JSON value",
                method
            ).into());
        }
        if args.len() != 2 {
            return Err(format!("text.{}() takes 2 arguments", method).into());
        }

        let target = self.expr_to_selector_arg(&args[0], "text target")?;
        let component = self.expr_to_text_component(&args[1])?;
        let command = match method {
            "tellraw" => format!("tellraw {} {}", target, component),
            "title" | "subtitle" | "actionbar" => {
                format!("title {} {} {}", target, method, component)
            }
            _ => return Err(format!("Unknown text function: text.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_score_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("score.set() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_i32(&args[1], "score value")?;
                format!("scoreboard players set {} temp {}", name, value)
            }
            "add" | "remove" => {
                if args.len() != 2 {
                    return Err(format!("score.{}() takes 2 arguments", method).into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_i32(&args[1], "score value")?;
                format!("scoreboard players {} {} temp {}", method, name, value)
            }
            "reset" => {
                if args.len() != 1 {
                    return Err("score.reset() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                format!("scoreboard players reset {} temp", name)
            }
            "copy" => {
                if args.len() != 2 {
                    return Err("score.copy() takes 2 arguments".into());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let src = self.expr_to_plain_arg(&args[1], "source score")?;
                format!("scoreboard players operation {} temp = {} temp", dst, src)
            }
            "operation" => {
                if args.len() != 3 {
                    return Err("score.operation() takes 3 arguments".into());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let op = self.expr_to_plain_arg(&args[1], "score operation")?;
                let src = self.expr_to_plain_arg(&args[2], "source score")?;
                const ALLOWED_OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "<", ">", "><"];
                if !ALLOWED_OPS.contains(&op.as_str()) {
                    return Err(format!("Unsupported scoreboard operation '{}'", op).into());
                }
                format!(
                    "scoreboard players operation {} temp {} {} temp",
                    dst, op, src
                )
            }
            _ => return Err(format!("Unknown score function: score.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_score_objective_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("score.objective.add() takes 2 or 3 arguments"
                        .to_string()
                        .into());
                }
                let name = self.expr_to_plain_arg(&args[0], "objective name")?;
                let criteria = self.expr_to_plain_arg(&args[1], "objective criteria")?;
                if args.len() == 3 {
                    let display = self.expr_to_text_component(&args[2])?;
                    format!(
                        "scoreboard objectives add {} {} {}",
                        name, criteria, display
                    )
                } else {
                    format!("scoreboard objectives add {} {}", name, criteria)
                }
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("score.objective.remove() takes 1 argument"
                        .to_string()
                        .into());
                }
                let name = self.expr_to_plain_arg(&args[0], "objective name")?;
                format!("scoreboard objectives remove {}", name)
            }
            "display" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("score.objective.display() takes 1 or 2 arguments"
                        .to_string()
                        .into());
                }
                let slot = self.expr_to_plain_arg(&args[0], "scoreboard display slot")?;
                if args.len() == 2 {
                    let name = self.expr_to_plain_arg(&args[1], "objective name")?;
                    format!("scoreboard objectives setdisplay {} {}", slot, name)
                } else {
                    format!("scoreboard objectives setdisplay {}", slot)
                }
            }
            _ => {
                return Err(format!(
                    "Unknown score objective function: score.objective.{}",
                    method
                )
                .into())
            }
        };
        self.push_current_command(command)
    }

    fn process_random_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "int" => {
                if args.len() != 3 {
                    return Err("random.int() takes 3 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                let min = self.expr_to_i32(&args[1], "random min")?;
                let max = self.expr_to_i32(&args[2], "random max")?;
                if min > max {
                    return Err("random.int() min cannot be greater than max"
                        .to_string()
                        .into());
                }
                if min == max {
                    format!("scoreboard players set {} temp {}", name, min)
                } else {
                    format!(
                        "execute store result score {} temp run random value {}..{}",
                        name, min, max
                    )
                }
            }
            "bool" => {
                if args.len() != 1 {
                    return Err("random.bool() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                format!(
                    "execute store result score {} temp run random value 0..1",
                    name
                )
            }
            _ => return Err(format!("Unknown random function: random.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_timer_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("timer.set() takes 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                let ticks = self.expr_to_i32(&args[1], "timer ticks")?;
                format!("scoreboard players set {} temp {}", name, ticks)
            }
            "tick" => {
                if args.len() != 1 {
                    return Err("timer.tick() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!(
                    "execute if score {} temp matches 1.. run scoreboard players remove {} temp 1",
                    name, name
                )
            }
            "done" => {
                if args.len() != 1 {
                    return Err("timer.done() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                let flag = format!("{}_done", name);
                self.push_current_command(format!("scoreboard players set {} temp 0", flag))?;
                format!(
                    "execute if score {} temp matches ..0 run scoreboard players set {} temp 1",
                    name, flag
                )
            }
            "reset" => {
                if args.len() != 1 {
                    return Err("timer.reset() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!("scoreboard players reset {} temp", name)
            }
            _ => return Err(format!("Unknown timer function: timer.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_storage_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let namespace = self.data_pack.namespace.clone();
        let mut commands = Vec::new();

        match method {
            "set" => {
                if args.len() != 2 {
                    return Err("storage.set() takes 2 arguments".into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                if let Some(static_path) = path.as_static().map(str::to_string) {
                    commands.extend(self.storage_value_commands(&static_path, &args[1])?);
                } else if let Ok(value) = self.serialize_to_snbt(&args[1]) {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &path,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![format!(
                                "$data modify storage {}:global {} set value {}",
                                ns, template, value
                            )]
                        },
                    )?);
                } else {
                    let (value_commands, value_path) = self.materialize_storage_value(&args[1])?;
                    commands.extend(value_commands);
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &path,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![format!(
                                "$data modify storage {}:global {} set from storage {}:global {}",
                                ns, template, ns, value_path
                            )]
                        },
                    )?);
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, value_path
                    ));
                }
            }
            "merge" => {
                if args.len() != 2 {
                    return Err("storage.merge() takes 2 arguments".into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                if let Ok(value) = self.serialize_to_snbt(&args[1]) {
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} merge value {}",
                            namespace, static_path, value
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} merge value {}",
                                    ns, template, value
                                )]
                            },
                        )?);
                    }
                } else {
                    let (value_commands, value_path) = self.materialize_storage_value(&args[1])?;
                    commands.extend(value_commands);
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} merge from storage {}:global {}",
                            namespace, static_path, namespace, value_path
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} merge from storage {}:global {}",
                                    ns, template, ns, value_path
                                )]
                            },
                        )?);
                    }
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, value_path
                    ));
                }
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("storage.remove() takes 1 argument".into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                if let Some(static_path) = path.as_static() {
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, static_path
                    ));
                } else {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &path,
                        GeneratedCommandKind::StdLib,
                        |template| vec![format!("$data remove storage {}:global {}", ns, template)],
                    )?);
                }
            }
            "copy" => {
                if args.len() != 2 {
                    return Err("storage.copy() takes 2 arguments".into());
                }
                let dst = self.expr_to_storage_path_plan(&args[0], "destination storage path")?;
                let src = self.expr_to_storage_path_plan(&args[1], "source storage path")?;

                let source_path = if let Some(static_src) = src.as_static().map(str::to_string) {
                    static_src
                } else {
                    let temp_path = format!("_cobble_copy_source_{}", self.next_temp_id());
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &src,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![
                                format!("data remove storage {}:global {}", ns, temp_path),
                                format!(
                                    "$data modify storage {}:global {} set from storage {}:global {}",
                                    ns, temp_path, ns, template
                                ),
                            ]
                        },
                    )?);
                    temp_path
                };

                if let Some(static_dst) = dst.as_static() {
                    commands.push(format!(
                        "data modify storage {}:global {} set from storage {}:global {}",
                        namespace, static_dst, namespace, source_path
                    ));
                } else {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &dst,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![format!(
                                "$data modify storage {}:global {} set from storage {}:global {}",
                                ns, template, ns, source_path
                            )]
                        },
                    )?);
                }

                if source_path.starts_with("_cobble_copy_source_") {
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, source_path
                    ));
                }
            }
            "append" | "prepend" => {
                if args.len() != 2 {
                    return Err(format!("storage.{}() takes 2 arguments", method).into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                if let Ok(value) = self.serialize_to_snbt(&args[1]) {
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} {} value {}",
                            namespace, static_path, method, value
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} {} value {}",
                                    ns, template, method, value
                                )]
                            },
                        )?);
                    }
                } else {
                    let (value_commands, value_path) = self.materialize_storage_value(&args[1])?;
                    commands.extend(value_commands);
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} {} from storage {}:global {}",
                            namespace, static_path, method, namespace, value_path
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} {} from storage {}:global {}",
                                    ns, template, method, ns, value_path
                                )]
                            },
                        )?);
                    }
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, value_path
                    ));
                }
            }
            "insert" => {
                if args.len() != 3 {
                    return Err("storage.insert() takes 3 arguments".into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                let index = self.expr_to_i32(&args[1], "storage insert index")?;
                if let Ok(value) = self.serialize_to_snbt(&args[2]) {
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} insert {} value {}",
                            namespace, static_path, index, value
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} insert {} value {}",
                                    ns, template, index, value
                                )]
                            },
                        )?);
                    }
                } else {
                    let (value_commands, value_path) = self.materialize_storage_value(&args[2])?;
                    commands.extend(value_commands);
                    if let Some(static_path) = path.as_static() {
                        commands.push(format!(
                            "data modify storage {}:global {} insert {} from storage {}:global {}",
                            namespace, static_path, index, namespace, value_path
                        ));
                    } else {
                        let ns = namespace.clone();
                        commands.extend(self.dynamic_storage_command(
                            &path,
                            GeneratedCommandKind::StdLib,
                            |template| {
                                vec![format!(
                                    "$data modify storage {}:global {} insert {} from storage {}:global {}",
                                    ns, template, index, ns, value_path
                                )]
                            },
                        )?);
                    }
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        namespace, value_path
                    ));
                }
            }
            "get" => {
                if args.len() != 1 {
                    return Err("storage.get() takes 1 argument".into());
                }
                let path = self.expr_to_storage_path_plan(&args[0], "storage path")?;
                if let Some(static_path) = path.as_static() {
                    commands.push(format!(
                        "data get storage {}:global {}",
                        namespace, static_path
                    ));
                } else {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &path,
                        GeneratedCommandKind::StdLib,
                        |template| vec![format!("$data get storage {}:global {}", ns, template)],
                    )?);
                }
            }
            "read_score" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("storage.read_score() takes 2 or 3 arguments"
                        .to_string()
                        .into());
                }
                self.data_pack.track_objective("temp");
                let score = self.expr_to_plain_arg(&args[0], "target score")?;
                let path = self.expr_to_storage_path_plan(&args[1], "storage path")?;
                // No default scale: `data get` with a scale accepts only
                // numbers, while the bare form also yields a list's or
                // compound's size and a string's length (numbers floor the
                // same either way).
                let scale = if args.len() == 3 {
                    format!(" {}", self.expr_to_plain_arg(&args[2], "data get scale")?)
                } else {
                    String::new()
                };
                if let Some(static_path) = path.as_static() {
                    commands.push(format!(
                        "execute store result score {} temp run data get storage {}:global {}{}",
                        score, namespace, static_path, scale
                    ));
                } else {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &path,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![
                                format!("scoreboard players set {} temp 0", score),
                                format!(
                                    "$execute store result score {} temp run data get storage {}:global {}{}",
                                    score, ns, template, scale
                                ),
                            ]
                        },
                    )?);
                }
            }
            "copy_from" => {
                if args.len() != 4 {
                    return Err("storage.copy_from() takes 4 arguments".into());
                }
                let dst = self.expr_to_storage_path_plan(&args[0], "destination storage path")?;
                let source_type = self.expr_to_plain_arg(&args[1], "source type")?;
                let source_id = if source_type == "entity" {
                    self.expr_to_selector_arg(&args[2], "source id")?
                } else {
                    self.expr_to_plain_arg(&args[2], "source id")?
                };
                let source_path = self.expr_to_plain_arg(&args[3], "source path")?;
                if !matches!(source_type.as_str(), "entity" | "block" | "storage") {
                    return Err(
                        "storage.copy_from() source type must be entity, block, or storage"
                            .to_string()
                            .into(),
                    );
                }
                if let Some(static_dst) = dst.as_static() {
                    commands.push(format!(
                        "data modify storage {}:global {} set from {} {} {}",
                        namespace, static_dst, source_type, source_id, source_path
                    ));
                } else {
                    let ns = namespace.clone();
                    commands.extend(self.dynamic_storage_command(
                        &dst,
                        GeneratedCommandKind::StdLib,
                        |template| {
                            vec![format!(
                                "$data modify storage {}:global {} set from {} {} {}",
                                ns, template, source_type, source_id, source_path
                            )]
                        },
                    )?);
                }
            }
            "path" | "child" | "index" => {
                let func = Expression::Attribute(
                    Box::new(Expression::Identifier("storage".to_string())),
                    method.to_string(),
                );
                if self.storage_path_helper_plan(&func, args)?.is_some() {
                    return Err(format!(
                        "storage.{}() returns a storage path string; pass it to a helper that accepts a storage path",
                        method
                    ).into());
                }
                return Err(format!("Unknown storage function: storage.{}", method).into());
            }
            _ => return Err(format!("Unknown storage function: storage.{}", method).into()),
        }

        self.push_current_commands(commands)
    }

    fn process_schedule_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "once" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("schedule.once() takes 2 or 3 arguments".into());
                }
                let function = self.expr_to_plain_arg(&args[0], "scheduled function")?;
                let delay = self.expr_to_plain_arg(&args[1], "schedule delay")?;
                let function = self.qualify_namespaced_id(&function);
                if args.len() == 3 {
                    let mode = self.expr_to_plain_arg(&args[2], "schedule mode")?;
                    if !matches!(mode.as_str(), "append" | "replace") {
                        return Err("schedule.once() mode must be append or replace"
                            .to_string()
                            .into());
                    }
                    format!("schedule function {} {} {}", function, delay, mode)
                } else {
                    format!("schedule function {} {}", function, delay)
                }
            }
            "clear" => {
                if args.len() != 1 {
                    return Err("schedule.clear() takes 1 argument".into());
                }
                let function = self.expr_to_plain_arg(&args[0], "scheduled function")?;
                format!("schedule clear {}", self.qualify_namespaced_id(&function))
            }
            _ => return Err(format!("Unknown schedule function: schedule.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_bossbar_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if args.len() != 2 {
                    return Err("bossbar.add() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let name = self.expr_to_text_component(&args[1])?;
                format!("bossbar add {} {}", self.qualify_namespaced_id(&id), name)
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("bossbar.remove() takes 1 argument".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                format!("bossbar remove {}", self.qualify_namespaced_id(&id))
            }
            "set_value" | "set_max" => {
                if args.len() != 2 {
                    return Err(format!("bossbar.{}() takes 2 arguments", method).into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let value = self.expr_to_i32(&args[1], "bossbar value")?;
                let property = method.strip_prefix("set_").unwrap();
                format!(
                    "bossbar set {} {} {}",
                    self.qualify_namespaced_id(&id),
                    property,
                    value
                )
            }
            "set_name" => {
                if args.len() != 2 {
                    return Err("bossbar.set_name() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let name = self.expr_to_text_component(&args[1])?;
                format!(
                    "bossbar set {} name {}",
                    self.qualify_namespaced_id(&id),
                    name
                )
            }
            "set_color" | "set_style" => {
                if args.len() != 2 {
                    return Err(format!("bossbar.{}() takes 2 arguments", method).into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let value = self.expr_to_plain_arg(&args[1], "bossbar property value")?;
                let property = method.strip_prefix("set_").unwrap();
                match method {
                    "set_color" if !Self::is_valid_bossbar_color(&value) => {
                        return Err(format!("Invalid bossbar color: {}", value).into());
                    }
                    "set_style" if !Self::is_valid_bossbar_style(&value) => {
                        return Err(format!("Invalid bossbar style: {}", value).into());
                    }
                    _ => {}
                }
                format!(
                    "bossbar set {} {} {}",
                    self.qualify_namespaced_id(&id),
                    property,
                    value
                )
            }
            "set_visible" => {
                if args.len() != 2 {
                    return Err("bossbar.set_visible() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let visible = self.expr_to_bool(&args[1], "bossbar visibility")?;
                format!(
                    "bossbar set {} visible {}",
                    self.qualify_namespaced_id(&id),
                    visible
                )
            }
            "set_players" => {
                if args.len() != 2 {
                    return Err("bossbar.set_players() takes 2 arguments".into());
                }
                let id = self.expr_to_plain_arg(&args[0], "bossbar id")?;
                let players = self.expr_to_selector_arg(&args[1], "bossbar players")?;
                format!(
                    "bossbar set {} players {}",
                    self.qualify_namespaced_id(&id),
                    players
                )
            }
            _ => return Err(format!("Unknown bossbar function: bossbar.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn is_valid_bossbar_color(value: &str) -> bool {
        matches!(
            value,
            "pink" | "blue" | "red" | "green" | "yellow" | "purple" | "white"
        )
    }

    fn is_valid_bossbar_style(value: &str) -> bool {
        matches!(
            value,
            "progress" | "notched_6" | "notched_10" | "notched_12" | "notched_20"
        )
    }

    fn process_team_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "add" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("team.add() takes 1 or 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                if args.len() == 2 {
                    let display = self.expr_to_text_component(&args[1])?;
                    format!("team add {} {}", name, display)
                } else {
                    format!("team add {}", name)
                }
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("team.remove() takes 1 argument".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                format!("team remove {}", name)
            }
            "join" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("team.join() takes 1 or 2 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                if args.len() == 2 {
                    let members = self.expr_to_selector_arg(&args[1], "team members")?;
                    format!("team join {} {}", name, members)
                } else {
                    format!("team join {}", name)
                }
            }
            "leave" => {
                if args.len() != 1 {
                    return Err("team.leave() takes 1 argument".into());
                }
                let members = self.expr_to_selector_arg(&args[0], "team members")?;
                format!("team leave {}", members)
            }
            "modify" => {
                if args.len() != 3 {
                    return Err("team.modify() takes 3 arguments".into());
                }
                let name = self.expr_to_plain_arg(&args[0], "team name")?;
                let option = self.expr_to_plain_arg(&args[1], "team option")?;
                let value = match option.as_str() {
                    "prefix" | "suffix" | "displayName" => self.expr_to_text_component(&args[2])?,
                    _ => self.expr_to_plain_arg(&args[2], "team option value")?,
                };
                format!("team modify {} {} {}", name, option, value)
            }
            _ => return Err(format!("Unknown team function: team.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_entity_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        let command = match method {
            "tag_add" | "tag_remove" => {
                if args.len() != 2 {
                    return Err(format!("entity.{}() takes 2 arguments", method).into());
                }
                let target = self.expr_to_selector_arg(&args[0], "entity target")?;
                let tag = self.expr_to_plain_arg(&args[1], "entity tag")?;
                let action = method.strip_prefix("tag_").unwrap();
                format!("tag {} {} {}", target, action, tag)
            }
            "effect_give" => {
                if !(2..=5).contains(&args.len()) {
                    return Err("entity.effect_give() takes 2 to 5 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "effect target")?;
                let effect = self.expr_to_plain_arg(&args[1], "effect id")?;
                let mut command = format!("effect give {} {}", target, effect);
                if args.len() >= 3 {
                    command.push(' ');
                    command.push_str(&self.expr_to_plain_arg(&args[2], "effect duration")?);
                }
                if args.len() >= 4 {
                    command.push(' ');
                    command.push_str(&self.expr_to_i32(&args[3], "effect amplifier")?.to_string());
                }
                if args.len() == 5 {
                    command.push(' ');
                    command.push_str(
                        &self
                            .expr_to_bool(&args[4], "effect hide particles")?
                            .to_string(),
                    );
                }
                command
            }
            "effect_clear" => {
                if !(1..=2).contains(&args.len()) {
                    return Err("entity.effect_clear() takes 1 or 2 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "effect target")?;
                if args.len() == 2 {
                    let effect = self.expr_to_plain_arg(&args[1], "effect id")?;
                    format!("effect clear {} {}", target, effect)
                } else {
                    format!("effect clear {}", target)
                }
            }
            "attribute_get" => {
                if !(2..=3).contains(&args.len()) {
                    return Err("entity.attribute_get() takes 2 or 3 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "attribute target")?;
                let attribute = self.expr_to_plain_arg(&args[1], "attribute id")?;
                if args.len() == 3 {
                    let scale = self.expr_to_plain_arg(&args[2], "attribute scale")?;
                    format!("attribute {} {} get {}", target, attribute, scale)
                } else {
                    format!("attribute {} {} get", target, attribute)
                }
            }
            "attribute_base_set" => {
                if args.len() != 3 {
                    return Err("entity.attribute_base_set() takes 3 arguments"
                        .to_string()
                        .into());
                }
                let target = self.expr_to_selector_arg(&args[0], "attribute target")?;
                let attribute = self.expr_to_plain_arg(&args[1], "attribute id")?;
                let value = self.expr_to_plain_arg(&args[2], "attribute value")?;
                format!("attribute {} {} base set {}", target, attribute, value)
            }
            "teleport" => {
                if args.len() != 2 {
                    return Err("entity.teleport() takes 2 arguments".into());
                }
                let target = self.expr_to_selector_arg(&args[0], "teleport target")?;
                let destination = self.expr_to_plain_arg(&args[1], "teleport destination")?;
                format!("tp {} {}", target, destination)
            }
            _ => return Err(format!("Unknown entity function: entity.{}", method).into()),
        };
        self.push_current_command(command)
    }

    fn process_datapack_intrinsic(
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

    fn process_resource_pack_intrinsic(
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

    fn add_resource_pack_model(
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
                namespace,
                format!("models/{}/{}", model_type, name),
                content,
                self.current_statement_source.clone(),
            )
            .map_err(|message| {
                TranspileError::new(TranspileErrorKind::ResourcePackDuplicate, message)
            })
    }

    fn validate_resource_pack_model_json(
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

    fn require_optional_json_string(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_string()) {
            return Err(format!("{context} field '{field}' must be a string").into());
        }
        Ok(())
    }

    fn require_optional_json_object(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_object()) {
            return Err(format!("{context} field '{field}' must be an object").into());
        }
        Ok(())
    }

    fn require_optional_json_array(
        object: &serde_json::Map<String, serde_json::Value>,
        field: &str,
        context: &str,
    ) -> Result<(), TranspileError> {
        if object.get(field).is_some_and(|value| !value.is_array()) {
            return Err(format!("{context} field '{field}' must be an array").into());
        }
        Ok(())
    }

    fn add_resource_pack_lang(&mut self, args: &[Expression]) -> Result<(), TranspileError> {
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

    fn add_datapack_tag_resource(
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
        self.data_pack
            .add_json_resource_in_namespace_with_source(
                namespace,
                format!("tags/{}/{}", tag_type, name),
                content,
                self.current_statement_source.clone(),
            )
            .map_err(TranspileError::from)
    }

    fn validate_datapack_tag_value(&self, value: &serde_json::Value) -> Result<(), TranspileError> {
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

    fn add_datapack_json_resource(
        &mut self,
        resource_type: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err(format!("datapack.{}() takes 2 arguments", resource_type).into());
        }

        let (namespace, name) = self.expr_to_resource_id(&args[0], "resource name")?;
        let json = self.expr_to_json_value(&args[1])?;
        if !json.is_object() {
            return Err(
                format!("datapack.{}() JSON value must be an object", resource_type).into(),
            );
        }
        self.check_datapack_resource_format(resource_type, &json)?;
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

    /// Minecraft rejects data pack resources written in pre-26.3 shapes, and a
    /// server whose packs fail to load refuses to start, so the common
    /// renames are reported at compile time.
    fn check_datapack_resource_format(
        &self,
        resource_type: &str,
        json: &serde_json::Value,
    ) -> Result<(), TranspileError> {
        let mut problems = Vec::new();
        match resource_type {
            "predicate" => resource_format::check_predicate(json, "", &mut problems),
            "item_modifier" => resource_format::check_item_modifier(json, "", &mut problems),
            "loot_table" => resource_format::check_loot_table(json, &mut problems),
            "advancement" => resource_format::check_advancement(json, &mut problems),
            "recipe" => resource_format::check_recipe(json, &mut problems),
            _ => {}
        }
        if problems.is_empty() {
            return Ok(());
        }
        let mut message = format!(
            "datapack.{}() uses fields Minecraft {} rejects or ignores (a server refuses to start when a pack fails to load):\n{}",
            resource_type,
            crate::pack_format::SUPPORTED_MINECRAFT_VERSION,
            problems
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

    fn expr_to_resource_id(
        &self,
        expr: &Expression,
        label: &str,
    ) -> Result<(Option<String>, String), TranspileError> {
        let id = self.expr_to_plain_arg(expr, label)?;
        self.plain_resource_id_to_parts(&id, label)
    }

    fn plain_resource_id_to_parts(
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

    fn process_math_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        match method {
            "sqrt" => {
                if args.len() != 1 {
                    return Err("math.sqrt() takes 1 argument".into());
                }
                self.process_math_unary_op(args, "sqrt")
            }
            "abs" => {
                if args.len() != 1 {
                    return Err("math.abs() takes 1 argument".into());
                }
                self.process_math_unary_op(args, "abs")
            }
            "min" => {
                if args.len() != 2 {
                    return Err("math.min() takes 2 arguments".into());
                }
                self.process_math_binary_op(args, "min")
            }
            "max" => {
                if args.len() != 2 {
                    return Err("math.max() takes 2 arguments".into());
                }
                self.process_math_binary_op(args, "max")
            }
            _ => Err(format!("Unknown math function: math.{}", method).into()),
        }
    }

    fn process_math_unary_op(
        &mut self,
        args: &[Expression],
        op: &str,
    ) -> Result<(), TranspileError> {
        let input_var = "#math_input";
        // output_var is implied to be "#math_result" by convention in helpers

        self.data_pack.track_objective("temp");
        self.variable_objectives
            .insert(input_var.to_string(), "temp".to_string());
        self.scoreboard_variables.insert(input_var.to_string());

        // Evaluate argument
        let eval_commands = self.evaluate_expression_to_target(&args[0], input_var)?;

        // Ensure helper exists
        self.ensure_math_helper(op);

        if let Some(ref mut commands) = self.current_function {
            commands.extend(eval_commands);
            commands.push(format!(
                "function {}:_cobble_math_{}",
                self.data_pack.namespace, op
            ));
        }
        Ok(())
    }

    fn process_math_binary_op(
        &mut self,
        args: &[Expression],
        op: &str,
    ) -> Result<(), TranspileError> {
        let input1 = "#math_input";
        let input2 = "#math_input2";

        self.data_pack.track_objective("temp");
        self.variable_objectives
            .insert(input1.to_string(), "temp".to_string());
        self.variable_objectives
            .insert(input2.to_string(), "temp".to_string());
        self.scoreboard_variables.insert(input1.to_string());
        self.scoreboard_variables.insert(input2.to_string());

        // Evaluating an argument can itself run a math helper (a nested
        // math.* call), which stages into the shared #math_input scores and
        // would clobber an argument staged earlier. Stage each argument into
        // a unique scratch and copy into the helper inputs right before the
        // call.
        let arg1_scratch = format!("#math_arg_{}", self.next_temp_id());
        let arg2_scratch = format!("#math_arg_{}", self.next_temp_id());

        let mut cmds = Vec::new();
        cmds.extend(self.evaluate_expression_to_target(&args[0], &arg1_scratch)?);
        cmds.extend(self.evaluate_expression_to_target(&args[1], &arg2_scratch)?);
        cmds.push(format!(
            "scoreboard players operation {} temp = {} temp",
            input1, arg1_scratch
        ));
        cmds.push(format!(
            "scoreboard players operation {} temp = {} temp",
            input2, arg2_scratch
        ));

        self.ensure_math_helper(op);

        if let Some(ref mut commands) = self.current_function {
            commands.extend(cmds);
            commands.push(format!(
                "function {}:_cobble_math_{}",
                self.data_pack.namespace, op
            ));
        }
        Ok(())
    }

    fn ensure_math_helper(&mut self, op: &str) {
        self.data_pack.ensure_math_helper(op);
    }

    fn generate_function_call(
        &mut self,
        func_name: &str,
        args: &[Expression],
    ) -> Result<(), TranspileError> {
        self.record_user_function_call(func_name)?;
        let mut commands = Vec::new();

        if let Some(param_names) = self.function_params.get(func_name).cloned() {
            // Validate argument count matches parameter count (a function
            // without parameters takes no arguments either)
            if args.len() != param_names.len() {
                return Err(format!(
                    "Function '{}' expects {} argument(s), but {} provided.\n\
                    Expected parameters: ({})",
                    func_name,
                    param_names.len(),
                    args.len(),
                    param_names.join(", ")
                )
                .into());
            }

            // If function has parameters, use macro system
            if !param_names.is_empty() && !args.is_empty() {
                let preserve_caller_args = !self.current_context.params.is_empty();
                let staged_args_path = {
                    let id = self.next_temp_id();
                    let call_path = format!("_call_args_{}", id);
                    let caller_path = if preserve_caller_args {
                        let caller_path = format!("_caller_args_{}", id);
                        commands.push(format!(
                            "data remove storage {}:global {}",
                            self.data_pack.namespace, caller_path
                        ));
                        commands.push(format!(
                            "data modify storage {}:global {} set from storage {}:global args",
                            self.data_pack.namespace, caller_path, self.data_pack.namespace
                        ));
                        Some(caller_path)
                    } else {
                        None
                    };
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        self.data_pack.namespace, call_path
                    ));
                    Some((call_path, caller_path))
                };

                // Store arguments in storage for macro substitution
                for (i, arg) in args.iter().enumerate() {
                    if i < param_names.len() {
                        let param_name = &param_names[i];
                        let target_path = staged_args_path
                            .as_ref()
                            .map(|(path, _)| format!("{}.{}", path, param_name))
                            .unwrap_or_else(|| format!("args.{}", param_name));
                        match arg {
                            Expression::String(s) => {
                                // Escape quotes, backslashes, and special characters in the string
                                let escaped = s
                                    .replace('\\', "\\\\")
                                    .replace('"', "\\\"")
                                    .replace('\n', "\\n")
                                    .replace('\r', "\\r")
                                    .replace('\t', "\\t");
                                commands.push(format!(
                                    "data modify storage {}:global {} set value \"{}\"",
                                    self.data_pack.namespace, target_path, escaped
                                ));
                            }
                            Expression::Number(n) => {
                                commands.push(format!(
                                    "data modify storage {}:global {} set value {}",
                                    self.data_pack.namespace, target_path, *n as i32
                                ));
                            }
                            Expression::Identifier(var) => {
                                // Check if this is a selector (@...) or literal string (like item names)
                                if var.starts_with('@') {
                                    // Selector - store as string
                                    commands.push(format!(
                                        "data modify storage {}:global {} set value \"{}\"",
                                        self.data_pack.namespace, target_path, var
                                    ));
                                } else if self.is_current_macro_param(var) {
                                    commands.push(format!(
                                        "data modify storage {}:global {} set from storage {}:global args.{}",
                                        self.data_pack.namespace,
                                        target_path,
                                        self.data_pack.namespace,
                                        var
                                    ));
                                } else if let Some(source_path) =
                                    self.variable_storage_paths.get(var).cloned()
                                {
                                    // Storage-backed string/list/map: pass its
                                    // value, not the variable's name.
                                    commands.push(format!(
                                        "data modify storage {}:global {} set from storage {}:global {}",
                                        self.data_pack.namespace,
                                        target_path,
                                        self.data_pack.namespace,
                                        source_path
                                    ));
                                } else if self.scoreboard_variables.contains(var)
                                    || self.variable_objectives.contains_key(var)
                                {
                                    // Scoreboard variable - read from scoreboard
                                    let obj = self
                                        .variable_objectives
                                        .get(var)
                                        .unwrap_or(&"temp".to_string())
                                        .clone();
                                    commands.push(format!(
                                        "execute store result storage {}:global {} int 1 run scoreboard players get {} {}",
                                        self.data_pack.namespace, target_path, var, obj
                                    ));
                                } else if let Some(_const_val) = self.variables.get(var) {
                                    // Constant variable - try to get compile-time constant value
                                    if let Some(&val) = self.compile_time_constants.get(var) {
                                        commands.push(format!(
                                            "data modify storage {}:global {} set value {}",
                                            self.data_pack.namespace, target_path, val
                                        ));
                                    } else {
                                        // Const variable but not a simple constant - treat as identifier string
                                        commands.push(format!(
                                            "data modify storage {}:global {} set value \"{}\"",
                                            self.data_pack.namespace, target_path, var
                                        ));
                                    }
                                } else {
                                    // Unknown identifier: silently passing it through as a
                                    // string literal hides typos from library callers, so
                                    // fail loudly instead (the CLI diagnostics catch this
                                    // earlier with `undefined-variable`).
                                    return Err(format!(
                                        "Unknown identifier '{}' passed as argument to function '{}'. Define the variable first, or quote it (\"{}\") to pass a string literal.",
                                        var, func_name, var
                                    ).into());
                                }
                            }
                            Expression::Boolean(b) => {
                                // Boolean literal - store as 1 (true) or 0 (false)
                                let value = if *b { 1 } else { 0 };
                                commands.push(format!(
                                    "data modify storage {}:global {} set value {}",
                                    self.data_pack.namespace, target_path, value
                                ));
                            }
                            Expression::Binary(_, _, _)
                            | Expression::Unary(_, _)
                            | Expression::Call(_, _)
                            | Expression::Attribute(_, _)
                            | Expression::Subscript(_, _) => {
                                // Arithmetic or condition expression: evaluate into a
                                // per-call scratch score, then store it. The name is
                                // unique so calls passing differently typed values
                                // (`f(x + 1)`, then `f(a > b)`) never share a slot.
                                let temp_var = format!("#arg_temp_{}", self.next_temp_id());

                                // Track the temp variable
                                self.data_pack.track_objective("temp");
                                self.variable_objectives
                                    .insert(temp_var.clone(), "temp".to_string());
                                self.scoreboard_variables.insert(temp_var.clone());

                                // Evaluate the expression into the temp variable
                                let assignment = Assignment {
                                    target: temp_var.clone(),
                                    target_expr: None,
                                    value: arg.clone(),
                                    persist: false,
                                };
                                self.process_assignment(&assignment)?;

                                // Now store the temp variable value into storage
                                commands.push(format!(
                                    "execute store result storage {}:global {} int 1 run scoreboard players get {} temp",
                                    self.data_pack.namespace, target_path, temp_var
                                ));
                            }
                            _ => {
                                // For other complex expressions (Call, etc.)
                                return Err(format!(
                                    "Function parameter does not support this expression type.\n\
                                     \n\
                                     Parameter '{}' received: {:?}\n\
                                     \n\
                                     Supported parameter types:\n\
                                     - Literal numbers: func(42)\n\
                                     - Literal strings: func(\"text\")\n\
                                     - Boolean values: func(True), func(False)\n\
                                     - Variables: func(my_var)\n\
                                     - Arithmetic expressions: func(x + 10)\n\
                                     \n\
                                     Not supported:\n\
                                     - Arrays/maps as direct macro arguments",
                                    param_name, arg
                                )
                                .into());
                            }
                        }
                    }
                }
                if let Some((staged_args_path, _)) = staged_args_path.as_ref() {
                    commands.push(format!(
                        "data modify storage {}:global args set from storage {}:global {}",
                        self.data_pack.namespace, self.data_pack.namespace, staged_args_path
                    ));
                }

                // Generate macro call with storage (only when parameters are provided)
                commands.push(format!(
                    "function {}:{} with storage {}:global args",
                    self.data_pack.namespace, func_name, self.data_pack.namespace
                ));

                if let Some((staged_args_path, caller_args_path)) = staged_args_path {
                    if let Some(caller_args_path) = caller_args_path {
                        commands.push(format!(
                            "data modify storage {}:global args set from storage {}:global {}",
                            self.data_pack.namespace, self.data_pack.namespace, caller_args_path
                        ));
                        commands.push(format!(
                            "data remove storage {}:global {}",
                            self.data_pack.namespace, caller_args_path
                        ));
                    }
                    commands.push(format!(
                        "data remove storage {}:global {}",
                        self.data_pack.namespace, staged_args_path
                    ));
                }
            } else {
                // Function has no parameters or no arguments provided - regular call
                commands.push(format!(
                    "function {}:{}",
                    self.data_pack.namespace, func_name
                ));
            }
        } else {
            // Function not found in params map - regular function call
            commands.push(format!(
                "function {}:{}",
                self.data_pack.namespace, func_name
            ));
        }

        if let Some(ref mut func_commands) = self.current_function {
            func_commands.extend(commands);
        }

        Ok(())
    }

    fn process_add_event_listener(&mut self, args: &[Expression]) -> Result<(), TranspileError> {
        if args.len() != 2 {
            return Err("addEventListener requires 2 arguments: (event, handler)"
                .to_string()
                .into());
        }

        // Extract event type
        let event_type = match &args[0] {
            Expression::Attribute(obj, attr) => {
                // Handle event.LOAD, event.TICK
                if let Expression::Identifier(module) = &**obj {
                    if module == "event" {
                        match attr.as_str() {
                            "LOAD" => EventType::Load,
                            "TICK" => EventType::Tick,
                            _ => return Err(format!("Unknown event type: {}", attr).into()),
                        }
                    } else {
                        return Err(format!("Unknown module: {}", module).into());
                    }
                } else {
                    return Err("Event must be from 'event' module".into());
                }
            }
            _ => {
                return Err("First argument must be an event (e.g., event.LOAD)"
                    .to_string()
                    .into())
            }
        };

        // Extract handler function name
        let handler_name = match &args[1] {
            Expression::Identifier(name) => name.clone(),
            _ => return Err("Second argument must be a function name".into()),
        };

        // Register the event listener
        self.data_pack
            .stdlib
            .add_event_listener(event_type, handler_name);

        Ok(())
    }

    fn preprocess_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<Expression, TranspileError> {
        // Boolean literals read the `#true_const`/`#false_const` scores.
        if Self::contains_boolean_literal(condition) {
            self.data_pack.track_boolean_constants();
        }

        // Check if the condition has a complex expression on the left side of a comparison
        // For example: (x % 3) == 1 or (x ^ 2) > 10
        match condition {
            Expression::Binary(left, op, right) => {
                // Check if this is a comparison operator and left is a binary expression
                let is_comparison = matches!(
                    op,
                    BinaryOp::Eq
                        | BinaryOp::NotEq
                        | BinaryOp::Lt
                        | BinaryOp::LtEq
                        | BinaryOp::Gt
                        | BinaryOp::GtEq
                );

                if is_comparison {
                    // Evaluate each complex side (arithmetic, calls, storage
                    // reads, macro parameters) into its own unique temp, left
                    // first so side effects keep source order. Both sides are
                    // checked: `actual == expected` with two parameters must
                    // not leave the right one as an unset score name.
                    let new_left = if let Some(folded) = self.fold_condition_operand(left) {
                        folded
                    } else if self.condition_operand_needs_temp(left) {
                        self.lower_condition_value_to_temp(left)?
                    } else {
                        (**left).clone()
                    };
                    let new_right = if let Some(folded) = self.fold_condition_operand(right) {
                        folded
                    } else if self.condition_operand_needs_temp(right) {
                        self.lower_condition_value_to_temp(right)?
                    } else {
                        (**right).clone()
                    };
                    Ok(Expression::Binary(
                        Box::new(new_left),
                        op.clone(),
                        Box::new(new_right),
                    ))
                } else {
                    // Not a comparison, might be And/Or - recursively preprocess
                    match op {
                        BinaryOp::And | BinaryOp::Or => {
                            let new_left = self.preprocess_condition(left)?;
                            let new_right = self.preprocess_condition(right)?;
                            Ok(Expression::Binary(
                                Box::new(new_left),
                                op.clone(),
                                Box::new(new_right),
                            ))
                        }
                        _ => {
                            // Arithmetic expressions in condition position use numeric truthiness.
                            self.lower_condition_value_to_temp(condition)
                        }
                    }
                }
            }
            Expression::Unary(op, expr) => {
                // Recursively preprocess the inner expression
                let new_expr = self.preprocess_condition(expr)?;
                Ok(Expression::Unary(op.clone(), Box::new(new_expr)))
            }
            expr if self.condition_operand_needs_temp(expr) => {
                self.lower_condition_value_to_temp(expr)
            }
            _ => Ok(condition.clone()),
        }
    }

    /// Folds a literal-only comparison operand, such as `step - 1` inside an
    /// unrolled loop, so it needs no scratch score. Integer `/` and `%` fold
    /// with the same floor semantics as the scoreboard.
    fn fold_condition_operand(&self, expr: &Expression) -> Option<Expression> {
        if !matches!(expr, Expression::Binary(..) | Expression::Unary(..)) {
            return None;
        }
        let value = self.try_eval_const(expr)?;
        (value.fract() == 0.0 && value.abs() <= f64::from(i32::MAX))
            .then_some(Expression::Number(value))
    }

    fn lower_condition_value_to_temp(
        &mut self,
        expr: &Expression,
    ) -> Result<Expression, TranspileError> {
        let temp_var = format!("expr_cond_temp_{}", self.next_temp_id());

        self.data_pack.track_objective("temp");
        self.variable_objectives
            .insert(temp_var.clone(), "temp".to_string());
        self.scoreboard_variables.insert(temp_var.clone());

        let eval_commands = self.evaluate_expression_to_target(expr, &temp_var)?;
        if let Some(ref mut commands) = self.current_function {
            commands.extend(eval_commands);
        }

        Ok(Expression::Identifier(temp_var))
    }

    fn condition_operand_needs_temp(&self, expr: &Expression) -> bool {
        matches!(
            expr,
            Expression::Binary(_, _, _)
                | Expression::Unary(_, _)
                | Expression::Call(_, _)
                | Expression::Attribute(_, _)
                | Expression::Subscript(_, _)
        ) || matches!(expr, Expression::Identifier(name) if self.is_current_macro_param(name))
    }

    /// Check if an expression contains any Boolean literals (True or False)
    fn contains_boolean_literal(expr: &Expression) -> bool {
        match expr {
            Expression::Boolean(_) => true,
            Expression::Binary(left, _, right) => {
                Self::contains_boolean_literal(left) || Self::contains_boolean_literal(right)
            }
            Expression::Unary(_, inner) => Self::contains_boolean_literal(inner),
            _ => false,
        }
    }

    pub(in crate::transpiler) fn capture_preprocessed_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<(TranslatedCondition, FunctionCapture), TranspileError> {
        self.capture_commands_with_result(|transpiler| {
            let processed_condition = transpiler.preprocess_condition(condition)?;
            transpiler.translate_condition(&processed_condition)
        })
    }

    pub(in crate::transpiler) fn capture_raw_python_condition(
        &mut self,
        condition: &str,
    ) -> Result<Option<(TranslatedCondition, FunctionCapture)>, TranspileError> {
        if !self.looks_like_python_expression(condition) {
            return Ok(None);
        }

        // Selector operands (`@s > 5`) and macro-parameter braces
        // (`{count} >= 3`) are valid in raw execute conditions but cannot be
        // parsed as Python expressions; mask them with synthetic identifiers
        // and restore the original spelling after translation.
        let (masked, operand_tokens) = Self::mask_condition_operand_tokens(condition);
        let Some(expr) = Self::parse_raw_condition_expression(&masked) else {
            return Ok(None);
        };

        let (translated, mut capture) = self
            .capture_preprocessed_condition(&expr)
            .map_err(|error| Self::restore_condition_operand_tokens(error, &operand_tokens))?;
        let mut translated = Self::fold_always_false_conditions(translated);
        for (placeholder, original) in &operand_tokens {
            translated.map_simple(&|text| text.replace(placeholder.as_str(), original));
            for command in &mut capture.commands {
                *command = command.replace(placeholder, original);
            }
            for generated in capture.metadata.values_mut() {
                generated.text = generated.text.replace(placeholder, original);
            }
        }
        Ok(Some((translated, capture)))
    }

    /// Restore the original spelling of masked condition operands in an error
    /// message, so translation failures report `@s`/`{count}` instead of
    /// leaking synthetic `__cobble_cond_operand_N__` placeholder names.
    fn restore_condition_operand_tokens(
        mut error: TranspileError,
        operand_tokens: &[(String, String)],
    ) -> TranspileError {
        for (placeholder, original) in operand_tokens {
            error.message = error.message.replace(placeholder, original);
        }
        error
    }

    /// Replace selector tokens (`@s`, `@a[tag=x]`, ...) and macro-parameter
    /// braces (`{count}`) in a raw condition string with synthetic
    /// identifiers so the condition can be parsed as a Python expression.
    /// Returns the masked string plus (placeholder, original) pairs used to
    /// restore the original spelling in the translated condition and any
    /// captured commands.
    fn mask_condition_operand_tokens(condition: &str) -> (String, Vec<(String, String)>) {
        let bytes = condition.as_bytes();
        let mut masked = String::with_capacity(condition.len());
        let mut operand_tokens: Vec<(String, String)> = Vec::new();
        let mut i = 0;
        while i < bytes.len() {
            let ch = bytes[i] as char;
            if ch == '"' || ch == '\'' {
                let quote = bytes[i];
                masked.push(ch);
                i += 1;
                while i < bytes.len() {
                    let c = bytes[i];
                    masked.push(c as char);
                    i += 1;
                    if c == b'\\' && i < bytes.len() {
                        masked.push(bytes[i] as char);
                        i += 1;
                    } else if c == quote {
                        break;
                    }
                }
                continue;
            }

            let token_start = i;
            if ch == '@' && i + 1 < bytes.len() && bytes[i + 1].is_ascii_alphabetic() {
                i += 1;
                while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                    i += 1;
                }
                if i < bytes.len() && bytes[i] == b'[' {
                    let Some(end) = Self::selector_bracket_end(condition, i) else {
                        // Unbalanced selector brackets; keep the rest verbatim.
                        masked.push_str(&condition[token_start..]);
                        break;
                    };
                    i = end;
                }
            } else if ch == '{' {
                let mut j = i + 1;
                while j < bytes.len() && (bytes[j].is_ascii_alphanumeric() || bytes[j] == b'_') {
                    j += 1;
                }
                if j > i + 1 && j < bytes.len() && bytes[j] == b'}' {
                    i = j + 1;
                } else {
                    masked.push(ch);
                    i += 1;
                    continue;
                }
            } else {
                masked.push(ch);
                i += 1;
                continue;
            }

            let original = condition[token_start..i].to_string();
            let placeholder = format!("__cobble_cond_operand_{}__", operand_tokens.len());
            masked.push_str(&placeholder);
            operand_tokens.push((placeholder, original));
        }
        (masked, operand_tokens)
    }

    /// Byte index just past the `]` closing the selector bracket group that
    /// opens at `open`, honoring quoted strings inside the brackets.
    fn selector_bracket_end(condition: &str, open: usize) -> Option<usize> {
        let bytes = condition.as_bytes();
        let mut depth = 0usize;
        let mut i = open;
        while i < bytes.len() {
            match bytes[i] {
                b'"' | b'\'' => {
                    let quote = bytes[i];
                    i += 1;
                    while i < bytes.len() && bytes[i] != quote {
                        if bytes[i] == b'\\' {
                            i += 1;
                        }
                        i += 1;
                    }
                    if i >= bytes.len() {
                        return None;
                    }
                }
                b'[' => depth += 1,
                b']' => {
                    depth -= 1;
                    if depth == 0 {
                        return Some(i + 1);
                    }
                }
                _ => {}
            }
            i += 1;
        }
        None
    }

    /// Map the AST translator's inline always-false output
    /// (`score X O matches 0 unless score X O matches 0`, produced for
    /// comparisons like `x > i32::MAX`) onto `TranslatedCondition::AlwaysFalse`,
    /// including inside OR alternatives. This lets the execute processor keep
    /// its simplifications (dropping impossible branches, `unless`-elision)
    /// for raw execute conditions.
    fn fold_always_false_conditions(translated: TranslatedCondition) -> TranslatedCondition {
        match translated {
            TranslatedCondition::Simple(text) if Self::is_inline_always_false_condition(&text) => {
                TranslatedCondition::AlwaysFalse
            }
            TranslatedCondition::AnyOf(left, right) => TranslatedCondition::AnyOf(
                Box::new(Self::fold_always_false_conditions(*left)),
                Box::new(Self::fold_always_false_conditions(*right)),
            ),
            other => other,
        }
    }

    fn is_inline_always_false_condition(condition: &str) -> bool {
        let Some(rest) = condition.strip_prefix("score ") else {
            return false;
        };
        let Some((head, tail)) = rest.split_once(" matches 0 unless score ") else {
            return false;
        };
        tail.strip_suffix(" matches 0") == Some(head)
    }

    fn parse_raw_condition_expression(condition: &str) -> Option<Expression> {
        let source = format!(
            "def __cobble_condition__():\n    if {}:\n        pass\n",
            condition
        );
        let program = crate::parser::parse(&source).ok()?;
        let Statement::FunctionDef(function) = program.statements.first()? else {
            return None;
        };
        let Statement::If(if_stmt) = function.body.first()? else {
            return None;
        };
        Some(if_stmt.condition.clone())
    }

    fn translate_condition(
        &mut self,
        condition: &Expression,
    ) -> Result<TranslatedCondition, TranspileError> {
        let translator = ConditionTranslator::new(
            &self.variable_objectives,
            &self.compile_time_constants,
            &self.variable_storage_paths,
            &self.variable_types,
        );
        let translated = translator.translate(condition)?;

        // Literal comparisons, boolean literals, constants, and negated
        // always-false conditions fold to the internal `#true_const` /
        // `#false_const` scores; register their objective whenever the
        // translated condition reads them.
        let mut reads_constants = false;
        translated.for_each_simple(&mut |text| {
            reads_constants |= text.contains("#true_const ") || text.contains("#false_const ");
        });
        if reads_constants {
            self.data_pack.track_boolean_constants();
        }
        Ok(translated)
    }

    /// Lower a translated condition to final execute-condition text at this
    /// emission point, materializing OR trees into `or_temp_N` flag variables
    /// in the current function.
    pub(in crate::transpiler) fn lower_translated_condition(
        &mut self,
        condition: TranslatedCondition,
    ) -> Result<String, TranspileError> {
        match condition {
            TranslatedCondition::Simple(text) => Ok(text),
            TranslatedCondition::AnyOf(..) => self.materialize_or_condition(condition),
            TranslatedCondition::AllOf(left, right) => {
                // Lower each side (materializing OR sides first), then chain
                // the results with "if ... if ...".
                let left = self.lower_translated_condition(*left)?;
                let right = self.lower_translated_condition(*right)?;
                Ok(format!(
                    "{} {}",
                    Self::condition_execute_args(&left),
                    Self::condition_execute_args(&right)
                ))
            }
            TranslatedCondition::AlwaysFalse => {
                self.data_pack.track_objective("temp");
                Ok(format!(
                    "score {holder} temp matches 0 unless score {holder} temp matches 0",
                    holder = "#cobble_always_false"
                ))
            }
        }
    }

    /// Lower an OR tree to a fresh flag variable: reset the flag, set it to 1
    /// under each alternative, and return the condition that reads it back.
    fn materialize_or_condition(
        &mut self,
        condition: TranslatedCondition,
    ) -> Result<String, TranspileError> {
        let mut leaves = Vec::new();
        self.flatten_or_leaves(condition, &mut leaves)?;

        // Use unique variable name to prevent race conditions with multiple entities
        let or_var = format!("or_temp_{}", self.next_temp_id());

        // Track the temp objective for or result
        self.data_pack.track_objective("temp");

        // Generate commands for OR logic
        if let Some(ref mut commands) = self.current_function {
            // Initialize or variable to 0
            commands.push(format!("scoreboard players set {} temp 0", or_var));

            // For each condition, if true, set or variable to 1
            for leaf in &leaves {
                let TranslatedCondition::Simple(cond) = leaf else {
                    continue;
                };
                commands.push(format!(
                    "execute {} run scoreboard players set {} temp 1",
                    Self::condition_execute_args(cond),
                    or_var
                ));
            }
        }

        // Return the condition that checks if or variable is 1
        Ok(format!("score {} temp matches 1", or_var))
    }

    /// Flatten an OR tree into its alternatives, in source order. Every
    /// returned leaf is `Simple` or `AlwaysFalse`: structured leaves (an AND
    /// or NOT around a nested OR) are lowered to a `Simple` condition first.
    pub(in crate::transpiler) fn flatten_or_leaves(
        &mut self,
        condition: TranslatedCondition,
        leaves: &mut Vec<TranslatedCondition>,
    ) -> Result<(), TranspileError> {
        match condition {
            TranslatedCondition::AnyOf(left, right) => {
                self.flatten_or_leaves(*left, leaves)?;
                self.flatten_or_leaves(*right, leaves)?;
            }
            leaf @ (TranslatedCondition::Simple(_) | TranslatedCondition::AlwaysFalse) => {
                leaves.push(leaf);
            }
            structured => {
                let rendered = self.lower_translated_condition(structured)?;
                leaves.push(TranslatedCondition::Simple(rendered));
            }
        }
        Ok(())
    }

    pub fn write_data_pack(&self) -> std::io::Result<()> {
        self.data_pack.write()
    }

    /// Check if a condition string looks like a Python expression
    fn looks_like_python_expression(&self, condition: &str) -> bool {
        // The raw-condition whitelist must be checked BEFORE the Python
        // heuristics below: raw conditions may legitimately contain
        // parentheses or operator-like text (selector `name="Bob (Admin)"`,
        // SNBT strings), which would otherwise misroute them into Python
        // translation.
        if let Some(is_raw) = raw_minecraft_condition_verdict(condition) {
            return !is_raw;
        }

        // Check for Python expression indicators
        let python_indicators = [
            " >= ", " <= ", " == ", " != ", " > ", " < ", " and ", " or ", " not ",
        ];
        for indicator in &python_indicators {
            if condition.contains(indicator) {
                return true;
            }
        }

        if condition.starts_with("not ") || condition.contains('(') {
            return true;
        }

        // Check if it's a simple variable name (also a Python expression)
        if condition.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Check if it's a known variable
            return self.variables.contains_key(condition)
                || self.scoreboard_variables.contains(condition);
        }

        false
    }
}
