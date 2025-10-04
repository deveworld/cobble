// Module declarations
mod command_processor;
mod condition_translator;
mod data_pack;
mod expression_evaluator;
mod statement_processors;

// Public exports
pub use data_pack::DataPack;

use crate::ast::*;
use crate::stdlib::EventType;
use command_processor::CommandProcessor;
use condition_translator::ConditionTranslator;
use expression_evaluator::ExpressionEvaluator;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

/// Context for tracking function parameters and scope
#[derive(Clone)]
struct FunctionContext {
    params: Vec<String>,
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

pub struct Transpiler {
    pub data_pack: DataPack,
    current_function: Option<Vec<String>>,
    current_context: FunctionContext,
    variables: HashMap<String, Expression>,
    temp_counter: u32,
    variable_objectives: HashMap<String, String>, // Track which objective each variable uses
    scoreboard_variables: HashSet<String>, // Track variables backed by scoreboard (not constants)
    function_params: HashMap<String, Vec<String>>, // Track function parameter names
    global_variables: HashSet<String>,     // Track global variables declared in current function
    module_level_vars: HashMap<String, Expression>, // Store module-level assignments
    compile_time_constants: HashMap<String, f64>, // Store compile-time constant values
    selector_aliases: HashMap<String, String>, // Store selector definitions (@Name -> @a[...])
    imported_files: HashSet<PathBuf>,      // Track imported files to prevent circular dependencies
    import_stack: Vec<PathBuf>,            // Track current import chain for circular detection
    current_file_dir: PathBuf, // Current file's directory for resolving relative imports
    variable_types: HashMap<String, crate::ast::CobbleType>, // Track type of each variable for type checking
}

impl Transpiler {
    pub fn new(namespace: String, output_dir: PathBuf) -> Self {
        Self {
            data_pack: DataPack::new(namespace, output_dir),
            current_function: None,
            current_context: FunctionContext::new(),
            variables: HashMap::new(),
            temp_counter: 0,
            variable_objectives: HashMap::new(),
            scoreboard_variables: HashSet::new(),
            function_params: HashMap::new(),
            global_variables: HashSet::new(),
            module_level_vars: HashMap::new(),
            compile_time_constants: HashMap::new(),
            selector_aliases: HashMap::new(),
            imported_files: HashSet::new(),
            import_stack: Vec::new(),
            current_file_dir: PathBuf::from("."),
            variable_types: HashMap::new(),
        }
    }

    pub fn set_current_file(&mut self, file_path: &PathBuf) {
        if let Some(parent) = file_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }

        // Add main file to import stack and imported_files for circular import detection
        // Canonicalize if possible, otherwise use as-is
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.clone());
        if !self.import_stack.contains(&canonical_path) {
            self.import_stack.push(canonical_path.clone());
            self.imported_files.insert(canonical_path);
        }
    }

    /// Infer the type of an expression
    /// Evaluate a constant expression to a number if possible
    fn try_eval_const(&self, expr: &Expression) -> Option<f64> {
        match expr {
            Expression::Number(n) => Some(*n),
            Expression::Identifier(name) => self.compile_time_constants.get(name).copied(),
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
                            Some(base.pow(exp as u32) as f64)
                        }
                    }
                    _ => None,
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

                    // Other operations not yet supported - return Unknown
                    _ => CobbleType::Unknown,
                }
            }
            Expression::Unary(op, _) => {
                use crate::ast::UnaryOp;

                match op {
                    UnaryOp::Not => CobbleType::Boolean,
                    UnaryOp::Neg | UnaryOp::Pos | UnaryOp::BitNot => CobbleType::Integer,
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

    pub fn set_pack_format(&mut self, format: u8) {
        self.data_pack.set_pack_format(format);
    }

    pub fn transpile(&mut self, program: &Program) -> Result<(), String> {
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

            for (var_name, value) in &self.module_level_vars {
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
                    continue;
                }

                match value {
                    Expression::String(_s) => {
                        // Strings can't be directly stored in scoreboards
                        return Err(format!(
                            "Module-level string variable '{}' is not supported.\n\
                             \n\
                             Minecraft scoreboards only support integer values.\n\
                             \n\
                             Solutions:\n\
                             1. Use function parameters with macros:\n\
                             \n\
                             def my_function({}):\n\
                                 /say {{{}}}\n\
                             \n\
                             2. Use string literals directly in commands:\n\
                             \n\
                             def my_function():\n\
                                 /say Hello World\n\
                             \n\
                             3. Remove the string variable and use constants in your code",
                            var_name, var_name, var_name
                        ));
                    }
                    _ => {
                        // Other complex expressions at module level
                        eprintln!("Note: Complex expression for '{}' cannot be initialized at module level", var_name);
                    }
                }
            }

            // Add these commands after gamerule and objective creation
            if !init_commands.is_empty() {
                if let Some(existing_init) = self.data_pack.functions.get_mut("_cobble_init") {
                    // Find the position after gamerule and all objectives
                    let setup_end = existing_init
                        .iter()
                        .position(|cmd| {
                            !cmd.starts_with("gamerule")
                                && !cmd.starts_with("scoreboard objectives add")
                        })
                        .unwrap_or(existing_init.len());

                    // Insert module vars after setup commands
                    for (i, cmd) in init_commands.iter().enumerate() {
                        existing_init.insert(setup_end + i, cmd.clone());
                    }
                } else {
                    // Find the load handler function
                    let load_handlers = self.data_pack.stdlib.get_event_handlers(&EventType::Load);
                    if let Some(handler_name) = load_handlers.first() {
                        if let Some(handler_func) = self.data_pack.functions.get_mut(handler_name) {
                            // Find the position after gamerule and all objectives
                            let setup_end = handler_func
                                .iter()
                                .position(|cmd| {
                                    !cmd.starts_with("gamerule")
                                        && !cmd.starts_with("scoreboard objectives add")
                                })
                                .unwrap_or(handler_func.len());

                            for (i, cmd) in init_commands.iter().enumerate() {
                                handler_func.insert(setup_end + i, cmd.clone());
                            }
                        }
                    } else {
                        // No _cobble_init and no load handler - create _cobble_init
                        self.data_pack
                            .functions
                            .insert("_cobble_init".to_string(), init_commands);
                        self.data_pack
                            .stdlib
                            .add_event_listener(EventType::Load, "_cobble_init".to_string());
                    }
                }
            }
        }

        Ok(())
    }

    fn process_import(&mut self, import: &Import) -> Result<(), String> {
        // Handle stdlib imports
        if import.module == "stdlib" {
            // stdlib is automatically available, no action needed
            return Ok(());
        }

        // Handle file imports
        let import_path = self.current_file_dir.join(format!("{}.cbl", import.module));

        // Canonicalize path for accurate comparison
        let canonical_path = import_path.canonicalize().unwrap_or(import_path.clone());

        // Check if already imported
        if self.imported_files.contains(&canonical_path) {
            // Check if this creates a circular dependency
            if self.import_stack.contains(&canonical_path) {
                // Build the circular import chain message for warning
                let mut chain = Vec::new();
                for path in &self.import_stack {
                    if let Some(name) = path.file_stem() {
                        chain.push(name.to_string_lossy().to_string());
                    }
                }
                chain.push(import.module.clone());

                eprintln!(
                    "⚠️  Warning: Circular import detected: {} → {}\n\
                    Each file will only be processed once, but circular imports may indicate a design issue.",
                    chain.join(" → "),
                    chain.first().unwrap_or(&"<unknown>".to_string())
                );
            }
            return Ok(()); // Already imported, skip to avoid infinite loop
        }

        // Check if file exists
        if !canonical_path.exists() && !import_path.exists() {
            return Err(format!(
                "Cannot import '{}': file '{}' not found",
                import.module,
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

        // Set directory for nested imports
        if let Some(parent) = import_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }

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

        // Pop from import stack after processing
        self.import_stack.pop();

        Ok(())
    }

    fn strip_command_prefix(cmd: &str) -> String {
        if let Some(stripped) = cmd.strip_prefix('/') {
            stripped.to_string()
        } else {
            cmd.to_string()
        }
    }

    fn process_statement(&mut self, statement: &Statement) -> Result<(), String> {
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
            Statement::Return(_) => {
                // Return statements in minecraft functions don't have a direct equivalent
                // We could potentially use function return values in future versions
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
                let processor = CommandProcessor::new(
                    &self.current_context.params,
                    &self.scoreboard_variables,
                    &self.variables,
                    &self.variable_objectives,
                    &self.selector_aliases,
                    &self.compile_time_constants,
                );

                let processed_cmd = processor.process_command_string(&clean_cmd)?;

                if let Some(ref mut commands) = self.current_function {
                    commands.push(processed_cmd);
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
        }
        Ok(())
    }

    fn process_function_def(&mut self, func: &FunctionDef) -> Result<(), String> {
        // Save previous function context
        let previous_function = self.current_function.take();
        let previous_context = self.current_context.clone();
        let previous_globals = self.global_variables.clone();
        let previous_variables = self.variables.clone();
        let previous_variable_types = self.variable_types.clone();

        // Extract parameter names from Parameter structs
        let param_names: Vec<String> = func.params.iter().map(|p| p.name.clone()).collect();

        // Set up new function context
        self.current_context = FunctionContext::with_params(param_names.clone());
        self.current_function = Some(Vec::new());
        self.global_variables.clear();

        // Store function parameters
        self.function_params.insert(func.name.clone(), param_names);

        // Process function body
        for statement in &func.body {
            self.process_statement(statement)?;
        }

        // Get the generated commands
        if let Some(commands) = self.current_function.take() {
            self.data_pack.add_function(func.name.clone(), commands);
        }

        // Restore previous context
        self.current_function = previous_function;
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

    fn generate_function_call(
        &mut self,
        func_name: &str,
        args: &[Expression],
    ) -> Result<(), String> {
        let mut commands = Vec::new();

        if let Some(param_names) = self.function_params.get(func_name) {
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
                                } else if self.scoreboard_variables.contains(var) || self.variable_objectives.contains_key(var) {
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
                                } else {
                                    // Unknown identifier - treat as string literal (for items, etc.)
                                    commands.push(format!(
                                        "data modify storage {}:global args.{} set value \"{}\"",
                                        self.data_pack.namespace, param_name, var
                                    ));
                                }
                            }
                            _ => {
                                // For complex expressions, try to evaluate as string
                                commands.push(
                                    "# Warning: Complex argument expression not fully supported"
                                        .to_string(),
                                );
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

    fn translate_condition(&self, condition: &Expression) -> Result<String, String> {
        let translator =
            ConditionTranslator::new(&self.variable_objectives, &self.compile_time_constants);
        translator.translate(condition)
    }

    fn handle_or_condition(&mut self, or_expr: &str) -> Result<String, String> {
        // Flatten all OR conditions into a single list
        let conditions = self.flatten_or_conditions(or_expr)?;

        // Generate commands for OR logic
        if let Some(ref mut commands) = self.current_function {
            // Initialize or_result to 0
            commands.push("scoreboard players set or_result temp 0".to_string());

            // For each condition, if true, set or_result to 1
            for cond in &conditions {
                let cond_prefix = if cond.starts_with("if ") || cond.starts_with("unless ") {
                    cond.clone()
                } else {
                    format!("if {}", cond)
                };
                commands.push(format!(
                    "execute {} run scoreboard players set or_result temp 1",
                    cond_prefix
                ));
            }
        }

        // Return the condition that checks if or_result is 1
        Ok("score or_result temp matches 1".to_string())
    }

    fn flatten_or_conditions(&self, or_expr: &str) -> Result<Vec<String>, String> {
        let mut conditions = Vec::new();

        if !or_expr.starts_with("OR(") {
            // Not an OR expression, return as-is
            return Ok(vec![or_expr.to_string()]);
        }

        // Extract inner content
        let inner = &or_expr[3..or_expr.len() - 1];
        let (cond1, cond2) = self.split_or_conditions(inner)?;

        // Recursively flatten left side
        if cond1.starts_with("OR(") {
            conditions.extend(self.flatten_or_conditions(cond1)?);
        } else {
            conditions.push(cond1.to_string());
        }

        // Recursively flatten right side
        if cond2.starts_with("OR(") {
            conditions.extend(self.flatten_or_conditions(cond2)?);
        } else {
            conditions.push(cond2.to_string());
        }

        Ok(conditions)
    }

    fn split_or_conditions<'a>(&self, inner: &'a str) -> Result<(&'a str, &'a str), String> {
        // Find the comma that separates cond1 and cond2
        // Need to handle nested parentheses
        let mut depth = 0;
        let mut comma_pos = None;
        for (i, ch) in inner.chars().enumerate() {
            match ch {
                '(' => depth += 1,
                ')' => depth -= 1,
                ',' if depth == 0 => {
                    comma_pos = Some(i);
                    break;
                }
                _ => {}
            }
        }

        if let Some(pos) = comma_pos {
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
}
