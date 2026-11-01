use crate::ast::*;
use crate::parser::{ProgramSpanTable, StatementSpan};

use super::{GeneratedCommand, GeneratedCommandKind, SourceLocation, Transpiler};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};

/// Where each statement and raw command of each source file starts, queued
/// in source order. Lowering takes the next location for the text it is
/// compiling.
#[derive(Default)]
pub(in crate::transpiler) struct SourceLocations {
    pub(in crate::transpiler) raw_commands:
        HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    pub(in crate::transpiler) statements:
        HashMap<PathBuf, HashMap<String, VecDeque<SourceLocation>>>,
    /// The keys of `statements` by their text without parentheses, built
    /// when a statement first misses its own key.
    pub(in crate::transpiler) bare_statement_keys: HashMap<PathBuf, HashMap<String, Vec<String>>>,
    /// Where each top-level import of each file starts, in import order.
    pub(in crate::transpiler) imports: HashMap<PathBuf, Vec<SourceLocation>>,
    /// The text of each source file, with continued raw commands joined.
    pub(in crate::transpiler) sources: HashMap<PathBuf, String>,
}

impl Transpiler {
    pub fn set_current_file(&mut self, file_path: &Path) {
        if let Some(parent) = file_path.parent() {
            self.imports.current_dir = parent.to_path_buf();
        }

        // Add main file to import stack and imported_files for circular import detection.
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        if !self.imports.stack.contains(&canonical_path) {
            self.imports.stack.push(canonical_path.clone());
            self.imports.files.insert(canonical_path.clone());
        }
        self.imports.current_file = Some(canonical_path);
    }

    /// Whether this transpiler already compiled `file_path`, either as the
    /// current file or through an import. A project build skips such a file
    /// when it reaches it in the source folder.
    pub fn has_compiled(&self, file_path: &Path) -> bool {
        let canonical_path = file_path
            .canonicalize()
            .unwrap_or_else(|_| file_path.to_path_buf());
        self.imports.files.contains(&canonical_path)
    }

    /// How many files this transpiler compiled, imported files included.
    pub fn compiled_file_count(&self) -> usize {
        self.imports.files.len()
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
        // A raw command continued with `\` is one command on its first line.
        let source = &*crate::parser::tokenizer::join_continued_commands(source);
        let mut raw_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();
        let mut statement_locations: HashMap<String, VecDeque<SourceLocation>> = HashMap::new();
        let mut pending_statement: Option<(String, SourceLocation, i32)> = None;
        let mut import_locations = Vec::new();

        for (line_index, line) in source.lines().enumerate() {
            let trimmed = line.trim_start();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if pending_statement.is_none()
                && (line.starts_with("import ") || line.starts_with("from "))
            {
                import_locations.push(SourceLocation {
                    file: canonical_path.clone(),
                    line: line_index + 1,
                    column: 1,
                });
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

        self.source_locations
            .raw_commands
            .insert(canonical_path.clone(), raw_locations);
        self.source_locations
            .statements
            .insert(canonical_path.clone(), statement_locations);
        self.source_locations
            .bare_statement_keys
            .remove(&canonical_path);
        self.source_locations
            .sources
            .insert(canonical_path.clone(), source.to_string());
        self.source_locations
            .imports
            .insert(canonical_path, import_locations);
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
        // A raw command continued with `\` is one command on its first line.
        // Joining keeps every offset, so the spans still apply.
        let source = &*crate::parser::tokenizer::join_continued_commands(source);
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

        let import_locations = spans
            .imports
            .iter()
            .map(|span| SourceLocation {
                file: canonical_path.clone(),
                line: span.line,
                column: span.column,
            })
            .collect();

        self.source_locations
            .raw_commands
            .insert(canonical_path.clone(), raw_locations);
        self.source_locations
            .statements
            .insert(canonical_path.clone(), statement_locations);
        self.source_locations
            .bare_statement_keys
            .remove(&canonical_path);
        self.source_locations
            .sources
            .insert(canonical_path.clone(), source.to_string());
        self.source_locations
            .imports
            .insert(canonical_path, import_locations);
    }

    /// The digits of the integer literal with the value `value` in the
    /// statement being lowered: `Ok(Some(digits))` for one such literal,
    /// `Ok(None)` when it is written as a float (`1e20`, `3.0e9`), and
    /// otherwise the integer literals that read as this value: none when
    /// the source can't tell, several when they can't be told apart. An
    /// `f64` can't hold every integer past 2^53, so resource JSON reads them
    /// from the source.
    pub(in crate::transpiler) fn integer_literal_text(
        &self,
        value: f64,
    ) -> Result<Option<String>, Vec<String>> {
        // The statement's own lines, or the whole file when the statement's
        // location isn't known.
        let location = self.current_statement_source.as_ref();
        let file = location
            .map(|location| &location.file)
            .or(self.imports.current_file.as_ref())
            .ok_or_else(Vec::new)?;
        let source = self
            .source_locations
            .sources
            .get(file)
            .ok_or_else(Vec::new)?;
        let first_line = location.map_or(0, |location| location.line.saturating_sub(1));
        let mut statement = String::new();
        let mut depth = 0;
        for line in source.lines().skip(first_line) {
            let line = Self::strip_inline_comment(line).unwrap_or(line);
            depth += Self::bracket_depth_delta(line);
            statement.push_str(line);
            statement.push('\n');
            if location.is_some() && depth <= 0 {
                break;
            }
        }
        let magnitude = value.abs();
        let mut integers = Vec::new();
        let mut float_form = false;
        for literal in Self::number_literals(&statement) {
            if literal.parse::<f64>().ok() != Some(magnitude) {
                continue;
            }
            if literal.bytes().all(|byte| byte.is_ascii_digit()) {
                let digits = literal.trim_start_matches('0');
                let digits = if digits.is_empty() { "0" } else { digits };
                if !integers.iter().any(|known| known == digits) {
                    integers.push(digits.to_string());
                }
            } else if let Some(digits) = Self::exact_integer_digits(literal) {
                // `9007199254740993.0` or `9.007199254740993e15` is that
                // integer exactly, which an `f64` can't hold.
                if !integers.contains(&digits) {
                    integers.push(digits);
                }
            } else {
                float_form = true;
            }
        }
        let sign = if value < 0.0 { "-" } else { "" };
        match (integers.as_slice(), float_form) {
            ([digits], false) => Ok(Some(format!("{sign}{digits}"))),
            ([], true) => Ok(None),
            _ => Err(integers
                .iter()
                .map(|digits| format!("{sign}{digits}"))
                .collect()),
        }
    }

    /// The digits of a float literal (`3.0e9`) whose exact value is a whole
    /// number that fits in 64 bits, or `None`.
    fn exact_integer_digits(literal: &str) -> Option<String> {
        let (mantissa, exponent) = match literal.find(['e', 'E']) {
            Some(index) => (&literal[..index], literal[index + 1..].parse::<i64>().ok()?),
            None => (literal, 0),
        };
        let (whole, fraction) = mantissa.split_once('.').unwrap_or((mantissa, ""));
        if !(whole.bytes().chain(fraction.bytes())).all(|byte| byte.is_ascii_digit()) {
            return None;
        }
        let mut digits = format!("{whole}{fraction}");
        let shift = exponent.checked_sub(i64::try_from(fraction.len()).ok()?)?;
        if shift >= 0 {
            if shift > 20 {
                return None;
            }
            digits.push_str(&"0".repeat(shift as usize));
        } else {
            let drop = usize::try_from(-shift).ok()?;
            if drop > digits.len() || !digits[digits.len() - drop..].bytes().all(|b| b == b'0') {
                return None;
            }
            digits.truncate(digits.len() - drop);
        }
        let digits = digits.trim_start_matches('0');
        let digits = if digits.is_empty() { "0" } else { digits };
        digits.parse::<i64>().ok()?;
        Some(digits.to_string())
    }

    /// The number literals of `text` outside strings, without their sign.
    fn number_literals(text: &str) -> Vec<&str> {
        let bytes = text.as_bytes();
        let mut literals = Vec::new();
        let mut quote: Option<u8> = None;
        let mut index = 0;
        while index < bytes.len() {
            let byte = bytes[index];
            if let Some(active) = quote {
                if byte == b'\\' {
                    index += 1;
                } else if byte == active {
                    quote = None;
                }
                index += 1;
                continue;
            }
            if byte == b'"' || byte == b'\'' {
                quote = Some(byte);
                index += 1;
                continue;
            }
            let follows_name = index > 0
                && (bytes[index - 1].is_ascii_alphanumeric()
                    || bytes[index - 1] == b'_'
                    || bytes[index - 1] == b'.');
            if !byte.is_ascii_digit() || follows_name {
                index += 1;
                continue;
            }
            let start = index;
            while index < bytes.len() && (bytes[index].is_ascii_digit() || bytes[index] == b'.') {
                index += 1;
            }
            if index < bytes.len() && matches!(bytes[index], b'e' | b'E') {
                let mut exponent = index + 1;
                if exponent < bytes.len() && matches!(bytes[exponent], b'+' | b'-') {
                    exponent += 1;
                }
                if exponent < bytes.len() && bytes[exponent].is_ascii_digit() {
                    index = exponent;
                    while index < bytes.len() && bytes[index].is_ascii_digit() {
                        index += 1;
                    }
                }
            }
            literals.push(&text[start..index]);
        }
        literals
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

    /// Where the `def` line of `func` in the current file is. Function
    /// definitions take no location while they are lowered, so this only
    /// looks it up.
    pub(in crate::transpiler) fn function_def_source(
        &self,
        func: &FunctionDef,
    ) -> Option<SourceLocation> {
        let params: Vec<&str> = func
            .params
            .iter()
            .map(|param| param.name.as_str())
            .collect();
        let key = Self::normalize_source_key(&format!("def {}({}):", func.name, params.join(",")));
        let file_path = self.imports.current_file.as_ref()?;
        self.source_locations
            .statements
            .get(file_path)?
            .get(&key)?
            .front()
            .cloned()
    }

    /// Where the current file's import number `index` starts.
    pub(in crate::transpiler) fn import_source(&self, index: usize) -> Option<SourceLocation> {
        let file_path = self.imports.current_file.as_ref()?;
        self.source_locations
            .imports
            .get(file_path)?
            .get(index)
            .cloned()
    }

    pub(in crate::transpiler) fn take_raw_command_source(
        &mut self,
        command: &str,
    ) -> Option<SourceLocation> {
        let file_path = self.imports.current_file.as_ref()?;
        self.source_locations
            .raw_commands
            .get_mut(file_path)
            .and_then(|commands| commands.get_mut(command))
            .and_then(VecDeque::pop_front)
    }

    pub(in crate::transpiler) fn take_raw_command_statement_source(
        &mut self,
        command: &str,
    ) -> Option<SourceLocation> {
        let key = Self::source_line_key(command);
        let file_path = self.imports.current_file.as_ref()?;
        self.source_locations
            .statements
            .get_mut(file_path)
            .and_then(|statements| statements.get_mut(&key))
            .and_then(VecDeque::pop_front)
    }

    pub(in crate::transpiler) fn take_statement_source(
        &mut self,
        statement: &Statement,
    ) -> Option<SourceLocation> {
        let keys = Self::statement_source_keys(statement);
        let file_path = self.imports.current_file.as_ref()?;
        let source_locations = &mut self.source_locations;
        let statements = source_locations.statements.get_mut(file_path)?;
        // Statements are lowered in source order, so the earliest pending
        // location among the candidate keys is this statement's. Taking the
        // first key with any location could hand `asat @p:` the location of a
        // later `as @p at @s:` (and the other way round).
        let key = keys
            .iter()
            .filter_map(|key| {
                let location = statements.get(key)?.front()?;
                Some((key, location.line, location.column))
            })
            .min_by_key(|(_, line, column)| (*line, *column))
            .map(|(key, _, _)| key.clone())
            .or_else(|| {
                // The parsed statement has lost its parentheses, as in
                // `b = (a and h) == h`, so compare without them.
                let bare = |key: &str| key.replace(['(', ')'], "");
                let bare_keys = source_locations
                    .bare_statement_keys
                    .entry(file_path.clone())
                    .or_insert_with(|| {
                        let mut bare_keys: HashMap<String, Vec<String>> = HashMap::new();
                        for key in statements.keys() {
                            bare_keys.entry(bare(key)).or_default().push(key.clone());
                        }
                        bare_keys
                    });
                keys.iter()
                    .filter_map(|key| bare_keys.get(&bare(key)))
                    .flatten()
                    .filter_map(|key| {
                        let location = statements.get(key)?.front()?;
                        Some((key, location.line, location.column))
                    })
                    .min_by_key(|(_, line, column)| (*line, *column))
                    .map(|(key, _, _)| key.clone())
            })?;
        statements.get_mut(&key).and_then(VecDeque::pop_front)
    }

    /// Source keys a statement may be registered under, most specific first.
    /// `asat @x:` parses to the same modifiers as `as @x at @s:`, so an
    /// execute block starting with that pair also tries the `asat` spelling.
    fn statement_source_keys(statement: &Statement) -> Vec<String> {
        let Some(primary) = Self::statement_source_key(statement) else {
            return Vec::new();
        };
        let mut keys = vec![primary];
        // A target such as `box[n + 1]`, whose parsed name can't spell its
        // key.
        if let Statement::Assignment(assign) = statement {
            if let Some(target) = assign
                .target_expr
                .as_ref()
                .filter(|_| !assign.assigns_name())
            {
                let target_key = Self::expression_source_key(target);
                keys.push(Self::normalize_source_key(&format!(
                    "{}={}",
                    target_key,
                    Self::expression_source_key(&assign.value)
                )));
                if let Expression::Binary(left, op, right) = &assign.value {
                    if left.as_ref() == target {
                        keys.push(Self::normalize_source_key(&format!(
                            "{}{}={}",
                            target_key,
                            Self::binary_op_source_key(op),
                            Self::expression_source_key(right)
                        )));
                    }
                }
            }
        }
        // `for i, item in pairs:`
        if let Statement::For(for_loop) = statement {
            if let Some(index) = &for_loop.index {
                let mut source = format!(
                    "for{},{}in{}",
                    index,
                    for_loop.target,
                    Self::expression_source_key(&for_loop.iter)
                );
                if let Some(step) = &for_loop.step {
                    source.push_str("by");
                    source.push_str(&Self::expression_source_key(step));
                }
                source.push(':');
                keys.push(Self::normalize_source_key(&source));
            }
        }
        // `x += 5` parses to `x = x + 5`, but its source line keys as `x+=5`.
        if let Statement::Assignment(assign) = statement {
            if let Expression::Binary(left, op, right) = &assign.value {
                // `box[k] += 1` too, whose target is a subscript.
                let augments_target = matches!(left.as_ref(), Expression::Identifier(name) if name == &assign.target)
                    || assign.target_expr.as_ref() == Some(left.as_ref());
                if augments_target {
                    keys.push(format!(
                        "{}{}={}",
                        assign.target,
                        Self::binary_op_source_key(op),
                        Self::expression_source_key(right)
                    ));
                }
            }
        }
        if let Statement::Execute(exec_block) = statement {
            if let [ExecuteModifier::As(selector), ExecuteModifier::At(at), rest @ ..] =
                exec_block.modifiers.as_slice()
            {
                if at == "@s" {
                    let rest = rest
                        .iter()
                        .map(Self::execute_modifier_source_key)
                        .collect::<String>();
                    keys.push(Self::normalize_source_key(&format!(
                        "asat{}{}:",
                        selector, rest
                    )));
                }
            }
            // A leading `execute`, as Minecraft writes it, is optional.
            let prefixed: Vec<String> = keys.iter().map(|key| format!("execute{key}")).collect();
            keys.extend(prefixed);
        }
        keys
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
                '#' if !crate::parser::tokenizer::hash_starts_comment(line, index) => {}
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

    /// The key a statement's source text is found by: its tokens, written
    /// the way [`Self::statement_source_key`] writes the parsed statement.
    /// Spellings the parser reads alike key alike: grouping parentheses,
    /// a trailing comma, `**` for `^`, and `//` for `/`.
    pub(crate) fn source_line_key(source: &str) -> String {
        use crate::parser::Token;
        let Ok(tokens) = crate::parser::tokenize(source) else {
            return Self::normalize_source_key(source);
        };
        let tokens: Vec<Token> = tokens
            .into_iter()
            .filter(|token| {
                !matches!(
                    token,
                    Token::Newline | Token::Indent | Token::Dedent | Token::Eof
                )
            })
            .collect();
        let mut key = String::new();
        // Whether each open parenthesis is a call's, which the key keeps.
        let mut calls: Vec<bool> = Vec::new();
        let mut index = 0;
        while index < tokens.len() {
            let token = &tokens[index];
            let next = tokens.get(index + 1);
            index += 1;
            match token {
                Token::LParen => {
                    let call = index >= 2
                        && matches!(
                            tokens[index - 2],
                            Token::Ident(_) | Token::RParen | Token::RBracket
                        );
                    calls.push(call);
                    if call {
                        key.push('(');
                    }
                }
                Token::RParen => {
                    if calls.pop().unwrap_or(true) {
                        key.push(')');
                    }
                }
                Token::Comma
                    if matches!(next, Some(Token::RParen | Token::RBracket | Token::RBrace)) => {}
                Token::Star if next == Some(&Token::Star) => {
                    key.push('^');
                    index += 1;
                }
                Token::Slash if next == Some(&Token::Slash) => {
                    key.push('/');
                    index += 1;
                }
                // `0.50` and `1.0` are the numbers `0.5` and `1`, the way
                // the statement's own key prints them.
                Token::Number(text) => {
                    key.push_str(&Self::number_source_key(text.parse().unwrap_or(0.0)))
                }
                token => key.push_str(&token.to_string()),
            }
        }
        key
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
            Statement::MultiAssign(multi) => {
                let join = |expressions: &[Expression]| {
                    expressions
                        .iter()
                        .map(Self::expression_source_key)
                        .collect::<Vec<_>>()
                        .join(",")
                };
                format!("{}={}", join(&multi.targets), join(&multi.values))
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
                format!("@{}={}", selector_def.name, selector_def.selector)
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
            ExecuteModifier::IfRaw(raw) => format!("if{}", raw),
            ExecuteModifier::UnlessRaw(raw) => format!("unless{}", raw),
            ExecuteModifier::Positioned(pos) => format!("positioned{}", pos),
            ExecuteModifier::Rotated(rot) => format!("rotated{}", rot),
            ExecuteModifier::In(dimension) => format!("in{}", dimension),
            ExecuteModifier::Anchored(anchor) => format!("anchored{}", anchor),
            ExecuteModifier::Align(axes) => format!("align{}", axes),
            ExecuteModifier::Other(modifier) => modifier.clone(),
            ExecuteModifier::Store(store) => format!("store{}", store),
        }
    }

    /// How a number reads in a source key: the same for `1`, `1.0`, and
    /// `1e0`.
    fn number_source_key(n: f64) -> String {
        if n.fract() == 0.0 && n.abs() < 1e18 {
            (n as i64).to_string()
        } else {
            n.to_string()
        }
    }

    /// A statement or expression as compact source text, without spaces.
    pub(in crate::transpiler) fn expression_source_key(expr: &Expression) -> String {
        match expr {
            Expression::Number(n) => Self::number_source_key(*n),
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
            Expression::Slice(base, start, stop) => {
                let bound = |bound: &Option<Box<Expression>>| {
                    bound
                        .as_ref()
                        .map_or(String::new(), |bound| Self::expression_source_key(bound))
                };
                format!(
                    "{}[{}:{}]",
                    Self::expression_source_key(base),
                    bound(start),
                    bound(stop)
                )
            }
            Expression::Conditional(condition, value, otherwise) => format!(
                "{}if{}else{}",
                Self::expression_source_key(value),
                Self::expression_source_key(condition),
                Self::expression_source_key(otherwise)
            ),
            Expression::Subscript(base, index) => {
                format!(
                    "{}[{}]",
                    Self::expression_source_key(base),
                    Self::expression_source_key(index)
                )
            }
        }
    }

    pub(in crate::transpiler) fn binary_op_source_key(op: &BinaryOp) -> &'static str {
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
            BinaryOp::In => "in",
            BinaryOp::NotIn => "notin",
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
