use crate::ast::{Assignment, CobbleType, Expression, FunctionDef, Program, Statement};
use crate::transpiler::function_context::{
    expression_reads, first_assigned_value, global_assignments, global_literal_assignments,
    read_names, FunctionContext,
};
use crate::transpiler::storage_paths::{StoragePathOps, StorageValueOps};
use crate::transpiler::{
    FunctionCapture, GeneratedCommand, GeneratedCommandKind, SourceLocation, TranspileError,
    Transpiler,
};
use std::collections::HashMap;

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
    /// Module-level code that runs commands, such as a loop over a list or
    /// a call, lowered where it stands in the file.
    Code(FunctionCapture),
}

/// `expr` with each local of `function` replaced by the value the function
/// first assigns it, `depth` levels deep, so its type can be told before
/// the function is lowered. Parameters and names the function declares
/// `global` stay.
fn substitute_locals(expr: &Expression, function: &FunctionDef, depth: usize) -> Expression {
    let again = |expr: &Expression| Box::new(substitute_locals(expr, function, depth));
    match expr {
        Expression::Identifier(name)
            if depth > 0
                && !function.params.iter().any(|param| &param.name == name)
                && !global_assignments(function)
                    .iter()
                    .any(|(global, _)| global == name) =>
        {
            match first_assigned_value(&function.body, name) {
                Some(value) => substitute_locals(value, function, depth - 1),
                None => expr.clone(),
            }
        }
        Expression::Binary(left, op, right) => {
            Expression::Binary(again(left), op.clone(), again(right))
        }
        Expression::Unary(op, inner) => Expression::Unary(op.clone(), again(inner)),
        Expression::Conditional(condition, value, otherwise) => {
            Expression::Conditional(again(condition), again(value), again(otherwise))
        }
        other => other.clone(),
    }
}

impl Transpiler {
    /// Records the type and backing of every module variable `program`
    /// declares, before any function body is lowered. A function body runs
    /// after the whole module, so, as in Python, it can use a name declared
    /// below it. Each name is recorded the way its first module-level
    /// assignment records it.
    pub(in crate::transpiler) fn register_module_declarations(&mut self, program: &Program) {
        // A `const` has its value before any function body is lowered, so
        // a function above it reads it. Module-level code still can't read
        // it before its line.
        for statement in &program.statements {
            let Statement::ConstAssignment(constant) = statement else {
                continue;
            };
            let name = &constant.target;
            self.module_consts.insert(name.clone());
            if self.is_declared_name(name) {
                continue;
            }
            let Ok(value) = self.evaluate_const_expr(&constant.value) else {
                continue;
            };
            self.symbols
                .pending_module_declarations
                .insert(name.clone());
            self.symbols
                .variable_types
                .insert(name.clone(), self.infer_type(&constant.value));
            self.symbols
                .compile_time_constants
                .insert(name.clone(), value);
            self.symbols
                .variables
                .insert(name.clone(), Expression::Number(value));
        }
        for statement in &program.statements {
            let assignments: Vec<Assignment> = match statement {
                Statement::Assignment(assign) if assign.assigns_name() => vec![assign.clone()],
                Statement::MultiAssign(multi) if multi.targets.len() == multi.values.len() => multi
                    .targets
                    .iter()
                    .zip(&multi.values)
                    .filter_map(|(target, value)| match target {
                        Expression::Identifier(name) => Some(Assignment {
                            target: name.clone(),
                            target_expr: None,
                            value: value.clone(),
                            persist: false,
                        }),
                        _ => None,
                    })
                    .collect(),
                // A module-level loop that keeps its variable assigns it.
                Statement::For(for_loop)
                    if self.module_loop_variables.contains(&for_loop.target) =>
                {
                    match self.expanded_loop_last_value(for_loop) {
                        Some(value) => vec![Assignment {
                            target: for_loop.target.clone(),
                            target_expr: None,
                            value,
                            persist: false,
                        }],
                        None => {
                            self.register_runtime_loop_variable(for_loop);
                            Vec::new()
                        }
                    }
                }
                _ => Vec::new(),
            };
            for assign in assignments {
                let name = &assign.target;
                if self.is_declared_name(name)
                    || self.symbols.late_module_declarations.contains(name)
                {
                    continue;
                }
                self.symbols
                    .pending_module_declarations
                    .insert(name.clone());
                // Lowering can still change what other values are, such as
                // `[0] * 3`, so their type is known only at the assignment.
                let known_here = self.try_eval_const(&assign.value).is_some()
                    || matches!(
                        &assign.value,
                        Expression::Number(_)
                            | Expression::Boolean(_)
                            | Expression::String(_)
                            | Expression::Array(_)
                            | Expression::Map(_)
                    )
                    || (matches!(
                        &assign.value,
                        Expression::Identifier(_)
                            | Expression::Subscript(..)
                            | Expression::Attribute(..)
                    ) && {
                        // A copy, item, or field of a variable known here.
                        let mut reads = Vec::new();
                        expression_reads(&assign.value, &mut reads);
                        reads.first().is_some_and(|root| {
                            self.is_declared_name(root)
                                && !self.symbols.late_module_declarations.contains(*root)
                        })
                    });
                // A value lowering doesn't change has the type its assignment
                // gives it. Lowering turns any other value into one of a
                // known type, such as `[0] * 3` into a list, which the
                // value's type before lowering tells.
                let value_type = if known_here {
                    self.assigned_value_type(&assign.value)
                } else {
                    match self.value_type(&assign.value) {
                        known @ (CobbleType::String | CobbleType::List | CobbleType::Map) => known,
                        // A number from variables known here, or from a
                        // call, is a score, which a function above reads
                        // without knowing more.
                        CobbleType::Integer | CobbleType::Boolean
                            if {
                                let mut reads = Vec::new();
                                expression_reads(&assign.value, &mut reads);
                                reads.iter().all(|read| {
                                    self.is_declared_name(read)
                                        && !self.symbols.late_module_declarations.contains(*read)
                                })
                            } =>
                        {
                            continue;
                        }
                        _ => {
                            self.symbols.late_module_declarations.insert(name.clone());
                            continue;
                        }
                    }
                };
                self.symbols.variable_types.insert(name.clone(), value_type);
                if known_here {
                    self.symbols
                        .variables
                        .insert(name.clone(), assign.value.clone());
                }
                self.register_module_variable_backing(&assign);
            }
        }
        // A selector alias or an entity template declared below a function
        // is known there too. A name declared twice keeps its first value
        // until the second declaration's own line.
        for statement in &program.statements {
            match statement {
                Statement::SelectorDef(def)
                    if !self.symbols.selector_aliases.contains_key(&def.name) =>
                {
                    let _ = self.process_selector_def(def);
                }
                Statement::EntityDef(def)
                    if !self.symbols.entity_templates.contains_key(&def.name) =>
                {
                    let _ = self.process_entity_def(def);
                }
                _ => {}
            }
        }
        // A function that declares a name `global` and assigns it a literal
        // declares a module variable too.
        for statement in &program.statements {
            let Statement::FunctionDef(function) = statement else {
                continue;
            };
            for (name, value) in global_literal_assignments(function) {
                if self.is_declared_name(&name)
                    || self.symbols.late_module_declarations.contains(&name)
                {
                    continue;
                }
                self.symbols
                    .variable_types
                    .insert(name.clone(), self.infer_type(&value));
                self.register_module_variable_backing(&Assignment {
                    target: name,
                    target_expr: None,
                    value,
                    persist: false,
                });
            }
        }
        // Any other value a function assigns under `global` declares a
        // module variable of the value's type: text, a list, or a map in
        // storage, a number as a score, and a value whose type only the
        // running pack knows, such as a parameter, in both.
        let module_level: std::collections::HashSet<&str> = program
            .statements
            .iter()
            .flat_map(|statement| -> Vec<&str> {
                match statement {
                    Statement::Assignment(assign) if assign.assigns_name() => {
                        vec![assign.target.as_str()]
                    }
                    Statement::MultiAssign(multi) => multi
                        .targets
                        .iter()
                        .filter_map(|target| match target {
                            Expression::Identifier(name) => Some(name.as_str()),
                            _ => None,
                        })
                        .collect(),
                    Statement::For(for_loop) => vec![for_loop.target.as_str()],
                    Statement::ConstAssignment(constant) => vec![constant.target.as_str()],
                    _ => Vec::new(),
                }
            })
            .collect();
        for statement in &program.statements {
            let Statement::FunctionDef(function) = statement else {
                continue;
            };
            for (name, value) in global_assignments(function) {
                if module_level.contains(name.as_str())
                    || self.imports.later_variables.contains_key(&name)
                    || self.is_declared_name(&name)
                    || self.symbols.late_module_declarations.contains(&name)
                {
                    continue;
                }
                // The type is known here only for a value of module names
                // and literals, with each local replaced by the value the
                // function first assigns it. A call's result, and a
                // parameter, are stored both ways.
                let value = substitute_locals(&value, function, 3);
                let mut reads = Vec::new();
                expression_reads(&value, &mut reads);
                let module_value = reads.iter().all(|read| {
                    self.is_declared_name(read) && !function.params.iter().any(|p| p.name == *read)
                }) && (!matches!(value, Expression::Call(..))
                    || matches!(
                        self.value_type(&value),
                        CobbleType::String | CobbleType::List | CobbleType::Map
                    ));
                let value_type = if module_value {
                    self.value_type(&value)
                } else {
                    CobbleType::Unknown
                };
                let storage = matches!(
                    value_type,
                    CobbleType::String | CobbleType::List | CobbleType::Map | CobbleType::Unknown
                );
                let score = !matches!(
                    value_type,
                    CobbleType::String | CobbleType::List | CobbleType::Map
                );
                self.symbols.variable_types.insert(name.clone(), value_type);
                if storage {
                    self.symbols
                        .variable_storage_paths
                        .insert(name.clone(), format!("vars.{name}"));
                }
                if score {
                    self.data_pack.track_objective("temp");
                    self.symbols
                        .variable_objectives
                        .insert(name.clone(), "temp".to_string());
                    self.symbols.scoreboard_variables.insert(name.clone());
                }
            }
        }
    }

    /// Module-level code runs in order: it can't read a module variable
    /// before the variable's own module-level assignment.
    pub(in crate::transpiler) fn check_module_level_reads(
        &self,
        value: &Expression,
    ) -> Result<(), TranspileError> {
        let mut reads = Vec::new();
        expression_reads(value, &mut reads);
        match reads
            .into_iter()
            .find(|name| self.symbols.pending_module_declarations.contains(*name))
        {
            Some(name) => Err(format!(
                "Module variable `{name}` is read here before its assignment further down the file. Module-level code runs from top to bottom, so move `{name} = ...` above this line."
            )
            .into()),
            None => Ok(()),
        }
    }

    /// A function above the module-level assignment of a variable whose
    /// type only that assignment decides can't use the variable.
    pub(in crate::transpiler) fn check_late_module_reads(
        &self,
        function: &FunctionDef,
        context: &FunctionContext,
    ) -> Result<(), TranspileError> {
        let late = &self.symbols.late_module_declarations;
        if late.is_empty() {
            return Ok(());
        }
        // One walk of the body, then the first late name it reads in name
        // order, so the error doesn't depend on hash order.
        let reads = read_names(&function.body);
        match reads
            .iter()
            .filter(|name| {
                late.contains(*name) && !context.is_param(name) && !context.is_local(name)
            })
            .min()
        {
            Some(name) => Err(format!(
                "`{name}` is declared later in the file, and `{}` above it uses it. Move the module-level `{name} = ...` above `def {}`.",
                function.name, function.name
            )
            .into()),
            None => Ok(()),
        }
    }

    /// Records the variable of a module-level runtime loop as a module
    /// variable, before function bodies are lowered: a score for a `range`,
    /// and otherwise an item of a type the running pack knows, in storage
    /// and as a score, as the loop binds it.
    fn register_runtime_loop_variable(&mut self, for_loop: &crate::ast::ForLoop) {
        let name = &for_loop.target;
        if self.is_declared_name(name) || self.symbols.late_module_declarations.contains(name) {
            return;
        }
        self.symbols
            .pending_module_declarations
            .insert(name.clone());
        self.data_pack.track_objective("temp");
        self.symbols
            .variable_objectives
            .insert(name.clone(), "temp".to_string());
        self.symbols.scoreboard_variables.insert(name.clone());
        let is_range = matches!(
            &for_loop.iter,
            Expression::Call(func, _) if matches!(&**func, Expression::Identifier(f) if f == "range")
        );
        if is_range {
            self.symbols
                .variable_types
                .insert(name.clone(), CobbleType::Integer);
        } else {
            self.symbols
                .variable_types
                .insert(name.clone(), CobbleType::Unknown);
            self.symbols
                .variable_storage_paths
                .insert(name.clone(), format!("vars.{name}"));
        }
    }

    /// Whether the module-level `statement` is code that runs commands, as
    /// a loop over a list, a `while`, a call, or a raw command, rather than
    /// a declaration. Such code runs in `_cobble_init`, in source order with
    /// the module variables' assignments. A loop expanded at compile time,
    /// or an `if` on a constant, whose body only assigns, stays a list of
    /// declarations.
    pub(in crate::transpiler) fn is_module_code(&self, statement: &Statement) -> bool {
        fn declarations_only(body: &[Statement]) -> bool {
            body.iter().all(|statement| {
                matches!(
                    statement,
                    Statement::Assignment(_)
                        | Statement::MultiAssign(_)
                        | Statement::ConstAssignment(_)
                        | Statement::Pass
                )
            })
        }
        match statement {
            Statement::For(for_loop) => {
                !self.expands_at_compile_time(for_loop) || !declarations_only(&for_loop.body)
            }
            Statement::If(if_stmt) => {
                self.try_eval_const(&if_stmt.condition).is_none()
                    || !declarations_only(&if_stmt.then_branch)
                    || if_stmt.elif_branches.iter().any(|(condition, branch)| {
                        self.try_eval_const(condition).is_none() || !declarations_only(branch)
                    })
                    || !if_stmt.else_branch.as_deref().is_none_or(declarations_only)
            }
            Statement::While(_)
            | Statement::Match(_)
            | Statement::Expression(_)
            | Statement::MinecraftCommand(_)
            | Statement::Execute(_) => true,
            _ => false,
        }
    }

    /// Lowers the module-level code `statement` where it stands, into
    /// commands that `_cobble_init` runs after the module variables
    /// assigned above it. The names it assigns are module variables.
    pub(in crate::transpiler) fn process_module_code(
        &mut self,
        statement: &Statement,
    ) -> Result<(), TranspileError> {
        if let Statement::For(for_loop) = statement {
            self.symbols
                .pending_module_declarations
                .remove(&for_loop.target);
        }
        let saved_context = std::mem::replace(&mut self.current_context, FunctionContext::new());
        let saved_return = self.current_return.take();
        let saved_flag = std::mem::replace(&mut self.in_module_code, true);
        let capture = self.capture_commands(|transpiler| transpiler.process_statement(statement));
        self.in_module_code = saved_flag;
        self.current_return = saved_return;
        self.current_context = saved_context;
        self.module_level_vars
            .push(ModuleLevelAssignment::Code(capture?));
        Ok(())
    }

    /// Whether `name` already names a variable or a constant.
    fn is_declared_name(&self, name: &str) -> bool {
        self.symbols.variable_types.contains_key(name)
            || self.symbols.variable_storage_paths.contains_key(name)
            || self.symbols.scoreboard_variables.contains(name)
            || self.symbols.compile_time_constants.contains_key(name)
    }

    pub(in crate::transpiler) fn build_module_init_commands(
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
                    let mut generated = generated
                        .map_err(|error| error.with_location_if_missing(source.clone()))?;
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
                    let generated = generated
                        .map_err(|error| error.with_location_if_missing(source.clone()))?;
                    Self::append_module_init_commands(
                        &mut init_commands,
                        &mut init_metadata,
                        generated,
                        source,
                    );
                }
                ModuleLevelAssignment::Code(capture) => {
                    let start = init_commands.len();
                    for (index, generated) in capture.metadata {
                        init_metadata.insert(start + index, generated);
                    }
                    init_commands.extend(capture.commands);
                }
            }
        }

        Ok((init_commands, init_metadata))
    }

    /// Wraps a `persist` variable's initializer so it only runs while the
    /// value is unset. A multi-command initializer moves into a helper
    /// function behind the same guard.
    pub(in crate::transpiler) fn guard_persistent_init(
        &mut self,
        name: &str,
        commands: Vec<String>,
        source: &Option<SourceLocation>,
    ) -> Vec<String> {
        let namespace = self.data_pack.namespace.clone();
        let guard = match self.symbols.variable_storage_paths.get(name) {
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
    pub(in crate::transpiler) fn is_persist_initializer(&self, value: &Expression) -> bool {
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

    pub(in crate::transpiler) fn append_module_init_commands(
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

    pub(in crate::transpiler) fn module_variable_init_commands(
        &mut self,
        var_name: &str,
        value: &Expression,
    ) -> Result<Vec<String>, TranspileError> {
        if let Some(const_value) = self.try_eval_const(value) {
            let truncated = const_value as i32;
            self.warn_score_value(const_value);

            return Ok(vec![format!(
                "scoreboard players set {} temp {}",
                var_name, truncated
            )]);
        }

        // A copy of a string, list, or map variable, or a character of a
        // string, is stored like a literal.
        let storage_path = format!("vars.{}", var_name);
        if let Expression::Subscript(base, index) = value {
            if self.is_string_base(base) {
                self.symbols
                    .variable_storage_paths
                    .insert(var_name.to_string(), storage_path.clone());
                return self.string_char_commands(&storage_path, base, index);
            }
        }
        if let Expression::Identifier(name) = value {
            if let Some(source_path) = self.symbols.variable_storage_paths.get(name).cloned() {
                self.symbols
                    .variable_storage_paths
                    .insert(var_name.to_string(), storage_path.clone());
                return Ok(vec![format!(
                    "data modify storage {ns}:global {storage_path} set from storage {ns}:global {source_path}",
                    ns = self.data_pack.namespace
                )]);
            }
        }
        // An item or field that isn't a number is copied in storage, with a
        // score too when its type is unknown.
        if matches!(value, Expression::Subscript(..) | Expression::Attribute(..)) {
            let read_type = self.storage_read_type(value);
            if read_type != CobbleType::Integer && self.resolve_storage_path_plan(value).is_some() {
                // One that isn't there gives an empty value, or 0.
                let mut commands = self.storage_leaf_set_commands(&storage_path, value)?;
                if read_type == CobbleType::Unknown {
                    commands.push(format!(
                        "execute store result score {var_name} temp run data get storage {}:global {storage_path}",
                        self.data_pack.namespace
                    ));
                }
                return Ok(commands);
            }
        }

        match value {
            Expression::Array(_) | Expression::Map(_) | Expression::String(_) => {
                let storage_path = format!("vars.{}", var_name);
                self.symbols
                    .variable_storage_paths
                    .insert(var_name.to_string(), storage_path.clone());
                self.storage_value_commands(&storage_path, value)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
            // Comparing text, lists, or maps takes commands of its own.
            Expression::Binary(left, op, right)
                if matches!(op, crate::ast::BinaryOp::Eq | crate::ast::BinaryOp::NotEq)
                    && [left, right].iter().any(|side| {
                        matches!(
                            self.infer_type(side),
                            CobbleType::String | CobbleType::List | CobbleType::Map
                        )
                    }) =>
            {
                Err(format!(
                    "Failed to initialize module variable '{var_name}': comparing text, lists, or maps takes commands, so it can't set a module variable. Assign it inside a function."
                )
                .into())
            }
            _ => {
                self.data_pack.track_objective("temp");
                self.symbols
                    .variable_objectives
                    .insert(var_name.to_string(), "temp".to_string());
                self.symbols
                    .scoreboard_variables
                    .insert(var_name.to_string());
                self.evaluate_expression_to_target(value, var_name)
                    .map_err(|e| {
                        format!("Failed to initialize module variable '{}': {}", var_name, e)
                    })
                    .map_err(TranspileError::from)
            }
        }
    }
}
