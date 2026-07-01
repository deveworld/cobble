use crate::ast::*;
use crate::parser::{ProgramSpanTable, StatementSpan};

use super::{GeneratedCommand, GeneratedCommandKind, SourceLocation, Transpiler};
use std::collections::{HashMap, VecDeque};
use std::path::Path;

impl Transpiler {
    pub fn set_current_file(&mut self, file_path: &Path) {
        if let Some(parent) = file_path.parent() {
            self.current_file_dir = parent.to_path_buf();
        }

        // Add main file to import stack and imported_files for circular import detection.
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

    pub fn set_current_file_with_source_spans(
        &mut self,
        file_path: &Path,
        source: &str,
        spans: &ProgramSpanTable,
    ) {
        self.set_current_file(file_path);
        self.register_source_locations_from_spans(file_path, source, spans);
    }

    pub(in crate::transpiler) fn register_source_locations(
        &mut self,
        file_path: &Path,
        source: &str,
    ) {
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
                let source_location = SourceLocation {
                    file: canonical_path.clone(),
                    line: line_index + 1,
                    column,
                };
                raw_locations
                    .entry(command.trim().to_string())
                    .or_default()
                    .push_back(source_location.clone());
                let key = Self::source_line_key(command.trim());
                if !key.is_empty() {
                    statement_locations
                        .entry(key)
                        .or_default()
                        .push_back(source_location);
                }
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

    pub(in crate::transpiler) fn register_source_locations_from_spans(
        &mut self,
        file_path: &Path,
        source: &str,
        spans: &ProgramSpanTable,
    ) {
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        let mut raw_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();
        let mut statement_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();

        for statement_span in &spans.statements {
            Self::register_statement_span_source(
                &canonical_path,
                source,
                statement_span,
                &mut raw_locations,
                &mut statement_locations,
            );
        }

        self.raw_command_locations
            .insert(canonical_path.clone(), raw_locations);
        self.statement_locations
            .insert(canonical_path, statement_locations);
    }

    fn register_statement_span_source(
        canonical_path: &Path,
        source: &str,
        statement_span: &StatementSpan,
        raw_locations: &mut HashMap<String, VecDeque<SourceLocation>>,
        statement_locations: &mut HashMap<String, VecDeque<SourceLocation>>,
    ) {
        if let Some(header) = source.get(statement_span.span.start..statement_span.span.end) {
            let trimmed = header.trim();
            if !trimmed.is_empty() {
                let source_location = SourceLocation {
                    file: canonical_path.to_path_buf(),
                    line: statement_span.span.line,
                    column: statement_span.span.column,
                };

                if let Some(command) = trimmed.strip_prefix('/') {
                    let command = command.trim();
                    raw_locations
                        .entry(command.to_string())
                        .or_default()
                        .push_back(source_location.clone());
                    let key = Self::source_line_key(command);
                    if !key.is_empty() {
                        statement_locations
                            .entry(key)
                            .or_default()
                            .push_back(source_location);
                    }
                } else {
                    let key = Self::source_line_key(trimmed);
                    if !key.is_empty() {
                        statement_locations
                            .entry(key)
                            .or_default()
                            .push_back(source_location);
                    }
                }
            }
        }

        for child in &statement_span.children {
            Self::register_statement_span_source(
                canonical_path,
                source,
                child,
                raw_locations,
                statement_locations,
            );
        }
    }

    pub(in crate::transpiler) fn take_raw_command_source(
        &mut self,
        command: &str,
    ) -> Option<SourceLocation> {
        let file_path = self.current_file_path.as_ref()?;
        self.raw_command_locations
            .get_mut(file_path)
            .and_then(|commands| commands.get_mut(command))
            .and_then(VecDeque::pop_front)
    }

    pub(in crate::transpiler) fn take_raw_command_statement_source(
        &mut self,
        command: &str,
    ) -> Option<SourceLocation> {
        let key = Self::source_line_key(command);
        let file_path = self.current_file_path.as_ref()?;
        self.statement_locations
            .get_mut(file_path)
            .and_then(|statements| statements.get_mut(&key))
            .and_then(VecDeque::pop_front)
    }

    pub(in crate::transpiler) fn take_statement_source(
        &mut self,
        statement: &Statement,
    ) -> Option<SourceLocation> {
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

    pub(crate) fn source_line_key(source: &str) -> String {
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
            Statement::For(for_loop) => Self::for_loop_source_key(for_loop),
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

    pub(in crate::transpiler) fn for_loop_source_key(for_loop: &ForLoop) -> String {
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
        Self::normalize_source_key(&source)
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

    pub(in crate::transpiler) fn annotate_recent_commands(
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
}
