// Module declarations
mod command_processor;
mod condition_translator;
mod data_pack;
mod expression_evaluator;
mod statement_processors;

// Public exports
pub use data_pack::{
    DataPack, GeneratedCommand, GeneratedCommandKind, SourceLocation, SourceMap, SourceMapEntry,
};

use crate::ast::*;
use crate::stdlib::EventType;
use command_processor::CommandProcessor;
use condition_translator::ConditionTranslator;
use expression_evaluator::ExpressionEvaluator;
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::{Path, PathBuf};

/// Context for tracking function parameters and scope
#[derive(Clone)]
struct FunctionContext {
    params: Vec<String>,
}

pub(in crate::transpiler) struct FunctionCapture {
    pub(in crate::transpiler) commands: Vec<String>,
    pub(in crate::transpiler) metadata: HashMap<usize, GeneratedCommand>,
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

    pub(in crate::transpiler) fn with_extra_param(&self, param: String) -> Vec<String> {
        let mut params = self.params.clone();
        if !params.contains(&param) {
            params.push(param);
        }
        params
    }
}

impl FunctionCapture {
    pub(in crate::transpiler) fn requires_macro_context(&self) -> bool {
        self.commands
            .iter()
            .any(|command| command.trim_start().starts_with('$'))
    }
}

pub struct Transpiler {
    pub data_pack: DataPack,
    current_function: Option<Vec<String>>,
    current_function_metadata: Option<HashMap<usize, GeneratedCommand>>,
    current_context: FunctionContext,
    variables: HashMap<String, Expression>,
    temp_counter: u32,
    variable_objectives: HashMap<String, String>, // Track which objective each variable uses
    scoreboard_variables: HashSet<String>, // Track variables backed by scoreboard (not constants)
    function_params: HashMap<String, Vec<String>>, // Track function parameter names
    global_variables: HashSet<String>,     // Track global variables declared in current function
    module_level_vars: Vec<(String, Expression, Option<SourceLocation>)>, // Store module-level assignments (Vec for ordering)
    compile_time_constants: HashMap<String, f64>, // Store compile-time constant values
    selector_aliases: HashMap<String, String>,    // Store selector definitions (@Name -> @a[...])
    entity_templates: HashMap<String, EntityDef>, // Store entity templates
    variable_storage_paths: HashMap<String, String>, // Track storage path for variables (e.g., "list" -> "vars.list")
    imported_files: HashSet<PathBuf>, // Track imported files to prevent circular dependencies
    import_stack: Vec<PathBuf>,       // Track current import chain for circular detection
    current_file_path: Option<PathBuf>,
    current_file_dir: PathBuf, // Current file's directory for resolving relative imports
    raw_command_locations: HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    statement_locations: HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    current_statement_source: Option<SourceLocation>,
    variable_types: HashMap<String, crate::ast::CobbleType>, // Track type of each variable for type checking
}

impl Transpiler {
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self {
            data_pack: DataPack::new(namespace, output_dir),
            current_function: None,
            current_function_metadata: None,
            current_context: FunctionContext::new(),
            variables: HashMap::new(),
            temp_counter: 0,
            variable_objectives: HashMap::new(),
            scoreboard_variables: HashSet::new(),
            function_params: HashMap::new(),
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
        }
    }

    pub fn set_current_file(&mut self, file_path: &Path) {
        if let Some(parent) = file_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }

        // Add main file to import stack and imported_files for circular import detection
        // Canonicalize if possible, otherwise use as-is
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        if !self.import_stack.contains(&canonical_path) {
            self.import_stack.push(canonical_path.clone());
            self.imported_files.insert(canonical_path.clone());
        }
        self.current_file_path = Some(canonical_path);
    }

    pub fn set_current_file_with_source(&mut self, file_path: &Path, source: &str) {
        self.set_current_file(file_path);
        self.register_source_locations(file_path, source);
    }

    fn register_source_locations(&mut self, file_path: &Path, source: &str) {
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        let mut raw_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();
        let mut statement_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();
        let mut pending_statement: Option<(String, SourceLocation, i32)> = None;

        for (line_index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            if let Some(command) = trimmed.strip_prefix('/') {
                let column = line.find('/').map(|index| index + 1).unwrap_or(1);
                raw_locations
                    .entry(command.trim().to_string())
                    .or_default()
                    .push_back(SourceLocation {
                        file: canonical_path.clone(),
                        line: line_index + 1,
                        column,
                    });
                continue;
            }

            let Some(statement_text) = Self::strip_inline_comment(trimmed) else {
                continue;
            };
            let key = Self::source_line_key(statement_text.trim());
            if key.is_empty() {
                continue;
            }

            let column = line
                .char_indices()
                .find(|(_, c)| !c.is_whitespace())
                .map(|(index, _)| index + 1)
                .unwrap_or(1);
            let source_location = SourceLocation {
                file: canonical_path.clone(),
                line: line_index + 1,
                column,
            };

            let depth_delta = Self::bracket_depth_delta(statement_text);
            if let Some((pending_source, pending_location, pending_depth)) =
                pending_statement.as_mut()
            {
                pending_source.push(' ');
                pending_source.push_str(statement_text);
                *pending_depth += depth_delta;
                if *pending_depth <= 0 {
                    let key = Self::source_line_key(pending_source.trim());
                    if !key.is_empty() {
                        statement_locations
                            .entry(key)
                            .or_default()
                            .push_back(pending_location.clone());
                    }
                    pending_statement = None;
                }
                continue;
            }

            if depth_delta > 0 {
                pending_statement =
                    Some((statement_text.to_string(), source_location, depth_delta));
                continue;
            }

            statement_locations
                .entry(key)
                .or_default()
                .push_back(source_location);
        }

        self.raw_command_locations
            .insert(canonical_path.clone(), raw_locations);
        self.statement_locations
            .insert(canonical_path, statement_locations);
    }

    fn take_raw_command_source(&mut self, command: &str) -> Option<SourceLocation> {
        let file_path = self.current_file_path.as_ref()?;
        self.raw_command_locations
            .get_mut(file_path)
            .and_then(|commands| commands.get_mut(command))
            .and_then(VecDeque::pop_front)
    }

    fn take_statement_source(&mut self, statement: &Statement) -> Option<SourceLocation> {
        let key = Self::statement_source_key(statement)?;
        let file_path = self.current_file_path.as_ref()?;
        self.statement_locations
            .get_mut(file_path)
            .and_then(|statements| statements.get_mut(&key))
            .and_then(VecDeque::pop_front)
    }

    fn strip_inline_comment(line: &str) -> Option<&str> {
        let mut quote: Option<char> = None;
        let mut escaped = false;

        for (index, ch) in line.char_indices() {
            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if let Some(active_quote) = quote {
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }

            match ch {
                '"' | '\'' => quote = Some(ch),
                '#' => return Some(&line[..index]),
                _ => {}
            }
        }

        Some(line)
    }

    fn normalize_source_key(source: &str) -> String {
        let mut normalized = String::new();
        let mut quote: Option<char> = None;
        let mut escaped = false;

        for ch in source.chars() {
            if escaped {
                normalized.push(ch);
                escaped = false;
                continue;
            }

            if ch == '\\' {
                normalized.push(ch);
                escaped = true;
                continue;
            }

            if let Some(active_quote) = quote {
                normalized.push(ch);
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }

            match ch {
                '"' | '\'' => {
                    quote = Some(ch);
                    normalized.push(ch);
                }
                c if c.is_whitespace() => {}
                _ => normalized.push(ch),
            }
        }

        normalized
    }

    fn bracket_depth_delta(source: &str) -> i32 {
        let mut depth = 0;
        let mut quote: Option<char> = None;
        let mut escaped = false;

        for ch in source.chars() {
            if escaped {
                escaped = false;
                continue;
            }

            if ch == '\\' {
                escaped = true;
                continue;
            }

            if let Some(active_quote) = quote {
                if ch == active_quote {
                    quote = None;
                }
                continue;
            }

            match ch {
                '"' | '\'' => quote = Some(ch),
                '(' | '[' | '{' => depth += 1,
                ')' | ']' | '}' => depth -= 1,
                _ => {}
            }
        }

        depth
    }

    fn source_line_key(source: &str) -> String {
        match crate::parser::tokenize(source) {
            Ok(tokens) => tokens
                .into_iter()
                .filter(|token| {
                    !matches!(
                        token,
                        crate::parser::Token::Newline
                            | crate::parser::Token::Indent
                            | crate::parser::Token::Dedent
                            | crate::parser::Token::Eof
                    )
                })
                .map(|token| token.to_string())
                .collect::<Vec<_>>()
                .join(""),
            Err(_) => Self::normalize_source_key(source),
        }
    }

    fn statement_source_key(statement: &Statement) -> Option<String> {
        let source = match statement {
            Statement::Assignment(assign) => {
                format!(
                    "{}={}",
                    assign.target,
                    Self::expression_source_key(&assign.value)
                )
            }
            Statement::ConstAssignment(assign) => {
                format!(
                    "const{}={}",
                    assign.target,
                    Self::expression_source_key(&assign.value)
                )
            }
            Statement::Expression(expr) => Self::expression_source_key(expr),
            Statement::If(if_stmt) => {
                format!("if{}:", Self::expression_source_key(&if_stmt.condition))
            }
            Statement::For(for_loop) => {
                let mut source = format!(
                    "for{}in{}",
                    for_loop.target,
                    Self::expression_source_key(&for_loop.iter)
                );
                if let Some(step) = &for_loop.step {
                    source.push_str("by");
                    source.push_str(&Self::expression_source_key(step));
                }
                source.push(':');
                source
            }
            Statement::While(while_loop) => {
                format!(
                    "while{}:",
                    Self::expression_source_key(&while_loop.condition)
                )
            }
            Statement::Match(match_stmt) => {
                format!("match{}:", Self::expression_source_key(&match_stmt.value))
            }
            Statement::Return(Some(expr)) => format!("return{}", Self::expression_source_key(expr)),
            Statement::Return(None) => "return".to_string(),
            Statement::Global(vars) => format!("global{}", vars.join(",")),
            Statement::MinecraftCommand(_) => return None,
            Statement::Execute(exec_block) => {
                let modifiers = exec_block
                    .modifiers
                    .iter()
                    .map(Self::execute_modifier_source_key)
                    .collect::<Vec<_>>()
                    .join("");
                format!("{}:", modifiers)
            }
            Statement::SelectorDef(selector_def) => {
                format!("{}={}", selector_def.name, selector_def.selector)
            }
            Statement::EntityDef(entity_def) => {
                format!(
                    "define{}={}create{}",
                    entity_def.name,
                    entity_def.selector,
                    Self::expression_source_key(&entity_def.nbt)
                )
            }
            Statement::CreateEntity(name) => format!("create{}", name),
            Statement::Import(_) | Statement::FunctionDef(_) | Statement::Pass => return None,
        };

        Some(Self::normalize_source_key(&source))
    }

    fn execute_modifier_source_key(modifier: &ExecuteModifier) -> String {
        match modifier {
            ExecuteModifier::As(selector) => format!("as{}", selector),
            ExecuteModifier::At(selector) => format!("at{}", selector),
            ExecuteModifier::If(expr) => format!("if{}", Self::expression_source_key(expr)),
            ExecuteModifier::IfRaw(raw) => format!("if{}", raw),
            ExecuteModifier::Unless(expr) => {
                format!("unless{}", Self::expression_source_key(expr))
            }
            ExecuteModifier::UnlessRaw(raw) => format!("unless{}", raw),
            ExecuteModifier::Positioned(pos) => format!("positioned{}", pos),
            ExecuteModifier::Rotated(rot) => format!("rotated{}", rot),
            ExecuteModifier::In(dimension) => format!("in{}", dimension),
            ExecuteModifier::Anchored(anchor) => format!("anchored{}", anchor),
            ExecuteModifier::Align(axes) => format!("align{}", axes),
            ExecuteModifier::Store(store) => format!("store{}", store),
        }
    }

    fn expression_source_key(expr: &Expression) -> String {
        match expr {
            Expression::Number(n) if n.fract() == 0.0 => (*n as i32).to_string(),
            Expression::Number(n) => n.to_string(),
            Expression::String(value) => serde_json::to_string(value)
                .unwrap_or_else(|_| format!("\"{}\"", value.replace('"', "\\\""))),
            Expression::Boolean(true) => "True".to_string(),
            Expression::Boolean(false) => "False".to_string(),
            Expression::None => "None".to_string(),
            Expression::Array(items) => {
                let items = items
                    .iter()
                    .map(Self::expression_source_key)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("[{}]", items)
            }
            Expression::Map(entries) => {
                let entries = entries
                    .iter()
                    .map(|(key, value)| {
                        let key = serde_json::to_string(key)
                            .unwrap_or_else(|_| format!("\"{}\"", key.replace('"', "\\\"")));
                        format!("{}:{}", key, Self::expression_source_key(value))
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{{{}}}", entries)
            }
            Expression::Identifier(name) => name.clone(),
            Expression::Attribute(obj, attr) => {
                format!("{}.{}", Self::expression_source_key(obj), attr)
            }
            Expression::Binary(left, op, right) => format!(
                "{}{}{}",
                Self::expression_source_key(left),
                Self::binary_op_source_key(op),
                Self::expression_source_key(right)
            ),
            Expression::Unary(op, expr) => {
                format!(
                    "{}{}",
                    Self::unary_op_source_key(op),
                    Self::expression_source_key(expr)
                )
            }
            Expression::Call(func, args) => {
                let args = args
                    .iter()
                    .map(Self::expression_source_key)
                    .collect::<Vec<_>>()
                    .join(",");
                format!("{}({})", Self::expression_source_key(func), args)
            }
            Expression::Subscript(base, index) => {
                format!(
                    "{}[{}]",
                    Self::expression_source_key(base),
                    Self::expression_source_key(index)
                )
            }
        }
    }

    fn binary_op_source_key(op: &BinaryOp) -> &'static str {
        match op {
            BinaryOp::Add => "+",
            BinaryOp::Sub => "-",
            BinaryOp::Mul => "*",
            BinaryOp::Div => "/",
            BinaryOp::Mod => "%",
            BinaryOp::Pow => "^",
            BinaryOp::Eq => "==",
            BinaryOp::NotEq => "!=",
            BinaryOp::Lt => "<",
            BinaryOp::LtEq => "<=",
            BinaryOp::Gt => ">",
            BinaryOp::GtEq => ">=",
            BinaryOp::And => "and",
            BinaryOp::Or => "or",
        }
    }

    fn unary_op_source_key(op: &UnaryOp) -> &'static str {
        match op {
            UnaryOp::Not => "not",
            UnaryOp::Neg => "-",
            UnaryOp::Pos => "+",
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
                    BinaryOp::Div => {
                        if right_val == 0.0 {
                            None
                        } else {
                            Some(left_val / right_val)
                        }
                    }
                    BinaryOp::Mod => {
                        if right_val == 0.0 {
                            None
                        } else {
                            Some((left_val as i32 % right_val as i32) as f64)
                        }
                    }
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
            Expression::Call(_, _) => CobbleType::Unknown, // Function return types not tracked yet
            _ => CobbleType::Unknown,
        }
    }

    /// Check if a type assignment is valid
    fn check_type_assignment(
        &self,
        var_name: &str,
        new_type: &crate::ast::CobbleType,
    ) -> Result<(), String> {
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
                ));
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

    pub fn transpile(&mut self, program: &Program) -> Result<(), String> {
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
        for statement in &program.statements {
            if let Statement::FunctionDef(func) = statement {
                let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();
                self.function_params.insert(func.name.clone(), param_names);
            }
        }

        // PASS 2: Process all statements (function bodies, etc.)
        for statement in &program.statements {
            self.process_statement(statement)?;
        }

        // Track temp objective for module-level variables BEFORE ensuring init function
        if !self.module_level_vars.is_empty() {
            self.data_pack.track_objective("temp");
        }

        // Ensure objectives are initialized FIRST
        self.data_pack.ensure_init_function();

        // Then initialize module-level variables in the init function
        if !self.module_level_vars.is_empty() {
            let mut init_commands = Vec::new();
            let mut init_metadata = HashMap::new();

            for (var_name, value, source) in self.module_level_vars.clone() {
                let command_start = init_commands.len();
                if let Some(const_value) = self.try_eval_const(&value) {
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
                            if const_value > i32::MAX as f64 { i32::MAX } else { i32::MIN }
                        );
                    }

                    if const_value.fract() != 0.0 {
                        eprintln!(
                            "⚠️  Warning: Module-level value {} for variable '{}' will lose precision.\n\
                            Scoreboard only supports integers. Fractional part will be truncated to: {}",
                            const_value,
                            var_name,
                            truncated
                        );
                    }

                    init_commands.push(format!(
                        "scoreboard players set {} temp {}",
                        var_name, truncated
                    ));
                    init_metadata.insert(
                        command_start,
                        GeneratedCommand::new(
                            init_commands[command_start].clone(),
                            source,
                            GeneratedCommandKind::ControlFlow,
                        ),
                    );
                    continue;
                }

                match &value {
                    Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                        // Storage variable initialization
                        let storage_path = format!("vars.{}", var_name);
                        self.variable_storage_paths
                            .insert(var_name.clone(), storage_path.clone());

                        // We need to serialize the value.
                        // self.serialize_to_snbt is defined in assignment.rs but available on Transpiler.
                        // However, Rust might complain if it's not visible?
                        // It is `pub(in crate::transpiler)`. `mod.rs` IS `crate::transpiler`.
                        // So it should be visible.
                        match self.serialize_to_snbt(&value) {
                            Ok(snbt) => {
                                init_commands.push(format!(
                                    "data modify storage {}:global {} set value {}",
                                    self.data_pack.namespace, storage_path, snbt
                                ));
                                init_metadata.insert(
                                    command_start,
                                    GeneratedCommand::new(
                                        init_commands[command_start].clone(),
                                        source,
                                        GeneratedCommandKind::ControlFlow,
                                    ),
                                );
                            }
                            Err(e) => {
                                return Err(format!(
                                    "Failed to serialize module variable '{}': {}",
                                    var_name, e
                                ));
                            }
                        }
                    }
                    _ => {
                        // Other complex expressions at module level
                        eprintln!("Note: Complex expression for '{}' cannot be initialized at module level", var_name);
                    }
                }
            }

            // Add these commands after gamerule and objective creation
            if !init_commands.is_empty() {
                if let Some(existing_init) = self.data_pack.functions.get("_cobble_init") {
                    // Find the position after gamerule and all objectives
                    let setup_end = existing_init
                        .iter()
                        .position(|cmd| {
                            !cmd.starts_with("gamerule")
                                && !cmd.starts_with("scoreboard objectives add")
                        })
                        .unwrap_or(existing_init.len());

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
                            let setup_end = handler_func
                                .iter()
                                .position(|cmd| {
                                    !cmd.starts_with("gamerule")
                                        && !cmd.starts_with("scoreboard objectives add")
                                })
                                .unwrap_or(handler_func.len());

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

        Ok(())
    }

    fn process_import(&mut self, import: &Import) -> Result<(), String> {
        // Handle stdlib imports
        if import.module == "stdlib" {
            // stdlib is automatically available, no action needed
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
            ));
        }

        // Security: Limit import depth to prevent stack overflow
        const MAX_IMPORT_DEPTH: usize = 50;
        if self.import_stack.len() >= MAX_IMPORT_DEPTH {
            return Err(format!(
                "Maximum import depth exceeded ({}).\n\
                This usually indicates a circular or excessively deep import chain.",
                MAX_IMPORT_DEPTH
            ));
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
            ));
        }

        // Check if already imported
        if self.imported_files.contains(&canonical_path) {
            return Ok(()); // Already imported, skip to avoid infinite loop
        }

        // Check if file exists
        if !canonical_path.exists() && !import_path.exists() {
            return Err(format!(
                "Cannot import '{}' from '{}': file '{}' not found",
                import.module,
                self.current_importer_display(),
                import_path.display()
            ));
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
        use crate::parser::{token_parser, tokenize};
        use chumsky::Parser;

        let tokens = tokenize(&source)
            .map_err(|e| format!("Tokenization failed for '{}': {}", import.module, e))?;

        let program = token_parser()
            .parse(&tokens)
            .into_result()
            .map_err(|errors| {
                format!(
                    "Parse failed for '{}': {}",
                    import.module,
                    errors
                        .iter()
                        .map(|e| format!("{:?}", e))
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;

        // Save current file dir
        let saved_dir = self.current_file_dir.clone();
        let saved_file = self.current_file_path.clone();

        // Set directory for nested imports
        if let Some(parent) = import_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }
        self.current_file_path = Some(canonical_path.clone());
        self.register_source_locations(&import_path, &source);

        // Process nested imports first
        for import in &program.imports {
            self.process_import(import)?;
        }

        // Then process imported program statements
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

    fn strip_command_prefix(cmd: &str) -> String {
        if let Some(stripped) = cmd.strip_prefix('/') {
            stripped.to_string()
        } else {
            cmd.to_string()
        }
    }

    fn process_statement(&mut self, statement: &Statement) -> Result<(), String> {
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

    fn process_statement_inner(&mut self, statement: &Statement) -> Result<(), String> {
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
                // Return statements are not supported in Minecraft functions
                // Minecraft functions execute all commands sequentially and cannot return early
                let expr_info = if let Some(e) = expr {
                    format!("\nAttempted to return: {:?}", e)
                } else {
                    String::new()
                };

                return Err(format!(
                    "Return statements are not supported.{}\n\n\
                    Minecraft functions cannot return early or return values.\n\
                    All commands in a function execute sequentially until the end.\n\n\
                    Solutions:\n\
                    1. Remove the return statement and restructure your code\n\
                    2. Use if/else blocks to conditionally execute code:\n\
                       if condition:\n\
                           # code to run\n\
                       # rest of function only runs if condition is false\n\
                    3. Use separate functions to organize logic\n\n\
                    Note: This is a limitation of Minecraft's function system,\n\
                    not a Cobble compiler issue.",
                    expr_info
                ));
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
                    return Err("Minecraft command outside of function context".to_string());
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
                matches!(
                    &**obj,
                    Expression::Identifier(module)
                        if matches!(
                            module.as_str(),
                            "math" | "text" | "score" | "random" | "timer" | "storage" | "datapack"
                        )
                )
            }
            Expression::Identifier(name) => {
                name == "addEventListener" || name == "stdlib.addEventListener"
            }
            _ => false,
        }
    }

    fn annotate_recent_commands(
        &mut self,
        start_index: usize,
        source: Option<SourceLocation>,
        default_kind: GeneratedCommandKind,
    ) {
        let Some(ref commands) = self.current_function else {
            return;
        };
        let Some(ref mut metadata) = self.current_function_metadata else {
            return;
        };
        if start_index > commands.len() {
            return;
        }

        for (index, command) in commands.iter().enumerate().skip(start_index) {
            let command = command.clone();
            metadata
                .entry(index)
                .and_modify(|generated| {
                    generated.text = command.clone();
                    if generated.source.is_none() {
                        generated.source = source.clone();
                    }
                })
                .or_insert_with(|| {
                    GeneratedCommand::new(command, source.clone(), default_kind.clone())
                });
        }
    }

    pub(in crate::transpiler) fn capture_statement(
        &mut self,
        statement: &Statement,
    ) -> Result<FunctionCapture, String> {
        self.capture_statements(std::slice::from_ref(statement))
    }

    pub(in crate::transpiler) fn capture_statements(
        &mut self,
        statements: &[Statement],
    ) -> Result<FunctionCapture, String> {
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
    ) -> Result<FunctionCapture, String> {
        let saved_state = self.save_compiler_state();
        let result = self.capture_statements(statements);
        self.restore_compiler_state(saved_state);
        result
    }

    pub(in crate::transpiler) fn capture_commands<F>(
        &mut self,
        body: F,
    ) -> Result<FunctionCapture, String>
    where
        F: FnOnce(&mut Self) -> Result<(), String>,
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
    ) -> Result<(T, FunctionCapture), String>
    where
        F: FnOnce(&mut Self) -> Result<T, String>,
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
    ) -> Result<(), String>
    where
        F: FnMut(&str) -> String,
    {
        let Some(ref mut commands) = self.current_function else {
            return Err("Cannot append generated commands outside of function context".to_string());
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

    fn process_create_entity(&mut self, name: &str) -> Result<(), String> {
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
            ));
        }
        Ok(())
    }

    fn process_entity_def(&mut self, entity_def: &EntityDef) -> Result<(), String> {
        self.entity_templates
            .insert(entity_def.name.clone(), entity_def.clone());
        Ok(())
    }

    fn process_function_def(&mut self, func: &FunctionDef) -> Result<(), String> {
        // Save previous function context
        let previous_function = self.current_function.take();
        let previous_metadata = self.current_function_metadata.take();
        let previous_context = self.current_context.clone();
        let previous_globals = self.global_variables.clone();
        let previous_variables = self.variables.clone();
        let previous_variable_types = self.variable_types.clone();

        // Extract parameter names from Parameter structs
        let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();

        // Set up new function context
        self.current_context = FunctionContext::with_params(param_names.clone());
        self.current_function = Some(Vec::new());
        self.current_function_metadata = Some(HashMap::new());
        self.global_variables.clear();

        // Store function parameters
        self.function_params.insert(func.name.clone(), param_names);

        // Process function body
        for statement in &func.body {
            self.process_statement(statement)?;
        }

        // Get the generated commands
        if let Some(commands) = self.current_function.take() {
            let metadata = self.current_function_metadata.take().unwrap_or_default();
            self.data_pack
                .add_function_with_metadata(func.name.clone(), commands, metadata);
        }

        // Restore previous context
        self.current_function = previous_function;
        self.current_function_metadata = previous_metadata;
        self.current_context = previous_context;
        self.global_variables = previous_globals;
        self.variables = previous_variables;
        self.variable_types = previous_variable_types;

        Ok(())
    }

    /// Helper method to evaluate a complex expression into a target variable
    /// Returns the commands needed to compute the expression
    fn evaluate_expression_to_target(
        &mut self,
        expr: &Expression,
        target: &str,
    ) -> Result<Vec<String>, String> {
        let mut evaluator = ExpressionEvaluator::new(
            &mut self.data_pack,
            &self.variable_objectives,
            &self.variable_storage_paths,
            &self.compile_time_constants,
        );
        evaluator.evaluate_expression_to_target(expr, target)
    }

    fn process_expression(&mut self, expr: &Expression) -> Result<(), String> {
        match expr {
            Expression::Call(func, args) => {
                // Handle both Identifier and Attribute (for stdlib.addEventListener)
                match &**func {
                    Expression::Identifier(func_name) => {
                        // Check for standalone addEventListener
                        if func_name == "addEventListener" {
                            self.process_add_event_listener(args)?;
                        } else if func_name.contains('.') {
                            // Method call on module (e.g., stdlib.addEventListener)
                            let parts: Vec<&str> = func_name.split('.').collect();
                            if parts.len() == 2
                                && parts[0] == "stdlib"
                                && parts[1] == "addEventListener"
                            {
                                self.process_add_event_listener(args)?;
                            } else {
                                return Err(format!("Unknown module method: {}", func_name));
                            }
                        } else {
                            // Regular function call - generate Minecraft function call
                            self.generate_function_call(func_name, args)?;
                        }
                    }
                    Expression::Attribute(obj, method) => {
                        // Handle attribute access like stdlib.addEventListener
                        if let Expression::Identifier(module_name) = &**obj {
                            if module_name == "stdlib" && method == "addEventListener" {
                                self.process_add_event_listener(args)?;
                            } else if module_name == "math" {
                                self.process_math_intrinsic(method, args)?;
                            } else if module_name == "text" {
                                self.process_text_intrinsic(method, args)?;
                            } else if module_name == "score" {
                                self.process_score_intrinsic(method, args)?;
                            } else if module_name == "random" {
                                self.process_random_intrinsic(method, args)?;
                            } else if module_name == "timer" {
                                self.process_timer_intrinsic(method, args)?;
                            } else if module_name == "storage" {
                                self.process_storage_intrinsic(method, args)?;
                            } else if module_name == "datapack" {
                                self.process_datapack_intrinsic(method, args)?;
                            } else {
                                return Err(format!(
                                    "Unknown module method: {}.{}",
                                    module_name, method
                                ));
                            }
                        } else {
                            return Err("Complex attribute access not supported".to_string());
                        }
                    }
                    _ => {
                        return Err("Unsupported function call expression".to_string());
                    }
                }
            }
            _ => {
                // Other expressions (like standalone identifiers for docstrings) don't generate commands
            }
        }
        Ok(())
    }

    fn push_current_command(&mut self, command: String) -> Result<(), String> {
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
            Err("Stdlib helper called outside of function context".to_string())
        }
    }

    fn expr_to_plain_arg(&self, expr: &Expression, label: &str) -> Result<String, String> {
        match expr {
            Expression::String(s) | Expression::Identifier(s) => Ok(s.clone()),
            Expression::Number(n) if n.fract() == 0.0 => Ok((*n as i32).to_string()),
            Expression::Number(n) => Ok(n.to_string()),
            _ => Err(format!("{} must be a string, identifier, or number", label)),
        }
    }

    fn expr_to_i32(&self, expr: &Expression, label: &str) -> Result<i32, String> {
        match expr {
            Expression::Number(n) => Ok(*n as i32),
            Expression::Identifier(name) => self
                .compile_time_constants
                .get(name)
                .map(|value| *value as i32)
                .ok_or_else(|| format!("{} must be a literal integer or const", label)),
            _ => Err(format!("{} must be a literal integer or const", label)),
        }
    }

    fn expr_to_json_value(&self, expr: &Expression) -> Result<serde_json::Value, String> {
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
                    ))
                }
            }
            _ => Err(format!("Unsupported expression in JSON value: {:?}", expr)),
        }
    }

    fn expr_to_text_component(&self, expr: &Expression) -> Result<String, String> {
        let value = match expr {
            Expression::String(s) => serde_json::json!({ "text": s }),
            _ => self.expr_to_json_value(expr)?,
        };
        serde_json::to_string(&value).map_err(|e| format!("Failed to encode text component: {}", e))
    }

    fn process_text_intrinsic(&mut self, method: &str, args: &[Expression]) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!("text.{}() takes 2 arguments", method));
        }

        let target = self.expr_to_plain_arg(&args[0], "text target")?;
        let component = self.expr_to_text_component(&args[1])?;
        let command = match method {
            "tellraw" => format!("tellraw {} {}", target, component),
            "title" | "subtitle" | "actionbar" => {
                format!("title {} {} {}", target, method, component)
            }
            _ => return Err(format!("Unknown text function: text.{}", method)),
        };
        self.push_current_command(command)
    }

    fn process_score_intrinsic(&mut self, method: &str, args: &[Expression]) -> Result<(), String> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("score.set() takes 2 arguments".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_i32(&args[1], "score value")?;
                format!("scoreboard players set {} temp {}", name, value)
            }
            "add" | "remove" => {
                if args.len() != 2 {
                    return Err(format!("score.{}() takes 2 arguments", method));
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                let value = self.expr_to_i32(&args[1], "score value")?;
                format!("scoreboard players {} {} temp {}", method, name, value)
            }
            "reset" => {
                if args.len() != 1 {
                    return Err("score.reset() takes 1 argument".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "score name")?;
                format!("scoreboard players reset {} temp", name)
            }
            "copy" => {
                if args.len() != 2 {
                    return Err("score.copy() takes 2 arguments".to_string());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let src = self.expr_to_plain_arg(&args[1], "source score")?;
                format!("scoreboard players operation {} temp = {} temp", dst, src)
            }
            "operation" => {
                if args.len() != 3 {
                    return Err("score.operation() takes 3 arguments".to_string());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination score")?;
                let op = self.expr_to_plain_arg(&args[1], "score operation")?;
                let src = self.expr_to_plain_arg(&args[2], "source score")?;
                const ALLOWED_OPS: &[&str] = &["=", "+=", "-=", "*=", "/=", "%=", "<", ">", "><"];
                if !ALLOWED_OPS.contains(&op.as_str()) {
                    return Err(format!("Unsupported scoreboard operation '{}'", op));
                }
                format!(
                    "scoreboard players operation {} temp {} {} temp",
                    dst, op, src
                )
            }
            _ => return Err(format!("Unknown score function: score.{}", method)),
        };
        self.push_current_command(command)
    }

    fn process_random_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "int" => {
                if args.len() != 3 {
                    return Err("random.int() takes 3 arguments".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                let min = self.expr_to_i32(&args[1], "random min")?;
                let max = self.expr_to_i32(&args[2], "random max")?;
                if min > max {
                    return Err("random.int() min cannot be greater than max".to_string());
                }
                format!(
                    "execute store result score {} temp run random value {}..{}",
                    name, min, max
                )
            }
            "bool" => {
                if args.len() != 1 {
                    return Err("random.bool() takes 1 argument".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "random target score")?;
                format!(
                    "execute store result score {} temp run random value 0..1",
                    name
                )
            }
            _ => return Err(format!("Unknown random function: random.{}", method)),
        };
        self.push_current_command(command)
    }

    fn process_timer_intrinsic(&mut self, method: &str, args: &[Expression]) -> Result<(), String> {
        self.data_pack.track_objective("temp");
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("timer.set() takes 2 arguments".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                let ticks = self.expr_to_i32(&args[1], "timer ticks")?;
                format!("scoreboard players set {} temp {}", name, ticks)
            }
            "tick" => {
                if args.len() != 1 {
                    return Err("timer.tick() takes 1 argument".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!(
                    "execute if score {} temp matches 1.. run scoreboard players remove {} temp 1",
                    name, name
                )
            }
            "done" => {
                if args.len() != 1 {
                    return Err("timer.done() takes 1 argument".to_string());
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
                    return Err("timer.reset() takes 1 argument".to_string());
                }
                let name = self.expr_to_plain_arg(&args[0], "timer name")?;
                format!("scoreboard players reset {} temp", name)
            }
            _ => return Err(format!("Unknown timer function: timer.{}", method)),
        };
        self.push_current_command(command)
    }

    fn process_storage_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        let command = match method {
            "set" => {
                if args.len() != 2 {
                    return Err("storage.set() takes 2 arguments".to_string());
                }
                let path = self.expr_to_plain_arg(&args[0], "storage path")?;
                let value = self.serialize_to_snbt(&args[1])?;
                format!(
                    "data modify storage {}:global {} set value {}",
                    self.data_pack.namespace, path, value
                )
            }
            "merge" => {
                if args.len() != 2 {
                    return Err("storage.merge() takes 2 arguments".to_string());
                }
                let path = self.expr_to_plain_arg(&args[0], "storage path")?;
                let value = self.serialize_to_snbt(&args[1])?;
                format!(
                    "data modify storage {}:global {} merge value {}",
                    self.data_pack.namespace, path, value
                )
            }
            "remove" => {
                if args.len() != 1 {
                    return Err("storage.remove() takes 1 argument".to_string());
                }
                let path = self.expr_to_plain_arg(&args[0], "storage path")?;
                format!(
                    "data remove storage {}:global {}",
                    self.data_pack.namespace, path
                )
            }
            "copy" => {
                if args.len() != 2 {
                    return Err("storage.copy() takes 2 arguments".to_string());
                }
                let dst = self.expr_to_plain_arg(&args[0], "destination storage path")?;
                let src = self.expr_to_plain_arg(&args[1], "source storage path")?;
                format!(
                    "data modify storage {}:global {} set from storage {}:global {}",
                    self.data_pack.namespace, dst, self.data_pack.namespace, src
                )
            }
            _ => return Err(format!("Unknown storage function: storage.{}", method)),
        };
        self.push_current_command(command)
    }

    fn process_datapack_intrinsic(
        &mut self,
        method: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        match method {
            "function_tag" => self.add_datapack_tag_resource("function", args),
            "block_tag" => self.add_datapack_tag_resource("block", args),
            "item_tag" => self.add_datapack_tag_resource("item", args),
            "entity_type_tag" => self.add_datapack_tag_resource("entity_type", args),
            "predicate" => self.add_datapack_json_resource("predicate", args),
            "advancement" => self.add_datapack_json_resource("advancement", args),
            "loot_table" => self.add_datapack_json_resource("loot_table", args),
            "recipe" => self.add_datapack_json_resource("recipe", args),
            "item_modifier" => self.add_datapack_json_resource("item_modifier", args),
            "dialog" => self.add_datapack_json_resource("dialog", args),
            _ => Err(format!("Unknown datapack function: datapack.{}", method)),
        }
    }

    fn add_datapack_tag_resource(
        &mut self,
        tag_type: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!("datapack.{}_tag() takes 2 arguments", tag_type));
        }

        let name = self.expr_to_resource_name(&args[0], "tag name")?;
        let values = self.expr_to_json_value(&args[1])?;
        if !values.is_array() {
            return Err("Tag values must be an array".to_string());
        }

        let json = serde_json::json!({ "values": values });
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode tag JSON: {}", e))?;
        self.data_pack
            .add_json_resource(format!("tags/{}/{}", tag_type, name), content)
    }

    fn add_datapack_json_resource(
        &mut self,
        resource_type: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        if args.len() != 2 {
            return Err(format!("datapack.{}() takes 2 arguments", resource_type));
        }

        let name = self.expr_to_resource_name(&args[0], "resource name")?;
        let json = self.expr_to_json_value(&args[1])?;
        let content = serde_json::to_string_pretty(&json)
            .map_err(|e| format!("Failed to encode resource JSON: {}", e))?;
        self.data_pack
            .add_json_resource(format!("{}/{}", resource_type, name), content)
    }

    fn expr_to_resource_name(&self, expr: &Expression, label: &str) -> Result<String, String> {
        let name = self.expr_to_plain_arg(expr, label)?;
        let has_invalid_segment = name
            .split('/')
            .any(|segment| segment.is_empty() || segment == "." || segment == "..");
        let has_invalid_char = name.chars().any(|c| {
            !(c.is_ascii_lowercase()
                || c.is_ascii_digit()
                || c == '_'
                || c == '-'
                || c == '.'
                || c == '/')
        });
        if name.is_empty() || name.contains('\\') || has_invalid_segment || has_invalid_char {
            return Err(format!(
                "Invalid {} '{}': use lowercase resource paths with letters, digits, '/', '_', '-', or '.', and no empty, '.', or '..' segments",
                label, name
            ));
        }
        Ok(name)
    }

    fn process_math_intrinsic(&mut self, method: &str, args: &[Expression]) -> Result<(), String> {
        match method {
            "sqrt" => {
                if args.len() != 1 {
                    return Err("math.sqrt() takes 1 argument".to_string());
                }
                self.process_math_unary_op(args, "sqrt")
            }
            "abs" => {
                if args.len() != 1 {
                    return Err("math.abs() takes 1 argument".to_string());
                }
                self.process_math_unary_op(args, "abs")
            }
            "min" => {
                if args.len() != 2 {
                    return Err("math.min() takes 2 arguments".to_string());
                }
                self.process_math_binary_op(args, "min")
            }
            "max" => {
                if args.len() != 2 {
                    return Err("math.max() takes 2 arguments".to_string());
                }
                self.process_math_binary_op(args, "max")
            }
            _ => Err(format!("Unknown math function: math.{}", method)),
        }
    }

    fn process_math_unary_op(&mut self, args: &[Expression], op: &str) -> Result<(), String> {
        let input_var = "#math_input";
        // output_var is implied to be "#math_result" by convention in helpers

        self.data_pack.track_objective("math");
        self.variable_objectives
            .insert(input_var.to_string(), "math".to_string());
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

    fn process_math_binary_op(&mut self, args: &[Expression], op: &str) -> Result<(), String> {
        let input1 = "#math_input";
        let input2 = "#math_input2";

        self.data_pack.track_objective("math");
        self.variable_objectives
            .insert(input1.to_string(), "math".to_string());
        self.variable_objectives
            .insert(input2.to_string(), "math".to_string());
        self.scoreboard_variables.insert(input1.to_string());
        self.scoreboard_variables.insert(input2.to_string());

        // Evaluate arguments
        let mut cmds = Vec::new();
        cmds.extend(self.evaluate_expression_to_target(&args[0], input1)?);
        cmds.extend(self.evaluate_expression_to_target(&args[1], input2)?);

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
    ) -> Result<(), String> {
        let mut commands = Vec::new();

        if let Some(param_names) = self.function_params.get(func_name).cloned() {
            // Validate argument count matches parameter count
            if !param_names.is_empty() && args.len() != param_names.len() {
                return Err(format!(
                    "Function '{}' expects {} argument(s), but {} provided.\n\
                    Expected parameters: ({})",
                    func_name,
                    param_names.len(),
                    args.len(),
                    param_names.join(", ")
                ));
            }

            // If function has parameters, use macro system
            if !param_names.is_empty() && !args.is_empty() {
                // Store arguments in storage for macro substitution
                for (i, arg) in args.iter().enumerate() {
                    if i < param_names.len() {
                        let param_name = &param_names[i];
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
                                    "data modify storage {}:global args.{} set value \"{}\"",
                                    self.data_pack.namespace, param_name, escaped
                                ));
                            }
                            Expression::Number(n) => {
                                commands.push(format!(
                                    "data modify storage {}:global args.{} set value {}",
                                    self.data_pack.namespace, param_name, *n as i32
                                ));
                            }
                            Expression::Identifier(var) => {
                                // Check if this is a selector (@...) or literal string (like item names)
                                if var.starts_with('@') {
                                    // Selector - store as string
                                    commands.push(format!(
                                        "data modify storage {}:global args.{} set value \"{}\"",
                                        self.data_pack.namespace, param_name, var
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
                                        "execute store result storage {}:global args.{} int 1 run scoreboard players get {} {}",
                                        self.data_pack.namespace, param_name, var, obj
                                    ));
                                } else if let Some(_const_val) = self.variables.get(var) {
                                    // Constant variable - try to get compile-time constant value
                                    if let Some(&val) = self.compile_time_constants.get(var) {
                                        commands.push(format!(
                                            "data modify storage {}:global args.{} set value {}",
                                            self.data_pack.namespace, param_name, val
                                        ));
                                    } else {
                                        // Const variable but not a simple constant - treat as identifier string
                                        commands.push(format!(
                                            "data modify storage {}:global args.{} set value \"{}\"",
                                            self.data_pack.namespace, param_name, var
                                        ));
                                    }
                                } else {
                                    // Unknown identifier - treat as string literal (for items, etc.)
                                    commands.push(format!(
                                        "data modify storage {}:global args.{} set value \"{}\"",
                                        self.data_pack.namespace, param_name, var
                                    ));
                                }
                            }
                            Expression::Boolean(b) => {
                                // Boolean literal - store as 1 (true) or 0 (false)
                                let value = if *b { 1 } else { 0 };
                                commands.push(format!(
                                    "data modify storage {}:global args.{} set value {}",
                                    self.data_pack.namespace, param_name, value
                                ));
                            }
                            Expression::Binary(_, _, _) | Expression::Unary(_, _) => {
                                // Arithmetic expression - evaluate to a temporary variable and store
                                let temp_var = format!("_arg_temp_{}", i);

                                // Track the temp variable
                                self.data_pack.track_objective("temp");
                                self.variable_objectives
                                    .insert(temp_var.clone(), "temp".to_string());
                                self.scoreboard_variables.insert(temp_var.clone());

                                // Evaluate the expression into the temp variable
                                let assignment = Assignment {
                                    target: temp_var.clone(),
                                    value: arg.clone(),
                                };
                                self.process_assignment(&assignment)?;

                                // Now store the temp variable value into storage
                                commands.push(format!(
                                    "execute store result storage {}:global args.{} int 1 run scoreboard players get {} temp",
                                    self.data_pack.namespace, param_name, temp_var
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
                                     - Function calls as parameters: func(other_func())\n\
                                     - Complex expressions with function calls",
                                    param_name, arg
                                ));
                            }
                        }
                    }
                }

                // Generate macro call with storage (only when parameters are provided)
                commands.push(format!(
                    "function {}:{} with storage {}:global args",
                    self.data_pack.namespace, func_name, self.data_pack.namespace
                ));
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

    fn process_add_event_listener(&mut self, args: &[Expression]) -> Result<(), String> {
        if args.len() != 2 {
            return Err("addEventListener requires 2 arguments: (event, handler)".to_string());
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
                            _ => return Err(format!("Unknown event type: {}", attr)),
                        }
                    } else {
                        return Err(format!("Unknown module: {}", module));
                    }
                } else {
                    return Err("Event must be from 'event' module".to_string());
                }
            }
            _ => return Err("First argument must be an event (e.g., event.LOAD)".to_string()),
        };

        // Extract handler function name
        let handler_name = match &args[1] {
            Expression::Identifier(name) => name.clone(),
            _ => return Err("Second argument must be a function name".to_string()),
        };

        // Register the event listener
        self.data_pack
            .stdlib
            .add_event_listener(event_type, handler_name);

        Ok(())
    }

    fn preprocess_condition(&mut self, condition: &Expression) -> Result<Expression, String> {
        // Check if condition contains Boolean literals and track __internal__ objective
        if Self::contains_boolean_literal(condition) {
            self.data_pack.track_objective("__internal__");
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
                    match &**left {
                        Expression::Binary(_, _, _) => {
                            // Left side is a binary expression, need to evaluate it first
                            // Use unique temp variable name to avoid conflicts in AND/OR chains
                            let temp_var = format!("expr_cond_temp_{}", self.temp_counter);
                            self.temp_counter += 1;

                            self.data_pack.track_objective("temp");
                            self.variable_objectives
                                .insert(temp_var.clone(), "temp".to_string());
                            self.scoreboard_variables.insert(temp_var.clone());

                            // Evaluate the left expression into the unique temp variable
                            let eval_commands =
                                self.evaluate_expression_to_target(left, &temp_var)?;

                            if let Some(ref mut commands) = self.current_function {
                                commands.extend(eval_commands);
                            }

                            // Return a simplified condition with the temporary variable
                            Ok(Expression::Binary(
                                Box::new(Expression::Identifier(temp_var)),
                                op.clone(),
                                right.clone(),
                            ))
                        }
                        _ => {
                            // Left side is simple, check if right side needs preprocessing
                            match &**right {
                                Expression::Binary(_, _, _) => {
                                    // Right side is a binary expression
                                    let temp_var = format!("expr_cond_temp_{}", self.temp_counter);
                                    self.temp_counter += 1;

                                    self.data_pack.track_objective("temp");
                                    self.variable_objectives
                                        .insert(temp_var.clone(), "temp".to_string());
                                    self.scoreboard_variables.insert(temp_var.clone());

                                    let eval_commands =
                                        self.evaluate_expression_to_target(right, &temp_var)?;

                                    if let Some(ref mut commands) = self.current_function {
                                        commands.extend(eval_commands);
                                    }

                                    Ok(Expression::Binary(
                                        left.clone(),
                                        op.clone(),
                                        Box::new(Expression::Identifier(temp_var)),
                                    ))
                                }
                                _ => {
                                    // Both sides are simple, return as is
                                    Ok(condition.clone())
                                }
                            }
                        }
                    }
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
                            // Other binary operators (arithmetic) shouldn't be in conditions
                            Ok(condition.clone())
                        }
                    }
                }
            }
            Expression::Unary(op, expr) => {
                // Recursively preprocess the inner expression
                let new_expr = self.preprocess_condition(expr)?;
                Ok(Expression::Unary(op.clone(), Box::new(new_expr)))
            }
            _ => {
                // Simple expressions (Identifier, Number, etc.) don't need preprocessing
                Ok(condition.clone())
            }
        }
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

    fn translate_condition(&self, condition: &Expression) -> Result<String, String> {
        let translator =
            ConditionTranslator::new(&self.variable_objectives, &self.compile_time_constants);
        translator.translate(condition)
    }

    fn handle_or_condition(&mut self, or_expr: &str) -> Result<String, String> {
        // Flatten all OR conditions into a single list
        let conditions = Self::flatten_or_conditions(or_expr)?;

        // Use unique variable name to prevent race conditions with multiple entities
        let or_var = format!("or_temp_{}", self.get_unique_id());

        // Track the temp objective for or result
        self.data_pack.track_objective("temp");

        // Generate commands for OR logic
        if let Some(ref mut commands) = self.current_function {
            // Initialize or variable to 0
            commands.push(format!("scoreboard players set {} temp 0", or_var));

            // For each condition, if true, set or variable to 1
            for cond in &conditions {
                let cond_prefix = if cond.starts_with("if ") || cond.starts_with("unless ") {
                    cond.clone()
                } else {
                    format!("if {}", cond)
                };
                commands.push(format!(
                    "execute {} run scoreboard players set {} temp 1",
                    cond_prefix, or_var
                ));
            }
        }

        // Return the condition that checks if or variable is 1
        Ok(format!("score {} temp matches 1", or_var))
    }

    fn flatten_or_conditions(or_expr: &str) -> Result<Vec<String>, String> {
        let mut conditions = Vec::new();

        if !or_expr.starts_with("OR(") {
            // Not an OR expression, return as-is
            return Ok(vec![or_expr.to_string()]);
        }

        // Extract inner content - be careful with the closing parenthesis
        let end_pos = or_expr
            .rfind(')')
            .ok_or("Missing closing parenthesis in OR expression")?;
        let inner = &or_expr[3..end_pos];

        // Split by semicolon, but only at the top level (not inside nested parentheses)
        let mut parts = Vec::new();
        let mut current_part = String::new();
        let mut depth = 0;

        for ch in inner.chars() {
            match ch {
                '(' => {
                    depth += 1;
                    current_part.push(ch);
                }
                ')' => {
                    depth -= 1;
                    current_part.push(ch);
                }
                ';' if depth == 0 => {
                    // Top-level semicolon - this is a separator
                    if !current_part.is_empty() {
                        parts.push(current_part.trim().to_string());
                        current_part.clear();
                    }
                }
                _ => current_part.push(ch),
            }
        }
        // Don't forget the last part
        if !current_part.is_empty() {
            parts.push(current_part.trim().to_string());
        }

        for part in parts {
            if !part.is_empty() {
                // Check if this part is itself an OR expression
                if part.starts_with("OR(") {
                    conditions.extend(Self::flatten_or_conditions(&part)?);
                } else {
                    conditions.push(part);
                }
            }
        }

        Ok(conditions)
    }

    fn split_or_conditions<'a>(&self, inner: &'a str) -> Result<(&'a str, &'a str), String> {
        // Find the semicolon that separates cond1 and cond2
        // Need to handle nested parentheses
        let mut depth = 0;
        let mut semicolon_pos = None;
        for (i, ch) in inner.chars().enumerate() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ';' if depth == 0 => {
                    semicolon_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }

        if let Some(pos) = semicolon_pos {
            let cond1 = inner[..pos].trim();
            let cond2 = inner[pos + 1..].trim();
            Ok((cond1, cond2))
        } else {
            Err("OR expression must have two conditions".to_string())
        }
    }

    fn handle_or_and_condition(&mut self, expr: &str) -> Result<String, String> {
        // Handle both OR(...) and OR_AND(...) expressions
        if expr.starts_with("OR_AND(") {
            // Extract the two parts
            let inner = &expr[7..expr.len() - 1];
            let (left, right) = self.split_or_conditions(inner)?;

            // Process left side first (which may contain OR)
            let left_processed = if left.contains("OR(") {
                self.handle_or_condition(left)?
            } else {
                left.to_string()
            };

            // Process right side
            let right_processed = if right.contains("OR(") {
                self.handle_or_condition(right)?
            } else {
                right.to_string()
            };

            // Now combine with AND
            let left_final =
                if left_processed.starts_with("if ") || left_processed.starts_with("unless ") {
                    left_processed
                } else {
                    format!("if {}", left_processed)
                };

            let right_final =
                if right_processed.starts_with("if ") || right_processed.starts_with("unless ") {
                    right_processed
                } else {
                    format!("if {}", right_processed)
                };

            Ok(format!("{} {}", left_final, right_final))
        } else {
            // Regular OR(...)
            self.handle_or_condition(expr)
        }
    }

    pub fn write_data_pack(&self) -> std::io::Result<()> {
        self.data_pack.write()
    }

    /// Get a unique ID for temporary variables
    fn get_unique_id(&mut self) -> u32 {
        let id = self.temp_counter;
        self.temp_counter += 1;
        id
    }

    /// Check if a condition string looks like a Python expression
    fn looks_like_python_expression(&self, condition: &str) -> bool {
        // Check for Python expression indicators FIRST
        // This must come before the Minecraft keyword check because a user variable
        // might have the same name as a Minecraft keyword (e.g., `score >= 10`)
        let python_indicators = [
            " >= ", " <= ", " == ", " != ", " > ", " < ", " and ", " or ", " not ",
        ];
        for indicator in &python_indicators {
            if condition.contains(indicator) {
                return true;
            }
        }

        // Minecraft raw conditions usually start with these keywords followed by
        // specific syntax (e.g., "score x temp matches 5", "entity @a", "block ~ ~ ~ stone")
        // Only treat as raw Minecraft if it matches the full pattern, not just the keyword
        let minecraft_raw_patterns = [
            "score ",     // "score <target> <objective> matches/</>/>=/<=/= ..."
            "entity ",    // "entity <selector>"
            "block ",     // "block <pos> <block>"
            "blocks ",    // "blocks <start> <end> <dest> <mode>"
            "biome ",     // "biome <pos> <biome>"
            "dimension ", // "dimension <dimension>"
            "predicate ", // "predicate <predicate>"
            "data ",      // "data <source> <path>"
        ];

        for pattern in &minecraft_raw_patterns {
            if condition.starts_with(pattern) {
                // Extra check: if this looks like "score <target> <obj> matches/op",
                // it's raw Minecraft. If it looks like "score >= 10", it's Python.
                if pattern == &"score " {
                    // Raw Minecraft score condition has at least 3 space-separated parts
                    // e.g., "score x temp matches 5" or "score x temp = y temp"
                    let parts: Vec<&str> = condition.splitn(4, ' ').collect();
                    if parts.len() >= 4 {
                        return false; // Looks like raw Minecraft: "score <target> <obj> ..."
                    }
                    // Otherwise it might be a Python expression with variable named "score"
                    return true;
                }
                return false;
            }
        }

        // Check if it's a simple variable name (also a Python expression)
        if condition.chars().all(|c| c.is_alphanumeric() || c == '_') {
            // Check if it's a known variable
            return self.variables.contains_key(condition)
                || self.scoreboard_variables.contains(condition);
        }

        false
    }

    /// Try to translate a Python expression string to Minecraft condition
    fn try_translate_python_expression(
        &self,
        condition: &str,
        _is_unless: bool,
    ) -> Result<String, String> {
        // First check if this contains AND or OR operators
        if condition.contains(" and ") || condition.contains(" or ") {
            // Complex expression with logical operators
            // We need to parse this properly, but for now we can handle simple cases

            // Handle AND conditions specially
            if condition.contains(" and ") {
                let parts: Vec<&str> = condition.split(" and ").collect();
                let mut translated_parts = Vec::new();

                for part in parts {
                    // Trim the part but preserve the basic structure
                    let part = part.trim();

                    // Add proper spacing before range operators if missing
                    let fixed_part = if part.contains("matches..") {
                        part.replace("matches..", "matches ..")
                    } else {
                        part.to_string()
                    };

                    // Check if this part already has "score" keyword (raw Minecraft condition)
                    if fixed_part.starts_with("score ") {
                        // This is already a Minecraft condition, just add "if" prefix
                        translated_parts.push(format!("if {}", fixed_part));
                    } else {
                        // Try to translate as Python expression
                        if let Ok(translated) =
                            self.try_translate_python_expression(&fixed_part, false)
                        {
                            // The translated part should already be in the form "score x temp matches ..."
                            // We just need to add "if " prefix
                            if translated.starts_with("score ") {
                                translated_parts.push(format!("if {}", translated));
                            } else if translated.starts_with("if ")
                                || translated.starts_with("unless ")
                            {
                                translated_parts.push(translated);
                            } else {
                                translated_parts.push(format!("if {}", translated));
                            }
                        } else {
                            return Err(format!(
                                "Failed to translate condition part: {}",
                                fixed_part
                            ));
                        }
                    }
                }

                // Join with spaces to create chained conditions
                return Ok(translated_parts.join(" "));
            }

            // OR conditions - return a special marker
            // We'll expand this into multiple conditions later
            // For now, translate each part and return OR(part1, part2)
            if condition.contains(" or ") {
                let parts: Vec<&str> = condition.split(" or ").collect();
                let mut translated_parts = Vec::new();

                for part in parts {
                    let part = part.trim();
                    // Recursively translate each part
                    if let Ok(translated) = self.try_translate_python_expression(part, false) {
                        // Remove any "if" or "unless" prefix that might have been added
                        let clean_translated = translated
                            .strip_prefix("if ")
                            .or_else(|| translated.strip_prefix("unless "))
                            .unwrap_or(&translated);
                        translated_parts.push(clean_translated.to_string());
                    } else {
                        // Try simple translation
                        translated_parts.push(part.to_string());
                    }
                }

                // Return OR marker - use semicolon as separator to avoid confusion with commas in conditions
                return Ok(format!("OR({})", translated_parts.join(";")));
            }

            // AND conditions handling remains the same
            return Err("Complex logical expressions not supported".to_string());
        }

        // Simple variable check: "varname" -> "score varname objective matches 1.."
        if condition.chars().all(|c| c.is_alphanumeric() || c == '_')
            && (self.variables.contains_key(condition)
                || self.scoreboard_variables.contains(condition))
        {
            let objective = self
                .variable_objectives
                .get(condition)
                .map(|s| s.as_str())
                .unwrap_or("temp");
            return Ok(format!("score {} {} matches 1..", condition, objective));
        }

        // For more complex expressions, we need proper parsing
        // This is a simplified version - ideally we'd parse the expression properly

        // Handle simple comparisons: "x > 5", "y == 10", etc.
        if let Some(op_pos) = condition.find(" > ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[3..]; // Skip " > "
            let left = left.trim();
            let right = right.trim();

            // Check if right side is a macro parameter {param}
            let right_value = if right.starts_with('{') && right.ends_with('}') {
                // This is a macro parameter, keep as-is with ..
                format!("{}..", right)
            } else if let Ok(value) = right.parse::<i32>() {
                format!("{}..", value + 1)
            } else {
                return Err("Cannot parse right side of comparison".to_string());
            };

            // Check if left side is a macro parameter {param}
            let left_var = left.to_string();
            let objective = if left.starts_with('{') && left.ends_with('}') {
                "temp".to_string() // Use temp for macro params
            } else {
                self.variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp")
                    .to_string()
            };

            return Ok(format!(
                "score {} {} matches {}",
                left_var, objective, right_value
            ));
        }

        if let Some(op_pos) = condition.find(" < ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[3..]; // Skip " < "
            let left = left.trim();
            let right = right.trim();

            if let Ok(value) = right.parse::<i32>() {
                let objective = self
                    .variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp");
                if value == i32::MIN {
                    return Ok("score 0 temp matches 1 unless score 0 temp matches 1".to_string());
                    // Always false
                }
                return Ok(format!(
                    "score {} {} matches ..{}",
                    left,
                    objective,
                    value - 1
                ));
            }
        }

        if let Some(op_pos) = condition.find(" == ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[4..]; // Skip " == "
            let left = left.trim();
            let right = right.trim();

            if let Ok(value) = right.parse::<i32>() {
                let objective = self
                    .variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp");
                return Ok(format!("score {} {} matches {}", left, objective, value));
            }
        }

        if let Some(op_pos) = condition.find(" >= ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[4..]; // Skip " >= "
            let left = left.trim();
            let right = right.trim();

            // Check if right side is a macro parameter {param}
            let right_value = if right.starts_with('{') && right.ends_with('}') {
                // This is a macro parameter, keep as-is with ..
                format!("{}..", right)
            } else if let Ok(value) = right.parse::<i32>() {
                format!("{}..", value)
            } else {
                return Err("Cannot parse right side of comparison".to_string());
            };

            let left_var = left.to_string();
            let objective = if left.starts_with('{') && left.ends_with('}') {
                "temp".to_string()
            } else {
                self.variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp")
                    .to_string()
            };

            return Ok(format!(
                "score {} {} matches {}",
                left_var, objective, right_value
            ));
        }

        if let Some(op_pos) = condition.find(" <= ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[4..]; // Skip " <= "
            let left = left.trim();
            let right = right.trim();

            if let Ok(value) = right.parse::<i32>() {
                let objective = self
                    .variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp");
                return Ok(format!("score {} {} matches ..{}", left, objective, value));
            }
        }

        if let Some(op_pos) = condition.find(" != ") {
            let (left, right) = condition.split_at(op_pos);
            let right = &right[4..]; // Skip " != "
            let left = left.trim();
            let right = right.trim();

            if let Ok(value) = right.parse::<i32>() {
                let objective = self
                    .variable_objectives
                    .get(left)
                    .map(|s| s.as_str())
                    .unwrap_or("temp");
                // != translates to "unless ... matches"
                return Ok(format!(
                    "unless score {} {} matches {}",
                    left, objective, value
                ));
            }
        }

        // If we can't parse it, return error
        Err("Could not parse Python expression".to_string())
    }
}
