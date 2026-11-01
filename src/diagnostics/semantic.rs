//! AST-anchored semantic checks.
//!
//! The passes here walk the parsed [`Program`] in lockstep with the
//! statement-span sidecar (`StatementSpanTree`) built by
//! `parse_located_with_diagnostics`, so every diagnostic can point at the
//! exact byte of the offending construct.
//!
//! This module is `#[path]`-included into the browser/wasm build through
//! `diagnostics.rs`; it must not touch the filesystem (`std::fs`) or
//! reference `crate::commands`.

use crate::transpiler::scoped::{Mark, ScopedMap};
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;

use crate::ast::{CobbleType, Expression, Import, Program, SourceSpan, Statement};
use crate::parser::{ProgramSpanTable, StatementSpanKind, StatementSpanNode, StatementSpanTree};

use super::text::{
    argument_span, column_from_byte, find_string_end, find_triple_quote, find_word,
    leading_triple_quote, mask_non_code, matching_close_paren, raw_offset_from_masked,
    single_equals_index, split_top_level_arg_spans, starts_with_keyword, ArgumentSpan,
};
use super::{
    compound_assignment_span, datapack_json_resource_type, datapack_json_value_ranges,
    datapack_tag_type, function_calls_in_expression, is_allowed_value_function_call,
    is_builtin_symbol, is_known_module_call, is_list_method, is_simple_module_name,
    math_value_function_arity, nullish_token_spans, should_skip_assignment_symbol_scan,
    split_module_call_name, starts_with_any_keyword, value_only_module_call_message,
    DefinedSymbols, FunctionCallArgumentSpan, FunctionCallSpan, FunctionSignature,
    SourceDiagnostic, DECLARATION_KEYWORDS, RESOURCE_BLOCK_KEYWORDS,
};

/// Shared context for the semantic passes: the source text, the parsed
/// program, and the statement-span sidecar (when the parser could align it
/// with the AST).
pub(super) struct SemanticContext<'a> {
    source: &'a str,
    program: &'a Program,
    spans: Option<&'a StatementSpanTree>,
    /// Top-level import header spans, aligned 1:1 with `program.imports`
    /// (import headers are not statement-tree nodes).
    import_spans: &'a [SourceSpan],
    /// Byte offset of the start of each 1-based source line.
    line_starts: Vec<usize>,
    /// Inclusive 1-based line ranges covered by `"""`/`'''` docstrings.
    /// The parser tokenizes docstring bodies as real statements, but the
    /// text passes treat them as comments; checks that would report errors
    /// in docstring prose skip these ranges.
    docstring_ranges: Vec<(usize, usize)>,
}

impl<'a> SemanticContext<'a> {
    pub(super) fn new(
        source: &'a str,
        program: &'a Program,
        span_table: &'a ProgramSpanTable,
    ) -> Self {
        let spans = span_table.statement_tree.as_ref();
        // A parseable source without an aligned span tree is a span-builder
        // bug (`verify_span_tree_alignment` pins alignment for every
        // parseable fixture); surface it loudly in debug builds instead of
        // silently degrading positions.
        debug_assert!(
            spans.is_some(),
            "parseable source produced no aligned statement span tree"
        );

        let mut line_starts = vec![0];
        line_starts.extend(
            source
                .bytes()
                .enumerate()
                .filter(|(_, byte)| *byte == b'\n')
                .map(|(index, _)| index + 1),
        );

        Self {
            source,
            program,
            spans,
            import_spans: &span_table.imports,
            line_starts,
            docstring_ranges: docstring_line_ranges(source),
        }
    }

    /// Visits every statement in the program (bodies, `elif`/`else` branches,
    /// and `match` cases included) in source order, paired with its span node
    /// when the sidecar is aligned.
    pub(super) fn walk(
        &self,
        visit: &mut impl FnMut(&'a Statement, Option<&'a StatementSpanNode>),
    ) {
        walk_statements_with_spans(
            &self.program.statements,
            self.spans.map(|tree| tree.statements.as_slice()),
            visit,
        );
    }

    /// 1-based (line, char-column) of an absolute byte offset in the source.
    fn position_at(&self, offset: usize) -> (usize, usize) {
        let offset = offset.min(self.source.len());
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        let column = self.source[line_start..offset].chars().count() + 1;
        (line_index + 1, column)
    }

    /// The raw source text a statement header span covers.
    fn statement_slice(&self, span: SourceSpan) -> &'a str {
        self.source.get(span.start..span.end).unwrap_or("")
    }

    /// 1-based line containing an absolute byte offset. Unlike
    /// [`Self::position_at`] this never slices the source, so it is safe on
    /// offsets that are not char boundaries.
    fn line_of_byte(&self, offset: usize) -> usize {
        let offset = offset.min(self.source.len());
        let line_index = match self.line_starts.binary_search(&offset) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        line_index + 1
    }

    /// 1-based line of the last byte a span covers (its start line for empty
    /// spans). Statement header spans stay on one line unless the tokenizer
    /// joined bracket-continued lines into one header.
    fn last_line_of_span(&self, span: SourceSpan) -> usize {
        if span.end > span.start {
            self.line_of_byte(span.end - 1).max(span.line)
        } else {
            span.line
        }
    }

    /// The statement slice with string/comment bytes masked out, line by
    /// line, so call scans see only code. Masking preserves char positions;
    /// [`raw_offset_from_masked`] maps masked offsets back to raw bytes.
    fn masked_statement_slice(&self, span: SourceSpan) -> String {
        let slice = self.statement_slice(span);
        let mut masked = String::with_capacity(slice.len());
        for (index, line) in slice.split('\n').enumerate() {
            if index > 0 {
                masked.push('\n');
            }
            masked.push_str(&mask_non_code(line));
        }
        masked
    }

    /// Position of the function name inside a `def` header: the first
    /// non-whitespace byte after the `def` keyword (computed from the `def`
    /// prefix width so a name that also occurs inside `def ` is not
    /// mislocated).
    fn function_name_position(&self, node: Option<&StatementSpanNode>) -> (usize, usize) {
        self.keyword_argument_position(node, "def")
    }

    /// Position of the `@Name` template inside a `define @Name = ...`
    /// header, pointing at the `@`.
    fn entity_template_name_position(&self, node: Option<&StatementSpanNode>) -> (usize, usize) {
        self.keyword_argument_position(node, "define")
    }

    /// Position of the first non-whitespace byte after a statement's leading
    /// keyword.
    fn keyword_argument_position(
        &self,
        node: Option<&StatementSpanNode>,
        keyword: &str,
    ) -> (usize, usize) {
        let Some(node) = node else {
            return (1, 1);
        };
        let slice = self.statement_slice(node.span);
        let rest = slice.strip_prefix(keyword).unwrap_or(slice);
        let offset = (slice.len() - rest.len()) + (rest.len() - rest.trim_start().len());
        self.position_at(node.span.start + offset)
    }

    /// True when a 1-based source line sits inside (or on the delimiters of)
    /// a triple-quoted docstring.
    fn in_docstring(&self, line: usize) -> bool {
        self.docstring_ranges
            .iter()
            .any(|(start, end)| line >= *start && line <= *end)
    }

    /// Byte offset of the start of a 1-based source line.
    fn line_start(&self, line: usize) -> usize {
        self.line_starts
            .get(line.saturating_sub(1))
            .copied()
            .unwrap_or(0)
    }

    /// The full text of a 1-based source line, without its line terminator.
    fn line_text(&self, line: usize) -> &'a str {
        let index = line.saturating_sub(1);
        let Some(start) = self.line_starts.get(index).copied() else {
            return "";
        };
        let end = self
            .line_starts
            .get(index + 1)
            .copied()
            .unwrap_or(self.source.len());
        self.source[start..end].trim_end_matches(['\n', '\r'])
    }
}

/// Inclusive 1-based line ranges of triple-quoted docstrings, tracked with
/// the same open/close rules as the per-line scans' docstring skips: the
/// opening line, body lines, and the closing line are all part of the range.
fn docstring_line_ranges(source: &str) -> Vec<(usize, usize)> {
    let mut ranges = Vec::new();
    let mut active: Option<(char, usize)> = None;

    for (line_index, line) in source.lines().enumerate() {
        let line_number = line_index + 1;
        if let Some((quote, start_line)) = active {
            if find_triple_quote(line, quote, 0).is_some() {
                ranges.push((start_line, line_number));
                active = None;
            }
            continue;
        }

        let raw_trimmed = line.trim_start();
        if let Some(quote) = leading_triple_quote(raw_trimmed) {
            if find_triple_quote(raw_trimmed, quote, 3).is_none() {
                active = Some((quote, line_number));
            } else {
                ranges.push((line_number, line_number));
            }
        }
    }

    if let Some((_, start_line)) = active {
        ranges.push((start_line, usize::MAX));
    }

    ranges
}

/// Walks `statements` and `nodes` in lockstep, recursing into every body:
/// function/loop/execute bodies and `if` then-branches through `children`,
/// `elif`/`else` bodies through the `If` node's branches, and `match` case
/// bodies through `cases`. Pre-order, which is source order.
fn walk_statements_with_spans<'a>(
    statements: &'a [Statement],
    nodes: Option<&'a [StatementSpanNode]>,
    visit: &mut impl FnMut(&'a Statement, Option<&'a StatementSpanNode>),
) {
    for (index, statement) in statements.iter().enumerate() {
        let node = nodes
            .and_then(|nodes| nodes.get(index))
            .filter(|node| node.kind == StatementSpanKind::of(statement));
        debug_assert!(
            nodes.is_none() || node.is_some(),
            "statement span tree misaligned with program statements"
        );
        visit(statement, node);

        match statement {
            Statement::FunctionDef(function_def) => walk_statements_with_spans(
                &function_def.body,
                node.map(|node| node.children.as_slice()),
                visit,
            ),
            Statement::If(if_statement) => {
                walk_statements_with_spans(
                    &if_statement.then_branch,
                    node.map(|node| node.children.as_slice()),
                    visit,
                );
                for (branch_index, (_, body)) in if_statement.elif_branches.iter().enumerate() {
                    let branch_nodes = node
                        .and_then(|node| node.elif_branches.get(branch_index))
                        .map(|branch| branch.children.as_slice());
                    walk_statements_with_spans(body, branch_nodes, visit);
                }
                if let Some(body) = &if_statement.else_branch {
                    let branch_nodes = node
                        .and_then(|node| node.else_branch.as_ref())
                        .map(|branch| branch.children.as_slice());
                    walk_statements_with_spans(body, branch_nodes, visit);
                }
            }
            Statement::For(for_loop) => walk_statements_with_spans(
                &for_loop.body,
                node.map(|node| node.children.as_slice()),
                visit,
            ),
            Statement::While(while_loop) => walk_statements_with_spans(
                &while_loop.body,
                node.map(|node| node.children.as_slice()),
                visit,
            ),
            Statement::Execute(execute_block) => walk_statements_with_spans(
                &execute_block.body,
                node.map(|node| node.children.as_slice()),
                visit,
            ),
            Statement::Match(match_statement) => {
                for (case_index, case) in match_statement.cases.iter().enumerate() {
                    let case_nodes = node
                        .and_then(|node| node.cases.get(case_index))
                        .map(|branch| branch.children.as_slice());
                    walk_statements_with_spans(&case.body, case_nodes, visit);
                }
            }
            _ => {}
        }
    }
}

/// Every function definition in the program, in source order (nested
/// definitions included; the namespace is flat, mirroring the transpiler),
/// with the definition's name position derived from its statement span.
pub(super) fn collect_function_definitions(
    context: &SemanticContext<'_>,
) -> Vec<FunctionSignature> {
    let mut definitions = Vec::new();
    context.walk(&mut |statement, node| {
        let Statement::FunctionDef(function_def) = statement else {
            return;
        };
        let (line, column) = context.function_name_position(node);
        definitions.push(FunctionSignature {
            name: function_def.name.clone(),
            params: function_def
                .params
                .iter()
                .map(|parameter| parameter.name.clone())
                .collect(),
            line,
            column,
        });
    });
    definitions
}

/// Collects every function definition in the program, reporting a
/// `duplicate-function` diagnostic for each redefinition. A redefinition
/// replaces the stored signature, so later arity checks resolve against the
/// latest definition.
pub(super) fn collect_function_signatures(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) -> HashMap<String, FunctionSignature> {
    let mut signatures: HashMap<String, FunctionSignature> = HashMap::new();

    for signature in collect_function_definitions(context) {
        let (name, line, column) = (signature.name.clone(), signature.line, signature.column);
        if let Some(first) = signatures.insert(name.clone(), signature) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "duplicate-function",
                    line,
                    column,
                    format!("Duplicate function definition `{name}`"),
                )
                .with_help(format!(
                    "First definition is at line {}, column {}. Rename one function or merge the implementations.",
                    first.line, first.column
                )),
            );
        }
    }

    signatures
}

/// A selector alias (`@Name = @selector`) or entity template
/// (`define @Name = ...`) definition. Aliases are located at the statement
/// start, templates at the `@` after `define`.
pub(super) struct NamedSymbolDef {
    pub(super) name: String,
    pub(super) is_entity_template: bool,
    pub(super) line: usize,
    pub(super) column: usize,
}

/// Every selector-alias and entity-template definition in the program, in
/// source order. Names that are not simple identifiers are skipped.
pub(super) fn collect_named_symbol_defs(context: &SemanticContext<'_>) -> Vec<NamedSymbolDef> {
    let mut symbols = Vec::new();
    context.walk(&mut |statement, node| match statement {
        Statement::SelectorDef(selector_def) if is_simple_module_name(&selector_def.name) => {
            let (line, column) = node
                .map(|node| (node.span.line, node.span.column))
                .unwrap_or((1, 1));
            symbols.push(NamedSymbolDef {
                name: selector_def.name.clone(),
                is_entity_template: false,
                line,
                column,
            });
        }
        Statement::EntityDef(entity_def) if is_simple_module_name(&entity_def.name) => {
            let (line, column) = context.entity_template_name_position(node);
            symbols.push(NamedSymbolDef {
                name: entity_def.name.clone(),
                is_entity_template: true,
                line,
                column,
            });
        }
        _ => {}
    });
    symbols
}

/// A module variable: a name the file binds at module level.
pub(super) struct ModuleVariableDef {
    pub(super) name: String,
    pub(super) line: usize,
    pub(super) column: usize,
    /// The value of a `const`. The same `const` in two files is one value.
    pub(super) const_value: Option<Expression>,
}

/// Every name the program binds at module level (`name = ...`,
/// `persist name = ...`, `a, b = ...`, or a module-level `for` target), the
/// first binding of each name, in source order. Names a function binds under
/// `global`, and names brought in with `from x import y`, are not counted:
/// they refer to a variable another file may own.
/// Whether a `for` over `iter` is expanded at compile time: a `range` of
/// number literals, or a list of number, text, and boolean literals.
fn expands_at_compile_time(iter: &Expression) -> bool {
    fn literal_number(expr: &Expression) -> bool {
        match expr {
            Expression::Number(_) => true,
            Expression::Unary(_, inner) => literal_number(inner),
            _ => false,
        }
    }
    match iter {
        Expression::Array(items) => items.iter().all(|item| {
            literal_number(item) || matches!(item, Expression::String(_) | Expression::Boolean(_))
        }),
        Expression::Call(func, args) => {
            matches!(&**func, Expression::Identifier(name) if name == "range")
                && args.iter().all(literal_number)
        }
        _ => false,
    }
}

pub(super) fn collect_module_variable_defs(
    context: &SemanticContext<'_>,
) -> Vec<ModuleVariableDef> {
    let nodes = context.spans.map(|tree| tree.statements.as_slice());
    let mut seen = HashSet::new();
    let mut variables = Vec::new();
    // The variable of a module-level loop exists only when the loop keeps
    // it in a real variable.
    let loop_variables = crate::transpiler::module_loop_variables(&context.program.statements);
    for (index, statement) in context.program.statements.iter().enumerate() {
        let node = nodes.and_then(|nodes| nodes.get(index));
        let names: Vec<&str> = match statement {
            Statement::Assignment(assignment) if assignment.assigns_name() => {
                vec![assignment.target.as_str()]
            }
            Statement::MultiAssign(multi) => multi
                .targets
                .iter()
                .filter_map(|target| match target {
                    Expression::Identifier(name) => Some(name.as_str()),
                    _ => None,
                })
                .collect(),
            // A loop that runs when the pack loads binds its variable in
            // every pass, a real module variable whether or not it's read.
            Statement::For(for_loop) => std::iter::once(for_loop.target.as_str())
                .filter(|target| {
                    loop_variables.contains(*target) || !expands_at_compile_time(&for_loop.iter)
                })
                .chain(for_loop.index.as_deref())
                .collect(),
            // A `const` and a module variable of another file share the
            // name too.
            Statement::ConstAssignment(constant) => vec![constant.target.as_str()],
            _ => Vec::new(),
        };
        let (line, column) = match statement {
            Statement::Assignment(assignment) if assignment.persist => {
                context.keyword_argument_position(node, "persist")
            }
            _ => node
                .map(|node| (node.span.line, node.span.column))
                .unwrap_or((1, 1)),
        };
        for name in names {
            if is_simple_module_name(name) && seen.insert(name.to_string()) {
                variables.push(ModuleVariableDef {
                    name: name.to_string(),
                    line,
                    column,
                    const_value: match statement {
                        Statement::ConstAssignment(constant) => Some(constant.value.clone()),
                        _ => None,
                    },
                });
            }
        }
    }
    variables
}

/// A function call found in the program's expressions, located at the callee
/// name inside its statement's source slice.
pub(super) struct CallSite {
    pub(super) name: String,
    pub(super) arg_count: usize,
    /// The call is an entire expression statement (`text.plain("x")` on a
    /// line of its own) — the shape the value-only helper check reports.
    pub(super) is_statement_expression: bool,
    /// Absolute source byte of the callee name, used to keep diagnostics in
    /// source order.
    sort_offset: usize,
    pub(super) line: usize,
    pub(super) column: usize,
}

/// Collects every function call in the program by walking all statement
/// expressions: assignment targets/values, standalone expressions, `return`
/// expressions, `if`/`elif`/`while` conditions, `for` iterables and steps,
/// `match` values, and entity-definition NBT payloads. Raw commands,
/// selectors, and `execute` modifiers are raw strings in the AST and are
/// never scanned.
pub(super) fn collect_call_sites(context: &SemanticContext<'_>) -> Vec<CallSite> {
    let mut call_sites = Vec::new();
    context.walk(&mut |statement, node| {
        collect_statement_call_sites(context, statement, node, &mut call_sites);
    });
    // The walk is pre-order (source order) statement-by-statement, but an
    // `if` statement's `elif` conditions are visited with their `if` header
    // even though branch bodies sit between them in the source; sorting by
    // source byte restores strict source order.
    call_sites.sort_by_key(|site| site.sort_offset);
    call_sites
}

fn collect_statement_call_sites(
    context: &SemanticContext<'_>,
    statement: &Statement,
    node: Option<&StatementSpanNode>,
    out: &mut Vec<CallSite>,
) {
    let header = node.map(|node| node.span);
    match statement {
        Statement::Assignment(assignment) => {
            let mut expressions = Vec::new();
            if let Some(target) = &assignment.target_expr {
                expressions.push(target);
            }
            expressions.push(&assignment.value);
            collect_header_call_sites(context, &expressions, header, false, out);
        }
        Statement::ConstAssignment(assignment) => {
            collect_header_call_sites(context, &[&assignment.value], header, false, out);
        }
        Statement::Expression(expression) => {
            collect_header_call_sites(context, &[expression], header, true, out);
        }
        Statement::Return(Some(expression)) => {
            collect_header_call_sites(context, &[expression], header, false, out);
        }
        Statement::If(if_statement) => {
            collect_header_call_sites(context, &[&if_statement.condition], header, false, out);
            for (branch_index, (condition, _)) in if_statement.elif_branches.iter().enumerate() {
                let branch_header = node
                    .and_then(|node| node.elif_branches.get(branch_index))
                    .map(|branch| branch.header);
                collect_header_call_sites(context, &[condition], branch_header, false, out);
            }
        }
        Statement::While(while_loop) => {
            collect_header_call_sites(context, &[&while_loop.condition], header, false, out);
        }
        Statement::For(for_loop) => {
            let mut expressions = vec![&for_loop.iter];
            if let Some(step) = &for_loop.step {
                expressions.push(step);
            }
            collect_header_call_sites(context, &expressions, header, false, out);
        }
        Statement::Match(match_statement) => {
            collect_header_call_sites(context, &[&match_statement.value], header, false, out);
        }
        Statement::EntityDef(entity_def) => {
            collect_header_call_sites(context, &[&entity_def.nbt], header, false, out);
        }
        // Function-def headers, imports, raw commands, selector aliases,
        // `create` statements, `global`, `pass`, and bare `return` carry no
        // scannable expressions.
        _ => {}
    }
}

fn collect_header_call_sites(
    context: &SemanticContext<'_>,
    expressions: &[&Expression],
    header: Option<SourceSpan>,
    statement_is_expression: bool,
    out: &mut Vec<CallSite>,
) {
    let mut calls = Vec::new();
    for expression in expressions {
        collect_expression_calls(expression, statement_is_expression, &mut calls);
    }
    if calls.is_empty() {
        return;
    }

    let mut locator = header.map(|span| CallNameLocator::new(context, span));
    for call in calls {
        let located = locator
            .as_mut()
            .and_then(|locator| locator.next_offset(&call.name));
        let (sort_offset, line, column) = match (located, header) {
            (Some(offset), _) => {
                let (line, column) = context.position_at(offset);
                (offset, line, column)
            }
            (None, Some(span)) => (span.start, span.line, span.column),
            (None, None) => (0, 1, 1),
        };
        out.push(CallSite {
            name: call.name,
            arg_count: call.arg_count,
            is_statement_expression: call.is_root,
            sort_offset,
            line,
            column,
        });
    }
}

/// Locates callee names inside one statement's masked source slice. Textual
/// call occurrences are consumed per name, in offset order, so the k-th AST
/// call named `n` maps to the k-th textual `n(...)` in the slice.
struct CallNameLocator<'a> {
    span: SourceSpan,
    slice: &'a str,
    masked: String,
    pending: HashMap<String, VecDeque<usize>>,
}

impl<'a> CallNameLocator<'a> {
    fn new(context: &SemanticContext<'a>, span: SourceSpan) -> Self {
        let slice = context.statement_slice(span);
        let masked = context.masked_statement_slice(span);
        let mut pending: HashMap<String, VecDeque<usize>> = HashMap::new();
        for call in function_calls_in_expression(&masked) {
            pending.entry(call.name).or_default().push_back(call.offset);
        }
        Self {
            span,
            slice,
            masked,
            pending,
        }
    }

    /// Absolute source byte of the next unconsumed occurrence of `name`.
    fn next_offset(&mut self, name: &str) -> Option<usize> {
        let masked_offset = self.pending.get_mut(name)?.pop_front()?;
        Some(self.span.start + raw_offset_from_masked(self.slice, &self.masked, masked_offset))
    }
}

struct AstCall {
    name: String,
    arg_count: usize,
    is_root: bool,
}

fn collect_expression_calls(expression: &Expression, is_root: bool, out: &mut Vec<AstCall>) {
    match expression {
        Expression::Call(callee, arguments) => {
            if let Some(name) = dotted_call_name(callee) {
                out.push(AstCall {
                    name,
                    arg_count: arguments.len(),
                    is_root,
                });
            }
            collect_expression_calls(callee, false, out);
            for argument in arguments {
                collect_expression_calls(argument, false, out);
            }
        }
        Expression::Attribute(base, _) => collect_expression_calls(base, false, out),
        Expression::Binary(left, _, right) => {
            collect_expression_calls(left, false, out);
            collect_expression_calls(right, false, out);
        }
        Expression::Unary(_, operand) => collect_expression_calls(operand, false, out),
        Expression::Subscript(base, index) => {
            collect_expression_calls(base, false, out);
            collect_expression_calls(index, false, out);
        }
        Expression::Slice(base, start, stop) => {
            collect_expression_calls(base, false, out);
            for bound in [start, stop].into_iter().flatten() {
                collect_expression_calls(bound, false, out);
            }
        }
        Expression::Conditional(condition, value, otherwise) => {
            collect_expression_calls(condition, false, out);
            collect_expression_calls(value, false, out);
            collect_expression_calls(otherwise, false, out);
        }
        Expression::Array(items) => {
            for item in items {
                collect_expression_calls(item, false, out);
            }
        }
        Expression::Map(entries) => {
            for (_, value) in entries {
                collect_expression_calls(value, false, out);
            }
        }
        Expression::Number(_)
        | Expression::String(_)
        | Expression::Boolean(_)
        | Expression::None
        | Expression::Identifier(_) => {}
    }
}

/// The dotted callee name of a call, mirroring how the text scanner reads
/// identifier-and-dot runs backwards from an open paren: an identifier chain
/// yields the full dotted name, and a chain hanging off a non-identifier
/// base (`items[0].reset(...)`) yields the trailing attribute names only.
fn dotted_call_name(callee: &Expression) -> Option<String> {
    match callee {
        Expression::Identifier(name) => Some(name.clone()),
        Expression::Attribute(base, attribute) => Some(match dotted_call_name(base) {
            Some(prefix) => format!("{prefix}.{attribute}"),
            None => attribute.clone(),
        }),
        _ => None,
    }
}

/// Visits every statement in a program in source order (bodies, branches,
/// and `match` cases included) without span pairing — for consumers that
/// only need the AST shape, such as the cross-file export collectors.
pub(super) fn for_each_statement<'a>(program: &'a Program, visit: &mut impl FnMut(&'a Statement)) {
    walk_statements_with_spans(&program.statements, None, &mut |statement, _| {
        visit(statement)
    });
}

/// True when the program imports anything other than the `stdlib` module
/// (top-level imports and imports nested inside bodies alike). Cross-file
/// call resolution in `imports.rs` owns call diagnostics for multi-file
/// projects, so the single-file undefined-function pass stands down.
pub(super) fn has_non_stdlib_imports(program: &Program) -> bool {
    if program
        .imports
        .iter()
        .any(|import| import.module != "stdlib")
    {
        return true;
    }

    let mut found = false;
    for_each_statement(program, &mut |statement| {
        if let Statement::Import(import) = statement {
            if import.module != "stdlib" {
                found = true;
            }
        }
    });
    found
}

/// Reports `function-argument-count` for calls to user-defined functions
/// whose argument count does not match the definition. Runs even when other
/// diagnostics were already reported.
pub(super) fn check_user_function_call_arguments(
    function_defs: &HashMap<String, FunctionSignature>,
    call_sites: &[CallSite],
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if function_defs.is_empty() {
        return;
    }

    for call in call_sites {
        let Some(signature) = function_defs.get(&call.name) else {
            continue;
        };
        if call.arg_count == signature.params.len() {
            continue;
        }

        diagnostics.push(
            SourceDiagnostic::error(
                "function-argument-count",
                call.line,
                call.column,
                format!(
                    "Function `{}` expects {} argument(s), but {} provided",
                    signature.name,
                    signature.params.len(),
                    call.arg_count
                ),
            )
            .with_help(format!(
                "Expected parameters: ({})",
                signature.params.join(", ")
            )),
        );
    }
}

/// Reports value-only helpers misused as standalone statements, unknown
/// dotted helper calls, and undefined bare function calls.
pub(super) fn check_undefined_function_calls(
    function_defs: &HashMap<String, FunctionSignature>,
    call_sites: &[CallSite],
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for call in call_sites {
        if let Some((module, method)) = split_module_call_name(&call.name) {
            if call.is_statement_expression {
                if let Some(message) = value_only_module_call_message(module, method, &call.name) {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-function-call-expression",
                            call.line,
                            call.column,
                            message,
                        )
                        .with_help(
                            "Pass the returned value to another helper or JSON resource value instead.",
                        ),
                    );
                    continue;
                }
            }

            if is_known_module_call(module, method) {
                continue;
            }

            // A method of a list held in a variable; the transpiler knows
            // what `module` holds.
            if is_list_method(method) && !is_builtin_symbol(module) {
                continue;
            }

            diagnostics.push(
                SourceDiagnostic::error(
                    "undefined-function",
                    call.line,
                    call.column,
                    format!("Unknown helper function `{}`", call.name),
                )
                .with_help(
                    "Use a documented Cobble helper module/function, define a Cobble function, or use a raw Minecraft command for external behavior.",
                ),
            );
            continue;
        }

        if function_defs.contains_key(&call.name)
            || is_allowed_value_function_call(&call.name)
            || is_list_method(&call.name)
            || matches!(call.name.as_str(), "range" | "enumerate")
        {
            continue;
        }

        diagnostics.push(
            SourceDiagnostic::error(
                "undefined-function",
                call.line,
                call.column,
                format!("Undefined function `{}`", call.name),
            )
            .with_help(
                "Define the Cobble function, import a file that defines it, or use a raw `/function namespace:path` command for external Minecraft functions.",
            ),
        );
    }
}

/// Reports `duplicate-function-parameter` for `def` headers that bind the
/// same parameter name twice, anchored on `FunctionDef` statement spans. A
/// bracket-continued header is one statement slice, so a header that spans
/// several lines is checked as a whole.
///
/// Parameter names and columns come from a top-level comma split of the
/// header's masked parameter list (the statement-span sidecar does not
/// itemize parameter positions). Names that are not simple identifiers are
/// skipped, and each repeat reports against the closest previous occurrence
/// of the name. Masked offsets are mapped back to raw bytes before
/// reporting. Statements inside docstring ranges are skipped.
pub(super) fn check_duplicate_function_parameters(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    context.walk(&mut |statement, node| {
        if !matches!(statement, Statement::FunctionDef(_)) {
            return;
        }
        let Some(node) = node else {
            return;
        };
        if context.in_docstring(node.span.line) {
            return;
        }

        let raw_slice = context.statement_slice(node.span);
        let masked_slice = context.masked_statement_slice(node.span);
        let Some(open) = masked_slice.find('(') else {
            return;
        };
        let Some(close) = masked_slice[open + 1..]
            .find(')')
            .map(|offset| open + 1 + offset)
        else {
            return;
        };

        let params = &masked_slice[open + 1..close];
        let mut seen_params: HashMap<String, (usize, usize)> = HashMap::new();
        for param in split_top_level_arg_spans(params) {
            let name = param.text.trim();
            if !is_simple_module_name(name) {
                continue;
            }

            let (line, column) = context.position_at(
                node.span.start
                    + raw_offset_from_masked(raw_slice, &masked_slice, open + 1 + param.offset),
            );
            if let Some((first_line, first_column)) =
                seen_params.insert(name.to_string(), (line, column))
            {
                diagnostics.push(
                    SourceDiagnostic::error(
                        "duplicate-function-parameter",
                        line,
                        column,
                        format!("Duplicate function parameter `{name}`"),
                    )
                    .with_help(format!(
                        "First `{name}` parameter is at line {first_line}, column {first_column}. Rename one parameter."
                    )),
                );
            }
        }
    });
}

/// Reports misused value helpers in assignment values and `return`
/// expressions: `len()`/math intrinsic arity errors and unknown math
/// functions everywhere, plus known command helpers used as expression
/// values on assignment lines only (a returned module call is left to the
/// undefined-function scan and the transpiler).
///
/// Runs on `Assignment`/`ConstAssignment`/`Return` statements. Calls are
/// found by a masked RHS call scan (`assignment_rhs_span`/
/// `return_expression_span` + `function_calls_in_expression`), with masked
/// offsets mapped back to raw bytes so non-ASCII text keeps char-exact
/// columns. The first invalid call wins, with at most one diagnostic per
/// source line, and statements inside docstring ranges are skipped. A
/// bracket-continued statement is scanned as one unit, so calls on
/// continuation lines are found.
pub(super) fn check_assignment_value_function_calls(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut flagged_lines: HashSet<usize> = HashSet::new();
    context.walk(&mut |statement, node| {
        let is_return_expression = match statement {
            Statement::Assignment(_) | Statement::ConstAssignment(_) => false,
            Statement::Return(Some(_)) => true,
            _ => return,
        };
        let Some(node) = node else {
            return;
        };
        if context.in_docstring(node.span.line) || flagged_lines.contains(&node.span.line) {
            return;
        }

        let raw_slice = context.statement_slice(node.span);
        let masked_slice = context.masked_statement_slice(node.span);
        let rhs_span = if is_return_expression {
            return_expression_span(&masked_slice)
        } else {
            assignment_rhs_span(&masked_slice)
        };
        let Some(rhs_span) = rhs_span else {
            return;
        };

        for call in function_calls_in_expression(&rhs_span.text) {
            let position_of_call = |context: &SemanticContext<'_>| {
                context.position_at(
                    node.span.start
                        + raw_offset_from_masked(
                            raw_slice,
                            &masked_slice,
                            rhs_span.offset + call.offset,
                        ),
                )
            };

            if let Some(diagnostic) = invalid_value_function_call(&call) {
                let (line, column) = position_of_call(context);
                diagnostics.push(
                    SourceDiagnostic::error(diagnostic.kind, line, column, diagnostic.message)
                        .with_help(diagnostic.help),
                );
                flagged_lines.insert(node.span.line);
                return;
            }

            if is_allowed_value_function_call(&call.name) {
                continue;
            }

            if is_return_expression {
                // Module calls in `return <expression>` are validated by the
                // undefined-function scan and the transpiler; only
                // value-helper misuse (len/math arity, unknown math) is
                // reported here.
                continue;
            }

            if let Some((module, method)) = split_module_call_name(&call.name) {
                if is_known_module_call(module, method) {
                    let (line, column) = position_of_call(context);
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-function-call-expression",
                            line,
                            column,
                            format!(
                                "`{}` is a command helper and cannot be used as an expression value",
                                call.name
                            ),
                        )
                        .with_help(
                            "Use command helpers as standalone statements, or call a user function that returns an integer score value.",
                        ),
                    );
                    flagged_lines.insert(node.span.line);
                    return;
                }
            }
        }
    });
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AssignmentRhsSpan {
    text: String,
    offset: usize,
}

/// Treats `return <expression>` like an assignment RHS so expression-position
/// checks can report at the `return` site. Offsets stay relative to `trimmed`.
fn return_expression_span(trimmed: &str) -> Option<AssignmentRhsSpan> {
    if !starts_with_keyword(trimmed, "return") {
        return None;
    }

    let rest = &trimmed["return".len()..];
    let offset = "return".len() + (rest.len() - rest.trim_start().len());
    let text = rest.trim();
    if text.is_empty() {
        return None;
    }

    Some(AssignmentRhsSpan {
        text: text.to_string(),
        offset,
    })
}

fn assignment_rhs_span(trimmed: &str) -> Option<AssignmentRhsSpan> {
    if let Some(compound) = compound_assignment_span(trimmed) {
        return Some(AssignmentRhsSpan {
            text: compound.value,
            offset: compound.value_offset,
        });
    }

    let equals_index = single_equals_index(trimmed)?;
    let rhs = &trimmed[equals_index + 1..];
    let trim_start = rhs.len() - rhs.trim_start().len();
    Some(AssignmentRhsSpan {
        text: rhs.trim().to_string(),
        offset: equals_index + 1 + trim_start,
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ValueFunctionDiagnostic {
    kind: &'static str,
    message: String,
    help: &'static str,
}

fn invalid_value_function_call(call: &FunctionCallSpan) -> Option<ValueFunctionDiagnostic> {
    if call.name == "len" {
        if call.arg_count != 1 {
            return Some(ValueFunctionDiagnostic {
                kind: "function-argument-count",
                message: format!("len() takes 1 argument, but {} provided", call.arg_count),
                help: "Use len(value) with a string, list, map, or storage-backed value.",
            });
        }
        return None;
    }

    invalid_math_value_function_call(call)
}

fn invalid_math_value_function_call(call: &FunctionCallSpan) -> Option<ValueFunctionDiagnostic> {
    if !call.name.starts_with("math.") {
        return None;
    }

    let Some(arity) = math_value_function_arity(&call.name) else {
        return Some(ValueFunctionDiagnostic {
            kind: "undefined-function",
            message: format!("Unknown math function `{}`", call.name),
            help: "Use one of the supported math intrinsics: math.sqrt, math.abs, math.min, or math.max.",
        });
    };

    if call.arg_count != arity {
        return Some(ValueFunctionDiagnostic {
            kind: "function-argument-count",
            message: format!(
                "{}() takes {} {}, but {} provided",
                call.name,
                arity,
                argument_word(arity),
                call.arg_count
            ),
            help: "Use math.sqrt(value), math.abs(value), math.min(left, right), or math.max(left, right).",
        });
    }

    None
}

fn argument_word(count: usize) -> &'static str {
    if count == 1 {
        "argument"
    } else {
        "arguments"
    }
}

/// Reports `no-op-expression` for standalone expression statements whose
/// root is not a function call: the transpiler compiles root calls and
/// silently drops every other expression statement, so these lines generate
/// no Minecraft commands.
///
/// Skipped: selector-ish statements (an `@...` identifier or a statement
/// whose text starts with `@`), string-led statements, and statements inside
/// docstring ranges (docstring prose parses into bare identifier statements
/// that the transpiler ignores). Expressions that only contain a call or
/// parentheses (`score + helper(1)`, `(score)`) are flagged.
pub(super) fn check_noop_expression_statements(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // At most one report per source line: the parser can split one line
    // into several statements (`nonlocal score` becomes two identifier
    // statements).
    let mut flagged_lines: std::collections::HashSet<usize> = std::collections::HashSet::new();

    context.walk(&mut |statement, node| {
        let Statement::Expression(expression) = statement else {
            return;
        };
        if matches!(expression, Expression::Call(..)) {
            return;
        }
        if let Expression::Identifier(name) = expression {
            if name.starts_with('@') {
                return;
            }
        }

        let (line, column) = match node {
            Some(node) => {
                if context.in_docstring(node.span.line) || flagged_lines.contains(&node.span.line)
                {
                    return;
                }
                let slice = context.statement_slice(node.span);
                if slice.starts_with('@') || slice.starts_with('"') || slice.starts_with('\'') {
                    return;
                }
                // Skip block-header and assignment lines, which the parser
                // can split into several statements: `execute as @a if
                // ready():` yields a leading identifier statement before the
                // `Execute` block, and `value = await score` yields a
                // trailing identifier statement after the assignment.
                let masked_line = mask_non_code(context.line_text(node.span.line));
                let masked_trimmed = masked_line.trim_start();
                if masked_trimmed.ends_with(':')
                    || single_equals_index(masked_trimmed).is_some()
                    || compound_assignment_span(masked_trimmed).is_some()
                {
                    return;
                }
                flagged_lines.insert(node.span.line);
                (node.span.line, node.span.column)
            }
            None => (1, 1),
        };

        diagnostics.push(
            SourceDiagnostic::error(
                "no-op-expression",
                line,
                column,
                "Standalone expression does not generate Minecraft commands",
            )
            .with_help(
                "Assign the value to a variable, call a function/helper, use a raw command, or write `pass` for an intentional no-op.",
            ),
        );
    });
}

/// Reports `unsupported-none` for `None`/`null` tokens anywhere in code,
/// except `None` used as a JSON null inside the value argument of a
/// datapack JSON resource helper call.
///
/// Detection is anchored on the statement-span sidecar: every statement
/// header slice (plus `elif` and `case` branch headers and top-level import
/// headers) gets a masked token scan, so `None`/`null` inside raw selector
/// strings, binding names, and entity payloads are reported too. Those
/// occurrences never surface as `Expression::None`/`Identifier` nodes, so a
/// pure expression walk would miss them. Raw `/` command statements and
/// docstring ranges are skipped.
pub(super) fn check_unsupported_none_usage(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    if context.spans.is_none() {
        // Span-builder failure on parseable source (reported by the
        // debug_assert in `SemanticContext::new`): fall back to a pure
        // expression walk so detection survives, at degraded positions.
        check_unsupported_none_usage_without_spans(context, diagnostics);
        return;
    }

    let mut regions: Vec<SourceSpan> = context.import_spans.to_vec();
    context.walk(&mut |statement, node| {
        if matches!(statement, Statement::MinecraftCommand(_)) {
            return;
        }
        let Some(node) = node else {
            return;
        };
        if !context.in_docstring(node.span.line) {
            regions.push(node.span);
        }
        for branch in node
            .elif_branches
            .iter()
            .chain(node.cases.iter())
            .chain(node.else_branch.iter())
        {
            if !context.in_docstring(branch.header.line) {
                regions.push(branch.header);
            }
        }
    });
    regions.sort_by_key(|span| span.start);

    for span in regions {
        let slice = context.statement_slice(span);
        let masked = context.masked_statement_slice(span);
        let allowed_ranges = datapack_json_value_ranges(&masked);
        for token in nullish_token_spans(&masked) {
            if token.name == "None"
                && allowed_ranges
                    .iter()
                    .any(|range| token.offset >= range.start && token.offset < range.end)
            {
                continue;
            }

            let offset = span.start + raw_offset_from_masked(slice, &masked, token.offset);
            let (line, column) = context.position_at(offset);
            diagnostics.push(unsupported_none_diagnostic(line, column));
        }
    }
}

/// Span-less fallback for [`check_unsupported_none_usage`]: walks statement
/// expressions for `Expression::None` and `Identifier("null")`, allowing
/// `None` inside the value argument of datapack JSON resource helper calls.
/// Positions degrade to 1:1 because no spans are available.
fn check_unsupported_none_usage_without_spans(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    context.walk(&mut |statement, _| {
        let mut expressions: Vec<&Expression> = Vec::new();
        match statement {
            Statement::Assignment(assignment) => {
                if let Some(target) = &assignment.target_expr {
                    expressions.push(target);
                }
                expressions.push(&assignment.value);
            }
            Statement::ConstAssignment(assignment) => expressions.push(&assignment.value),
            Statement::Expression(expression) => expressions.push(expression),
            Statement::Return(Some(expression)) => expressions.push(expression),
            Statement::If(if_statement) => {
                expressions.push(&if_statement.condition);
                expressions.extend(if_statement.elif_branches.iter().map(|(cond, _)| cond));
            }
            Statement::While(while_loop) => expressions.push(&while_loop.condition),
            Statement::For(for_loop) => {
                expressions.push(&for_loop.iter);
                expressions.extend(for_loop.step.iter());
            }
            Statement::Match(match_statement) => expressions.push(&match_statement.value),
            Statement::EntityDef(entity_def) => expressions.push(&entity_def.nbt),
            _ => {}
        }
        for expression in expressions {
            collect_expression_none_diagnostics(expression, false, diagnostics);
        }
    });
}

fn collect_expression_none_diagnostics(
    expression: &Expression,
    none_allowed: bool,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    match expression {
        Expression::None => {
            if !none_allowed {
                diagnostics.push(unsupported_none_diagnostic(1, 1));
            }
        }
        Expression::Identifier(name) => {
            if name == "null" {
                diagnostics.push(unsupported_none_diagnostic(1, 1));
            }
        }
        Expression::Call(callee, arguments) => {
            let is_datapack_json_call = dotted_call_name(callee)
                .and_then(|name| {
                    name.strip_prefix("datapack.")
                        .map(|helper| datapack_json_resource_type(helper).is_some())
                })
                .unwrap_or(false);
            collect_expression_none_diagnostics(callee, false, diagnostics);
            for (index, argument) in arguments.iter().enumerate() {
                let allowed = none_allowed || (is_datapack_json_call && index == 1);
                collect_expression_none_diagnostics(argument, allowed, diagnostics);
            }
        }
        Expression::Attribute(base, _) => {
            collect_expression_none_diagnostics(base, none_allowed, diagnostics)
        }
        Expression::Binary(left, _, right) => {
            collect_expression_none_diagnostics(left, none_allowed, diagnostics);
            collect_expression_none_diagnostics(right, none_allowed, diagnostics);
        }
        Expression::Unary(_, operand) => {
            collect_expression_none_diagnostics(operand, none_allowed, diagnostics)
        }
        Expression::Subscript(base, index) => {
            collect_expression_none_diagnostics(base, none_allowed, diagnostics);
            collect_expression_none_diagnostics(index, none_allowed, diagnostics);
        }
        Expression::Slice(base, start, stop) => {
            collect_expression_none_diagnostics(base, none_allowed, diagnostics);
            for bound in [start, stop].into_iter().flatten() {
                collect_expression_none_diagnostics(bound, none_allowed, diagnostics);
            }
        }
        Expression::Conditional(condition, value, otherwise) => {
            collect_expression_none_diagnostics(condition, none_allowed, diagnostics);
            collect_expression_none_diagnostics(value, none_allowed, diagnostics);
            collect_expression_none_diagnostics(otherwise, none_allowed, diagnostics);
        }
        Expression::Array(items) => {
            for item in items {
                collect_expression_none_diagnostics(item, none_allowed, diagnostics);
            }
        }
        Expression::Map(entries) => {
            for (_, value) in entries {
                collect_expression_none_diagnostics(value, none_allowed, diagnostics);
            }
        }
        Expression::Number(_) | Expression::String(_) | Expression::Boolean(_) => {}
    }
}

fn unsupported_none_diagnostic(line: usize, column: usize) -> SourceDiagnostic {
    SourceDiagnostic::error(
        "unsupported-none",
        line,
        column,
        "None/null is only supported in data pack JSON resource helper values",
    )
    .with_help(
        "Minecraft SNBT/NBT storage has no null type. Use `None` only as a JSON null in datapack resource helpers, use `None` instead of lowercase `null`, or choose an explicit sentinel value.",
    )
}

// ===========================================================================
// Undefined variable references (single-file, strictly last in the ladder)
// ===========================================================================

/// The whole-file forward-referencing symbol table the undefined-variable
/// check resolves against: function names, import symbols (`import X` module
/// names and `from X import` items), simple assignment targets (compound
/// assignments desugar to plain assignments in the AST, so their targets are
/// included), `global` names, and `for` targets — from any nesting depth,
/// regardless of position (unlike the placeholder pass's incremental scope).
/// Statements inside docstring ranges do not define symbols.
pub(super) fn collect_defined_symbols(
    context: &SemanticContext<'_>,
    function_defs: &HashMap<String, FunctionSignature>,
) -> DefinedSymbols {
    let mut symbols = DefinedSymbols::default();

    for signature in function_defs.values() {
        symbols.insert(signature.name.clone());
    }

    for import in &context.program.imports {
        add_import_symbols(import, &mut symbols);
    }

    context.walk(&mut |statement, node| {
        if node.is_some_and(|node| context.in_docstring(node.span.line)) {
            return;
        }
        match statement {
            Statement::Assignment(assignment) if is_simple_module_name(&assignment.target) => {
                symbols.insert(assignment.target.clone());
            }
            Statement::ConstAssignment(assignment) if is_simple_module_name(&assignment.target) => {
                symbols.insert(assignment.target.clone());
            }
            Statement::Global(names) => {
                for name in names {
                    if is_simple_module_name(name) {
                        symbols.insert(name.clone());
                    }
                }
            }
            Statement::MultiAssign(multi) => {
                for target in &multi.targets {
                    if let Expression::Identifier(name) = target {
                        symbols.insert(name.clone());
                    }
                }
            }
            Statement::For(for_loop) if is_simple_module_name(&for_loop.target) => {
                symbols.insert(for_loop.target.clone());
                if let Some(index) = &for_loop.index {
                    symbols.insert(index.clone());
                }
            }
            Statement::Import(import) => add_import_symbols(import, &mut symbols),
            _ => {}
        }
    });

    symbols
}

/// `import X` defines the module name; `from X import a, b` defines only the
/// items.
fn add_import_symbols(import: &Import, symbols: &mut DefinedSymbols) {
    if import.items.is_empty() {
        if is_simple_module_name(&import.module) {
            symbols.insert(import.module.clone());
        }
        return;
    }
    for item in &import.items {
        if is_simple_module_name(item) {
            symbols.insert(item.clone());
        }
    }
}

/// One source line to scan for undefined references, with the parameter set
/// of the innermost enclosing function definition (the only local scope;
/// `for` targets resolve through the whole-file symbol table).
struct ReferenceLineScan {
    line: usize,
    params: Rc<HashSet<String>>,
    /// The Python conditions of an execute header on this line, as in
    /// `as @a if x > 3:`. The header's other words are Minecraft syntax,
    /// such as the `=` of `if score a temp = b temp:`.
    conditions: Option<Vec<String>>,
}

/// Reports `undefined-variable` for identifier references that resolve
/// neither in the whole-file symbol table nor in the enclosing function's
/// parameters. The statement walk decides which lines are scanned (raw
/// commands, docstring ranges, and `def` header lines are skipped); text
/// extraction and skip-list predicates then analyze each physical line on
/// its own, including each line of a multi-line statement.
pub(super) fn check_undefined_variable_references(
    context: &SemanticContext<'_>,
    function_defs: &HashMap<String, FunctionSignature>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let symbols = collect_defined_symbols(context, function_defs);

    let mut scans = Vec::new();
    let module_params = Rc::new(HashSet::new());
    collect_reference_line_scans(
        context,
        &context.program.statements,
        context.spans.map(|tree| tree.statements.as_slice()),
        &module_params,
        &mut scans,
    );
    // The walk visits `elif`/`case` headers with their owning statement, so
    // sort to scan (and report) in strict line order.
    // An execute header keeps its conditions when a statement the parser
    // split from its line scans the line too.
    scans.sort_by_key(|scan| scan.line);
    scans.dedup_by(|later, kept| {
        let same = later.line == kept.line;
        if same && kept.conditions.is_none() {
            kept.conditions = later.conditions.take();
        }
        same
    });

    let mut reported = HashSet::new();
    for scan in scans {
        let line_text = context.line_text(scan.line);
        let masked = mask_non_code(line_text);
        let trimmed = masked.trim_start();
        if trimmed.is_empty() || trimmed.starts_with('/') {
            continue;
        }
        let indent = masked.len() - trimmed.len();

        let mut expressions = match &scan.conditions {
            None => expression_spans_for_undefined_scan(trimmed),
            Some(conditions) => execute_condition_spans(trimmed, conditions),
        };
        // The keys of a target such as `nums[i] = 5` are read too.
        if scan.conditions.is_none() && compound_assignment_span(trimmed).is_none() {
            if let Some(equals_index) = single_equals_index(trimmed) {
                expressions.extend(subscript_key_spans(&ExpressionSpan {
                    text: trimmed[..equals_index].to_string(),
                    offset: 0,
                }));
            }
        }
        let mut pending = expressions;
        let mut expressions = Vec::new();
        while let Some(expression) = pending.pop() {
            if expression_has_unsupported_reference_shape(&expression.text) {
                // The keys of `nums[nokey]` are plain expressions, whatever
                // the rest of the line holds.
                pending.extend(subscript_key_spans(&expression));
                continue;
            }
            expressions.push(expression);
        }
        expressions.sort_by_key(|expression| expression.offset);
        for expression in expressions {
            for reference in identifier_references_in_expression(&expression.text) {
                if symbols.contains(&reference.name) || scan.params.contains(&reference.name) {
                    continue;
                }

                if !reported.insert((scan.line, reference.name.clone())) {
                    continue;
                }

                diagnostics.push(
                    SourceDiagnostic::error(
                        "undefined-variable",
                        scan.line,
                        column_from_byte(&masked, indent + expression.offset + reference.offset),
                        format!("Undefined variable `{}`", reference.name),
                    )
                    .with_help(format!(
                        "Define or import `{}`, or pass it as a function parameter.",
                        reference.name
                    )),
                );
            }
        }
    }
}

fn collect_reference_line_scans(
    context: &SemanticContext<'_>,
    statements: &[Statement],
    nodes: Option<&[StatementSpanNode]>,
    params: &Rc<HashSet<String>>,
    scans: &mut Vec<ReferenceLineScan>,
) {
    for (index, statement) in statements.iter().enumerate() {
        let node = nodes
            .and_then(|nodes| nodes.get(index))
            .filter(|node| node.kind == StatementSpanKind::of(statement));

        match statement {
            // `def` lines are not scanned; the parameters become the body's
            // local scope. Only the innermost definition's parameters are
            // active (nested definitions do not inherit outer parameters).
            Statement::FunctionDef(function_def) => {
                let function_params: Rc<HashSet<String>> = Rc::new(
                    function_def
                        .params
                        .iter()
                        .map(|parameter| parameter.name.clone())
                        .collect(),
                );
                collect_reference_line_scans(
                    context,
                    &function_def.body,
                    node.map(|node| node.children.as_slice()),
                    &function_params,
                    scans,
                );
            }
            // Raw command lines are not reference-scanned.
            Statement::MinecraftCommand(_) => {}
            Statement::If(if_statement) => {
                if let Some(node) = node {
                    push_reference_scan_lines(context, node.span, params, scans);
                    for branch in &node.elif_branches {
                        push_reference_scan_lines(context, branch.header, params, scans);
                    }
                    if let Some(branch) = &node.else_branch {
                        push_reference_scan_lines(context, branch.header, params, scans);
                    }
                }
                collect_reference_line_scans(
                    context,
                    &if_statement.then_branch,
                    node.map(|node| node.children.as_slice()),
                    params,
                    scans,
                );
                for (branch_index, (_, body)) in if_statement.elif_branches.iter().enumerate() {
                    let branch_nodes = node
                        .and_then(|node| node.elif_branches.get(branch_index))
                        .map(|branch| branch.children.as_slice());
                    collect_reference_line_scans(context, body, branch_nodes, params, scans);
                }
                if let Some(body) = &if_statement.else_branch {
                    let branch_nodes = node
                        .and_then(|node| node.else_branch.as_ref())
                        .map(|branch| branch.children.as_slice());
                    collect_reference_line_scans(context, body, branch_nodes, params, scans);
                }
            }
            Statement::Match(match_statement) => {
                if let Some(node) = node {
                    push_reference_scan_lines(context, node.span, params, scans);
                    for case in &node.cases {
                        push_reference_scan_lines(context, case.header, params, scans);
                    }
                }
                for (case_index, case) in match_statement.cases.iter().enumerate() {
                    let case_nodes = node
                        .and_then(|node| node.cases.get(case_index))
                        .map(|branch| branch.children.as_slice());
                    collect_reference_line_scans(context, &case.body, case_nodes, params, scans);
                }
            }
            Statement::For(for_loop) => {
                if let Some(node) = node {
                    push_reference_scan_lines(context, node.span, params, scans);
                }
                collect_reference_line_scans(
                    context,
                    &for_loop.body,
                    node.map(|node| node.children.as_slice()),
                    params,
                    scans,
                );
            }
            Statement::While(while_loop) => {
                if let Some(node) = node {
                    push_reference_scan_lines(context, node.span, params, scans);
                }
                collect_reference_line_scans(
                    context,
                    &while_loop.body,
                    node.map(|node| node.children.as_slice()),
                    params,
                    scans,
                );
            }
            Statement::Execute(execute_block) => {
                if let Some(node) = node {
                    if !context.in_docstring(node.span.line) {
                        scans.push(ReferenceLineScan {
                            line: node.span.line,
                            params: Rc::clone(params),
                            conditions: Some(execute_python_conditions(execute_block)),
                        });
                    }
                }
                collect_reference_line_scans(
                    context,
                    &execute_block.body,
                    node.map(|node| node.children.as_slice()),
                    params,
                    scans,
                );
            }
            // Every remaining statement kind scans its header lines with the
            // shared text extraction, whose skip lists keep declarations,
            // resource blocks, `global`, and selector lines silent.
            _ => {
                if let Some(node) = node {
                    push_reference_scan_lines(context, node.span, params, scans);
                }
            }
        }
    }
}

/// The `if` and `unless` conditions of an execute header that are Python
/// conditions, as `x > 3` in `as @a if x > 3:`, each with its Minecraft
/// conditions, as in `entity @a and x > 3`, read as `True`.
fn execute_python_conditions(block: &crate::ast::ExecuteBlock) -> Vec<String> {
    use crate::ast::ExecuteModifier;
    let modifiers = &block.modifiers;
    modifiers
        .iter()
        .enumerate()
        .filter_map(|(index, modifier)| match modifier {
            ExecuteModifier::IfRaw(condition) | ExecuteModifier::UnlessRaw(condition) => {
                // `entity @a and x in items` reaches here split at `in`.
                match modifiers.get(index + 1) {
                    Some(ExecuteModifier::In(container)) => {
                        crate::transpiler::merged_execute_in_condition(condition, container)
                    }
                    _ => None,
                }
                .or_else(|| Some(condition.trim().to_string()))
            }
            _ => None,
        })
        .filter(|condition| {
            !condition.is_empty()
                && (crate::transpiler::mask_minecraft_conditions(condition).is_some()
                    || crate::transpiler::raw_minecraft_condition_verdict(condition) != Some(true))
        })
        .collect()
}

/// Where each of `conditions` stands in the execute header `trimmed`, in
/// order, with its Minecraft conditions padded to `True`. A condition the
/// masked line doesn't show as it is, as one holding text, is skipped.
fn execute_condition_spans(trimmed: &str, conditions: &[String]) -> Vec<ExpressionSpan> {
    let mut spans = Vec::new();
    let mut from = 0;
    for condition in conditions {
        // The line is masked, so a condition holding text is found masked.
        let masked = mask_non_code(condition);
        let Some(found) = trimmed[from..].find(masked.as_str()) else {
            continue;
        };
        let offset = from + found;
        from = offset + masked.len();
        let mut text = masked;
        if let Some((_, minecraft)) = crate::transpiler::mask_minecraft_conditions(condition) {
            let mut at = 0;
            for (_, part) in minecraft {
                let part = mask_non_code(&part);
                if let Some(found) = text[at..].find(&part) {
                    let start = at + found;
                    let filler = format!("{:<width$}", "True", width = part.len());
                    text.replace_range(start..start + part.len(), &filler);
                    at = start + part.len();
                }
            }
        }
        spans.push(ExpressionSpan { text, offset });
    }
    spans
}

fn push_reference_scan_lines(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    params: &Rc<HashSet<String>>,
    scans: &mut Vec<ReferenceLineScan>,
) {
    if context.in_docstring(span.line) {
        return;
    }
    for line in span.line..=context.last_line_of_span(span) {
        scans.push(ReferenceLineScan {
            line,
            params: Rc::clone(params),
            conditions: None,
        });
    }
}

// --- Text extraction for the undefined-variable check. These per-line
// expression-carving and identifier-reference rules define what counts as a
// scannable expression and a reference (the statement walk above only
// decides WHICH lines to scan).

#[derive(Debug, Clone, PartialEq, Eq)]
struct ExpressionSpan {
    text: String,
    offset: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct IdentifierReference {
    name: String,
    offset: usize,
}

fn expression_spans_for_undefined_scan(trimmed: &str) -> Vec<ExpressionSpan> {
    if should_skip_expression_reference_scan(trimmed) {
        return Vec::new();
    }

    let mut expressions = Vec::new();
    if let Some(compound) = compound_assignment_span(trimmed) {
        expressions.push(ExpressionSpan {
            text: compound.target,
            offset: compound.target_offset,
        });
        if !compound.value.is_empty() {
            expressions.push(ExpressionSpan {
                text: compound.value,
                offset: compound.value_offset,
            });
        }
        return expressions;
    }

    if let Some(equals_index) = single_equals_index(trimmed) {
        let rhs = &trimmed[equals_index + 1..];
        let trim_start = rhs.len() - rhs.trim_start().len();
        let expression = rhs.trim();
        if !expression.is_empty() {
            expressions.push(ExpressionSpan {
                text: expression.to_string(),
                offset: equals_index + 1 + trim_start,
            });
        }
        return expressions;
    }

    if starts_with_keyword(trimmed, "return") {
        let rest_offset = "return".len();
        let rest = trimmed[rest_offset..].trim_start();
        let trim_start = trimmed[rest_offset..].len() - rest.len();
        if !rest.is_empty() {
            expressions.push(ExpressionSpan {
                text: rest.to_string(),
                offset: rest_offset + trim_start,
            });
        }
        return expressions;
    }

    for keyword in ["if", "elif", "while", "match"] {
        if starts_with_keyword(trimmed, keyword) {
            let rest_offset = keyword.len();
            let rest = trimmed[rest_offset..].trim_start();
            let trim_start = trimmed[rest_offset..].len() - rest.len();
            let expression = rest.trim_end_matches(':').trim_end();
            // `if entity @a and x > 3:` checks only its Python operands: each
            // Minecraft condition reads as `True`, padded to keep columns.
            if keyword == "if" {
                if let Some((_, conditions)) =
                    crate::transpiler::mask_minecraft_conditions(expression)
                {
                    let mut text = expression.to_string();
                    let mut from = 0;
                    for (_, condition) in conditions {
                        if let Some(found) = text[from..].find(&condition) {
                            let at = from + found;
                            let filler = format!("{:<width$}", "True", width = condition.len());
                            text.replace_range(at..at + condition.len(), &filler);
                            from = at + condition.len();
                        }
                    }
                    expressions.push(ExpressionSpan {
                        text,
                        offset: rest_offset + trim_start,
                    });
                    return expressions;
                }
            }
            // `if block ~ ~ ~ minecraft:stone:` is an execute block with a
            // raw condition; its words are Minecraft syntax, not variables.
            if keyword == "if"
                && crate::transpiler::raw_minecraft_condition_verdict(expression) == Some(true)
            {
                return expressions;
            }
            if !expression.is_empty() {
                expressions.push(ExpressionSpan {
                    text: expression.to_string(),
                    offset: rest_offset + trim_start,
                });
            }
            return expressions;
        }
    }

    if starts_with_keyword(trimmed, "for") {
        let rest_offset = "for".len();
        let rest = trimmed[rest_offset..].trim_start();
        let rest_trim_start = trimmed[rest_offset..].len() - rest.len();
        let Some((target, iter_and_step)) = rest.split_once(" in ") else {
            return expressions;
        };
        let iter_offset = rest_offset + rest_trim_start + target.len() + " in ".len();
        let iter_and_step = iter_and_step.trim_end_matches(':').trim_end();
        if let Some((iter, step)) = iter_and_step.split_once(" by ") {
            let iter = iter.trim();
            if !iter.is_empty() {
                expressions.push(ExpressionSpan {
                    text: iter.to_string(),
                    offset: iter_offset,
                });
            }
            let step_trim_start = step.len() - step.trim_start().len();
            let step = step.trim();
            if !step.is_empty() {
                expressions.push(ExpressionSpan {
                    text: step.to_string(),
                    offset: iter_offset + iter.len() + " by ".len() + step_trim_start,
                });
            }
        } else if !iter_and_step.is_empty() {
            expressions.push(ExpressionSpan {
                text: iter_and_step.to_string(),
                offset: iter_offset,
            });
        }
    }

    if expressions.is_empty() {
        for call in function_calls_in_expression(trimmed) {
            for argument in call.arguments {
                if !should_scan_call_argument_references(&argument.text)
                    || !function_calls_in_expression(&argument.text).is_empty()
                {
                    continue;
                }

                expressions.push(ExpressionSpan {
                    text: argument.text,
                    offset: argument.offset,
                });
            }
        }
    }

    expressions
}

fn should_scan_call_argument_references(argument: &str) -> bool {
    let trimmed = argument.trim();
    if trimmed.is_empty() {
        return false;
    }

    !matches!(trimmed.chars().next(), Some('[' | '{'))
}

fn should_skip_expression_reference_scan(trimmed: &str) -> bool {
    // Declarations and resource blocks plus `global` statements and selector
    // (`@`) lines. Control-flow headers and `return` lines are NOT skipped so
    // the expressions they carry still get reference checks.
    starts_with_any_keyword(trimmed, DECLARATION_KEYWORDS)
        || starts_with_any_keyword(trimmed, RESOURCE_BLOCK_KEYWORDS)
        || starts_with_keyword(trimmed, "global")
        || trimmed.starts_with('@')
}

fn identifier_references_in_expression(expression: &str) -> Vec<IdentifierReference> {
    let bytes = expression.as_bytes();
    let mut refs = Vec::new();
    let mut index = 0;

    while index < bytes.len() {
        // A number, including the `e3` of an exponent (`1e3`, `2.5e-3`).
        if bytes[index].is_ascii_digit() {
            while index < bytes.len() && is_ident_continue_byte(bytes[index]) {
                index += 1;
            }
            continue;
        }
        if !is_ident_start_byte(bytes[index]) {
            index += 1;
            continue;
        }

        let start = index;
        index += 1;
        while index < bytes.len() && is_ident_continue_byte(bytes[index]) {
            index += 1;
        }

        let name = &expression[start..index];
        if should_skip_identifier_reference(expression, start, index, name) {
            continue;
        }

        refs.push(IdentifierReference {
            name: name.to_string(),
            offset: start,
        });
    }

    refs
}

/// The keys of the subscripts in `expression`, as `i + 1` of `nums[i + 1]`.
/// A list literal such as `[a, b]` and a slice are left out.
fn subscript_key_spans(expression: &ExpressionSpan) -> Vec<ExpressionSpan> {
    let text = &expression.text;
    let bytes = text.as_bytes();
    let mut spans = Vec::new();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }
        let Some(close) = matching_close_bracket(text, index) else {
            break;
        };
        let subscript = match previous_non_whitespace_byte(text, index) {
            Some(b']' | b')') => true,
            Some(byte) if is_ident_continue_byte(byte) => {
                identifier_before(text, index).is_some_and(|(name, _)| !is_expression_keyword(name))
            }
            _ => false,
        };
        let key = &text[index + 1..close];
        if subscript && !key.contains(':') && !key.trim().is_empty() {
            spans.push(ExpressionSpan {
                text: key.to_string(),
                offset: expression.offset + index + 1,
            });
        } else {
            // The items of a list literal can hold subscripts too.
            spans.extend(subscript_key_spans(&ExpressionSpan {
                text: key.to_string(),
                offset: expression.offset + index + 1,
            }));
        }
        index = close + 1;
    }
    spans
}

fn expression_has_unsupported_reference_shape(expression: &str) -> bool {
    if expression.contains('[') || expression.contains(']') {
        return true;
    }

    let bytes = expression.as_bytes();
    let mut index = 0;
    while let Some(dot_offset) = expression[index..].find('.') {
        let dot = index + dot_offset;
        if identifier_around_dot(expression, dot).is_some_and(|base| base != "math") {
            return true;
        }
        index = dot + 1;
        if index >= bytes.len() {
            break;
        }
    }

    false
}

fn identifier_around_dot(expression: &str, dot: usize) -> Option<&str> {
    let bytes = expression.as_bytes();
    let mut left_start = dot;
    while left_start > 0 && is_ident_continue_byte(bytes[left_start - 1]) {
        left_start -= 1;
    }
    let mut right_end = dot + 1;
    while right_end < bytes.len() && is_ident_continue_byte(bytes[right_end]) {
        right_end += 1;
    }

    if left_start == dot || right_end == dot + 1 {
        return None;
    }
    Some(&expression[left_start..dot])
}

fn should_skip_identifier_reference(
    expression: &str,
    start: usize,
    end: usize,
    name: &str,
) -> bool {
    if name.chars().all(|ch| ch == '_') {
        return true;
    }

    if is_builtin_symbol(name) || is_expression_keyword(name) {
        return true;
    }

    let previous = previous_non_whitespace_byte(expression, start);
    let next = next_non_whitespace_byte(expression, end);

    matches!(previous, Some(b'.' | b'@'))
        || matches!(next, Some(b'(' | b':'))
        || name.chars().next().is_some_and(|ch| ch.is_ascii_digit())
}

fn previous_non_whitespace_byte(text: &str, before: usize) -> Option<u8> {
    text.as_bytes()
        .get(..before)?
        .iter()
        .rev()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn next_non_whitespace_byte(text: &str, after: usize) -> Option<u8> {
    text.as_bytes()
        .get(after..)?
        .iter()
        .copied()
        .find(|byte| !byte.is_ascii_whitespace())
}

fn is_ident_start_byte(byte: u8) -> bool {
    byte.is_ascii_alphabetic() || byte == b'_'
}

/// Identifier continuation bytes. Like the tokenizer, identifiers start with
/// an ASCII letter or `_` but may continue with Unicode alphanumerics
/// (`résultat`, `força`); every byte of a non-ASCII char is >= 0x80, and
/// non-alphanumeric non-ASCII text is a tokenizer error before these passes
/// run, so any such byte is part of an identifier.
fn is_ident_continue_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || byte == b'_' || byte >= 0x80
}

fn is_expression_keyword(name: &str) -> bool {
    matches!(
        name,
        "and" | "or" | "not" | "in" | "by" | "to" | "if" | "else"
    )
}

// ===========================================================================
// Storage-backed access + type mismatches (co-reporting pair in the ladder)
// ===========================================================================

/// One step of the assignment scan the storage/type checks share: enter/leave
/// a function definition's body, or analyze the assignment statement whose
/// header starts on `line`.
enum AssignmentScanEvent {
    /// A function starts. Its parameters and locals are its own names, so
    /// they don't take the types of module variables with the same names.
    EnterFunction {
        params: Vec<String>,
        locals: Vec<String>,
    },
    LeaveFunction,
    Assignment {
        line: usize,
    },
    /// A `for` loop target, with its type: a number over a `range`, and
    /// unknown over anything else.
    LoopTarget(String, CobbleType),
}

/// Assignment statements and function-scope boundaries in source order,
/// gathered from the AST walk. Assignments inside docstring ranges are
/// excluded, and statements without an aligned span node are skipped — the
/// line-anchored analysis below cannot locate them (a parseable source
/// without an aligned tree is a span-builder bug, asserted in
/// `SemanticContext::new`).
fn collect_assignment_scan_events(context: &SemanticContext<'_>) -> Vec<AssignmentScanEvent> {
    let mut events = Vec::new();
    collect_assignment_scan_events_into(
        context,
        &context.program.statements,
        context.spans.map(|tree| tree.statements.as_slice()),
        &mut events,
    );
    events
}

fn collect_assignment_scan_events_into(
    context: &SemanticContext<'_>,
    statements: &[Statement],
    nodes: Option<&[StatementSpanNode]>,
    events: &mut Vec<AssignmentScanEvent>,
) {
    for (index, statement) in statements.iter().enumerate() {
        let node = nodes
            .and_then(|nodes| nodes.get(index))
            .filter(|node| node.kind == StatementSpanKind::of(statement));

        match statement {
            Statement::Assignment(_) | Statement::ConstAssignment(_) => {
                if let Some(node) = node {
                    if !context.in_docstring(node.span.line) {
                        events.push(AssignmentScanEvent::Assignment {
                            line: node.span.line,
                        });
                    }
                }
            }
            Statement::FunctionDef(function_def) => {
                events.push(AssignmentScanEvent::EnterFunction {
                    params: function_def
                        .params
                        .iter()
                        .map(|param| param.name.clone())
                        .collect(),
                    locals: crate::transpiler::function_locals(function_def),
                });
                collect_assignment_scan_events_into(
                    context,
                    &function_def.body,
                    node.map(|node| node.children.as_slice()),
                    events,
                );
                events.push(AssignmentScanEvent::LeaveFunction);
            }
            Statement::If(if_statement) => {
                collect_assignment_scan_events_into(
                    context,
                    &if_statement.then_branch,
                    node.map(|node| node.children.as_slice()),
                    events,
                );
                for (branch_index, (_, body)) in if_statement.elif_branches.iter().enumerate() {
                    let branch_nodes = node
                        .and_then(|node| node.elif_branches.get(branch_index))
                        .map(|branch| branch.children.as_slice());
                    collect_assignment_scan_events_into(context, body, branch_nodes, events);
                }
                if let Some(body) = &if_statement.else_branch {
                    let branch_nodes = node
                        .and_then(|node| node.else_branch.as_ref())
                        .map(|branch| branch.children.as_slice());
                    collect_assignment_scan_events_into(context, body, branch_nodes, events);
                }
            }
            Statement::For(for_loop) => {
                let target_type = match &for_loop.iter {
                    Expression::Call(function, _) if matches!(&**function, Expression::Identifier(name) if name == "range") => {
                        CobbleType::Integer
                    }
                    _ => CobbleType::Unknown,
                };
                events.push(AssignmentScanEvent::LoopTarget(
                    for_loop.target.clone(),
                    target_type,
                ));
                collect_assignment_scan_events_into(
                    context,
                    &for_loop.body,
                    node.map(|node| node.children.as_slice()),
                    events,
                );
            }
            Statement::While(while_loop) => {
                collect_assignment_scan_events_into(
                    context,
                    &while_loop.body,
                    node.map(|node| node.children.as_slice()),
                    events,
                );
            }
            Statement::Execute(execute_block) => {
                collect_assignment_scan_events_into(
                    context,
                    &execute_block.body,
                    node.map(|node| node.children.as_slice()),
                    events,
                );
            }
            Statement::Match(match_statement) => {
                for (case_index, case) in match_statement.cases.iter().enumerate() {
                    let case_nodes = node
                        .and_then(|node| node.cases.get(case_index))
                        .map(|branch| branch.children.as_slice());
                    collect_assignment_scan_events_into(context, &case.body, case_nodes, events);
                }
            }
            // `a, b = pair`: each name holds an item, whose type the
            // compiler knows.
            Statement::MultiAssign(multi) => {
                for target in &multi.targets {
                    if let Expression::Identifier(name) = target {
                        events.push(AssignmentScanEvent::LoopTarget(
                            name.clone(),
                            CobbleType::Unknown,
                        ));
                    }
                }
            }
            _ => {}
        }
    }
}

/// Reports `unsupported-storage-access` for subscript/attribute access shapes
/// the storage compiler cannot resolve. The AST walk anchors WHICH lines hold
/// assignments and where function scopes begin and end; the value analysis
/// (and its type/const inference) is text-based and reads only the first
/// line of each assignment, even when the statement continues on later lines.
pub(super) fn check_storage_backed_access(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut module_types: ScopedMap<String, CobbleType> = ScopedMap::default();
    let mut module_constants: ScopedMap<String, f64> = ScopedMap::default();
    let imported = Imports::of(context.program);
    let events = collect_assignment_scan_events(context);
    // A function body runs after the whole module, so it sees the module
    // variables declared below it too. Every function scope starts from
    // this view of the MODULE environment (nested definitions included):
    // the declared types with the module-level ones over them, kept up to
    // date as module-level code runs. A function's changes to it are
    // undone when it ends, so it is never copied, except for a nested
    // definition.
    let declared_module_types = module_level_assignment_types(context, &events);
    let mut view_types: ScopedMap<String, CobbleType> =
        declared_module_types.clone().into_iter().collect();
    let mut view_constants: ScopedMap<String, f64> = ScopedMap::default();
    let mut function_scopes: Vec<ScanScope> = Vec::new();

    for event in events {
        let module_level = function_scopes.is_empty();
        let touched = match event {
            AssignmentScanEvent::EnterFunction { params, locals } => {
                let (types, constants) = match function_scopes.first() {
                    None => {
                        function_scopes
                            .push(ScanScope::Shared(view_types.open(), view_constants.open()));
                        (&mut view_types, &mut view_constants)
                    }
                    Some(ScanScope::Shared(types_mark, constants_mark)) => {
                        let types = view_types.snapshot_at(*types_mark);
                        let constants = view_constants.snapshot_at(*constants_mark);
                        function_scopes.push(ScanScope::Own(types, constants));
                        match function_scopes.last_mut() {
                            Some(ScanScope::Own(types, constants)) => (types, constants),
                            _ => unreachable!("an own scope was just pushed"),
                        }
                    }
                    Some(ScanScope::Own(..)) => unreachable!("the outermost scope is shared"),
                };
                for local in locals {
                    constants.remove(&local);
                    types.remove(&local);
                }
                // A parameter holds whatever the caller passes.
                for param in params {
                    constants.remove(&param);
                    types.insert(param, CobbleType::Unknown);
                }
                None
            }
            AssignmentScanEvent::LeaveFunction => {
                if let Some(ScanScope::Shared(types_mark, constants_mark)) = function_scopes.pop() {
                    view_types.undo_to(types_mark);
                    view_constants.undo_to(constants_mark);
                }
                None
            }
            AssignmentScanEvent::LoopTarget(target, target_type) => {
                let (type_env, constant_env) = scan_envs(
                    &mut function_scopes,
                    (&mut view_types, &mut view_constants),
                    (&mut module_types, &mut module_constants),
                );
                constant_env.remove(&target);
                type_env.insert(target.clone(), target_type);
                Some(target)
            }
            AssignmentScanEvent::Assignment { line } => {
                let line_text = context.line_text(line);
                let masked = mask_non_code(line_text);
                let trimmed = masked.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('/') {
                    continue;
                }
                let indent = masked.len() - trimmed.len();
                let Some(assignment) = assignment_span_for_type_check(line_text, trimmed, indent)
                else {
                    continue;
                };

                let (type_env, constant_env) = scan_envs(
                    &mut function_scopes,
                    (&mut view_types, &mut view_constants),
                    (&mut module_types, &mut module_constants),
                );

                if let Some(diagnostic) = unsupported_storage_access_in_expression(
                    &assignment.value,
                    type_env,
                    constant_env,
                    &imported,
                ) {
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "unsupported-storage-access",
                            line,
                            column_from_byte(
                                &masked,
                                indent + assignment.trimmed_offset(diagnostic.offset),
                            ),
                            diagnostic.message,
                        )
                        .with_help(diagnostic.help),
                    );
                    continue;
                }

                if assignment.is_const {
                    if let Some(value) =
                        evaluate_numeric_const_for_diagnostics(&assignment.value, constant_env)
                    {
                        constant_env.insert(assignment.target.clone(), value);
                    } else {
                        constant_env.remove(&assignment.target);
                    }
                } else {
                    constant_env.remove(&assignment.target);
                }

                // A field, an item, or `pop()` of a container can be a list,
                // a map, or a string: the compiler checks its use. So can a
                // variable imported from another file, whose type the
                // compiler knows.
                let new_type = infer_expression_type_for_diagnostics(&assignment.value, type_env)
                    .or_else(|| {
                        (reads_a_container(&assignment.value)
                            || imported.names.contains(assignment.value.trim()))
                        .then_some(CobbleType::Unknown)
                    });
                if let Some(new_type) = new_type {
                    match type_env.get(&assignment.target) {
                        Some(existing_type) if *existing_type != new_type => {}
                        _ => {
                            type_env.insert(assignment.target.clone(), new_type);
                        }
                    }
                }
                Some(assignment.target)
            }
        };
        // Keep the module view up to date with module-level code.
        if let Some(name) = touched.filter(|_| module_level) {
            match module_types
                .get(&name)
                .or_else(|| declared_module_types.get(&name))
            {
                Some(value_type) => view_types.insert(name.clone(), value_type.clone()),
                None => view_types.remove(&name),
            };
            match module_constants.get(&name) {
                Some(value) => view_constants.insert(name, *value),
                None => view_constants.remove(&name),
            };
        }
    }
}

/// A function scope of an assignment scan: the outermost one changes the
/// shared module view and undoes its changes at the end; a nested
/// definition starts from a copy of the module view.
enum ScanScope {
    Shared(Mark, Mark),
    Own(ScopedMap<String, CobbleType>, ScopedMap<String, f64>),
}

/// The type and constant tables the current scope of a scan reads.
fn scan_envs<'a>(
    scopes: &'a mut [ScanScope],
    view: (
        &'a mut ScopedMap<String, CobbleType>,
        &'a mut ScopedMap<String, f64>,
    ),
    module: (
        &'a mut ScopedMap<String, CobbleType>,
        &'a mut ScopedMap<String, f64>,
    ),
) -> (
    &'a mut ScopedMap<String, CobbleType>,
    &'a mut ScopedMap<String, f64>,
) {
    match scopes.last_mut() {
        Some(ScanScope::Own(types, constants)) => (types, constants),
        Some(ScanScope::Shared(..)) => view,
        None => module,
    }
}

/// The type of each module variable, from its first module-level
/// assignment whose type can be inferred.
fn module_level_assignment_types(
    context: &SemanticContext<'_>,
    events: &[AssignmentScanEvent],
) -> HashMap<String, CobbleType> {
    let mut types: HashMap<String, CobbleType> = HashMap::new();
    let mut depth = 0usize;
    for event in events {
        match event {
            AssignmentScanEvent::EnterFunction { .. } => depth += 1,
            AssignmentScanEvent::LeaveFunction => depth = depth.saturating_sub(1),
            AssignmentScanEvent::Assignment { line } if depth == 0 => {
                let line_text = context.line_text(*line);
                let masked = mask_non_code(line_text);
                let trimmed = masked.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('/') {
                    continue;
                }
                let indent = masked.len() - trimmed.len();
                let Some(assignment) = assignment_span_for_type_check(line_text, trimmed, indent)
                else {
                    continue;
                };
                if types.contains_key(&assignment.target) {
                    continue;
                }
                if let Some(value_type) =
                    infer_expression_type_for_diagnostics(&assignment.value, &types)
                {
                    types.insert(assignment.target, value_type);
                }
            }
            _ => {}
        }
    }
    // A name a function assigns under `global` is a module variable too,
    // whose type the compiler works out.
    for statement in &context.program.statements {
        if let Statement::FunctionDef(function) = statement {
            for (name, _) in crate::transpiler::global_assignments(function) {
                types.entry(name).or_insert(CobbleType::Unknown);
            }
        }
    }
    types
}

/// Reports `type-mismatch` when a variable is reassigned to a value of a
/// different inferred type. Runs after (and co-reports with) the storage
/// check under the same gate, with its own type environment.
pub(super) fn check_type_mismatches(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // A function scope changes the module table and undoes its changes at
    // the end, so the table is never copied, except for a nested definition.
    let mut module_types: ScopedMap<String, CobbleType> = ScopedMap::default();
    let mut module_mark: Option<Mark> = None;
    let mut function_scopes: Vec<Option<ScopedMap<String, CobbleType>>> = Vec::new();

    for event in collect_assignment_scan_events(context) {
        match event {
            AssignmentScanEvent::EnterFunction { params, locals } => {
                let types = match module_mark {
                    None => {
                        module_mark = Some(module_types.open());
                        function_scopes.push(None);
                        &mut module_types
                    }
                    Some(mark) => {
                        function_scopes.push(Some(module_types.snapshot_at(mark)));
                        function_scopes
                            .last_mut()
                            .and_then(Option::as_mut)
                            .expect("a nested scope was just pushed")
                    }
                };
                for name in params.iter().chain(&locals) {
                    types.remove(name);
                }
            }
            AssignmentScanEvent::LeaveFunction => {
                if let Some(None) = function_scopes.pop() {
                    if let Some(mark) = module_mark.take() {
                        module_types.undo_to(mark);
                    }
                }
            }
            AssignmentScanEvent::LoopTarget(target, _) => {
                match function_scopes.last_mut() {
                    Some(Some(types)) => types,
                    _ => &mut module_types,
                }
                .remove(&target);
            }
            AssignmentScanEvent::Assignment { line } => {
                let line_text = context.line_text(line);
                let masked = mask_non_code(line_text);
                let trimmed = masked.trim_start();
                if trimmed.is_empty() || trimmed.starts_with('/') {
                    continue;
                }
                let indent = masked.len() - trimmed.len();
                let Some(assignment) = assignment_span_for_type_check(line_text, trimmed, indent)
                else {
                    continue;
                };

                let type_env = match function_scopes.last_mut() {
                    Some(Some(types)) => types,
                    _ => &mut module_types,
                };
                // A value of unknown type, such as an item of a list, fits
                // any variable.
                let Some(new_type) =
                    infer_expression_type_for_diagnostics(&assignment.value, type_env)
                        .filter(|new_type| *new_type != CobbleType::Unknown)
                else {
                    continue;
                };

                if let Some(existing_type) = type_env
                    .get(&assignment.target)
                    .filter(|existing| **existing != CobbleType::Unknown)
                {
                    if *existing_type != new_type {
                        diagnostics.push(
                            SourceDiagnostic::error(
                                "type-mismatch",
                                line,
                                assignment.column,
                                format!("Type mismatch for variable '{}'.", assignment.target),
                            )
                            .with_help(format!(
                                "Variable was previously defined as type: {}\nCannot reassign to type: {}\nUse a different variable name or keep all assignments to '{}' the same type.",
                                existing_type.name(),
                                new_type.name(),
                                assignment.target
                            )),
                        );
                        continue;
                    }
                }

                type_env.insert(assignment.target, new_type);
            }
        }
    }
}

// --- Text analysis for the storage/type checks. It defines the exact
// (deliberately limited) inference capabilities: literal first-char shapes,
// single len()/math.* calls, and top-level operators over known references,
// on one line of RHS text.

/// What a file imports: `from other import name` names a variable of
/// another file, and `import other` a module.
struct Imports {
    names: HashSet<String>,
    modules: HashSet<String>,
}

impl Imports {
    fn of(program: &crate::ast::Program) -> Self {
        let mut imports = Imports {
            names: HashSet::new(),
            modules: HashSet::new(),
        };
        for import in &program.imports {
            if import.items.is_empty() {
                imports.modules.insert(import.module.clone());
            } else {
                imports.names.extend(import.items.iter().cloned());
            }
        }
        imports
    }

    /// Whether the file imports a whole file of the project, as `import
    /// state`, rather than the standard library.
    fn has_user_module(&self) -> bool {
        self.modules.iter().any(|module| {
            module != "stdlib" && !crate::stdlib::STDLIB_MODULES.contains(&module.as_str())
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct StorageAccessDiagnostic {
    offset: usize,
    message: String,
    help: String,
}

fn unsupported_storage_access_in_expression(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
    constant_env: &HashMap<String, f64>,
    imported: &Imports,
) -> Option<StorageAccessDiagnostic> {
    let masked = mask_non_code(expression);

    if let Some(diagnostic) =
        unsupported_subscript_access(expression, &masked, type_env, constant_env, imported)
    {
        return Some(diagnostic);
    }

    unsupported_attribute_access(&masked, type_env, imported)
}

fn unsupported_subscript_access(
    raw: &str,
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
    constant_env: &HashMap<String, f64>,
    imported: &Imports,
) -> Option<StorageAccessDiagnostic> {
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'[' {
            index += 1;
            continue;
        }

        let Some((base, base_start)) = identifier_before(expression, index) else {
            index += 1;
            continue;
        };
        // `or [1, 2]` starts a list, and a masked string literal is all `_`.
        if is_builtin_symbol(base)
            || is_expression_keyword(base)
            || base.chars().all(|ch| ch == '_')
        {
            index += 1;
            continue;
        }

        let Some(close_index) = matching_close_bracket(expression, index) else {
            index += 1;
            continue;
        };
        // The type of a key comes from the source text, where a string
        // literal still starts with a quote; the masked slice only hides
        // brackets and quotes from the structural scan.
        let raw_index_start = raw_offset_from_masked(raw, expression, index + 1);
        let raw_index_end = raw_offset_from_masked(raw, expression, close_index);
        let index_expression_raw = raw[raw_index_start..raw_index_end].trim();
        // In `x.y[1]`, `y` is a field of `x`, whatever a variable `y` holds.
        let chain_root = storage_chain_root_before_subscript(expression, base_start);
        let base_variable = if chain_root.is_some() {
            None
        } else {
            type_env.get(base)
        };
        let (base_type, nested_storage_chain) = match base_variable {
            // A parameter or a value of unknown type: the compiler decides.
            Some(CobbleType::Unknown) => {
                index = close_index + 1;
                continue;
            }
            Some(base_type) => (base_type.clone(), false),
            // A variable imported from another file: the compiler knows its
            // type.
            // `import state` makes every variable of `state.cbl` readable
            // by its own name; the compiler knows which ones there are.
            None if chain_root.is_none()
                && (imported.names.contains(base) || imported.has_user_module()) =>
            {
                index = close_index + 1;
                continue;
            }
            None => {
                let Some((root, _root_start)) = chain_root else {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Cannot resolve storage-backed subscript access `{base}[...]`"
                        ),
                        help: "Assign a list, map, or string literal to the base variable before using storage-backed access.".to_string(),
                    });
                };
                // `state.items[0]` reads a variable of another file.
                if imported.names.contains(root)
                    || (imported.modules.contains(root) && !type_env.contains_key(root))
                {
                    index = close_index + 1;
                    continue;
                }
                let Some(root_type) = type_env.get(root) else {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Cannot resolve storage-backed subscript access `{base}[...]`"
                        ),
                        help: "Assign a list, map, or string literal to the base variable before using storage-backed access.".to_string(),
                    });
                };
                // A parameter or a value of unknown type can hold a list
                // or a map.
                if !matches!(
                    root_type,
                    CobbleType::List | CobbleType::Map | CobbleType::String | CobbleType::Unknown
                ) {
                    return Some(StorageAccessDiagnostic {
                        offset: base_start,
                        message: format!(
                            "Variable `{root}` is type {}, which does not support subscript access",
                            root_type.name()
                        ),
                        help: "Use subscript access only on storage-backed list, map, or string variables."
                            .to_string(),
                    });
                }
                (CobbleType::Unknown, true)
            }
        };

        if !nested_storage_chain
            && !matches!(
                base_type,
                CobbleType::List | CobbleType::Map | CobbleType::String
            )
        {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!(
                    "Variable `{base}` is type {}, which does not support subscript access",
                    base_type.name()
                ),
                help: "Use subscript access only on storage-backed list, map, or string variables."
                    .to_string(),
            });
        }

        if base_type == CobbleType::Map {
            if is_quoted_string_literal(index_expression_raw) {
                index = close_index + 1;
                continue;
            }

            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression_raw, type_env)
            {
                if index_type != CobbleType::String && index_type != CobbleType::Unknown {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed map keys must be string values, not {}",
                            index_type.name()
                        ),
                        help: "Use a string variable, function parameter, string literal key, or attribute access for maps, such as `config[key]`, `config[\"chance\"]`, or `config.chance`.".to_string(),
                    });
                }
            }

            if is_constant_storage_index(index_expression_raw, constant_env) {
                return Some(StorageAccessDiagnostic {
                    offset: index + 1,
                    message: "Storage-backed map keys must be strings, not numeric consts"
                        .to_string(),
                    help: "Use a string variable, string literal key, or attribute access for maps, such as `config[key]`, `config[\"chance\"]`, or `config.chance`.".to_string(),
                });
            }

            index = close_index + 1;
            continue;
        }

        if nested_storage_chain {
            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression_raw, type_env)
            {
                if !matches!(
                    index_type,
                    CobbleType::Integer | CobbleType::String | CobbleType::Unknown
                ) {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed indexes must be integer or string values, not {}",
                            index_type.name()
                        ),
                        help: "Use integer scores for list/string indexes or string values for map keys in storage-backed chains.".to_string(),
                    });
                }
            }
            index = close_index + 1;
            continue;
        }

        let is_static_index = is_literal_storage_index(index_expression_raw)
            || is_constant_storage_index(index_expression_raw, constant_env);
        if !is_static_index {
            if let Some(index_type) =
                infer_expression_type_for_diagnostics(index_expression_raw, type_env)
            {
                // A value of unknown type, such as a parameter, may be one.
                if !matches!(index_type, CobbleType::Integer | CobbleType::Unknown) {
                    return Some(StorageAccessDiagnostic {
                        offset: index + 1,
                        message: format!(
                            "Dynamic storage-backed list indexes must be integer scores, not {}",
                            index_type.name()
                        ),
                        help: "Use an integer score expression for runtime list/string indexes, such as `items[i]` after `i = 0`.".to_string(),
                    });
                }
            }
        }

        index = close_index + 1;
    }

    None
}

fn unsupported_attribute_access(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
    imported: &Imports,
) -> Option<StorageAccessDiagnostic> {
    let bytes = expression.as_bytes();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes[index] != b'.' {
            index += 1;
            continue;
        }

        let Some((base, base_start)) = identifier_before(expression, index) else {
            index += 1;
            continue;
        };
        let Some((_field, field_end)) = identifier_after(expression, index + 1) else {
            index += 1;
            continue;
        };
        if matches!(next_non_whitespace_byte(expression, field_end), Some(b'(')) {
            index = field_end;
            continue;
        }
        // Only the root of a chain names a variable: in `cfg.inner.c`,
        // `inner` is a field of `cfg`.
        if is_builtin_symbol(base) || expression[..base_start].trim_end().ends_with('.') {
            index += 1;
            continue;
        }

        // A variable imported from another file: the compiler knows its type.
        if imported.names.contains(base) {
            index += 1;
            continue;
        }
        if imported.modules.contains(base) && !type_env.contains_key(base) {
            let field = identifier_after(expression, index + 1).map_or("name", |(field, _)| field);
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!(
                    "`{base}.{field}` names a variable of the module `{base}`, which this file can't read through the module name"
                ),
                help: format!(
                    "Import the variable: `from {base} import {field}`, then use `{field}`."
                ),
            });
        }
        let Some(base_type) = type_env.get(base) else {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!("Cannot resolve storage-backed attribute access `{base}.`"),
                help: "Assign a map literal to the base variable before using storage-backed attribute access.".to_string(),
            });
        };

        if !matches!(base_type, CobbleType::Map | CobbleType::Unknown) {
            return Some(StorageAccessDiagnostic {
                offset: base_start,
                message: format!(
                    "Variable `{base}` is type {}, which does not support attribute access",
                    base_type.name()
                ),
                help: "Use attribute access only on storage-backed map variables.".to_string(),
            });
        }

        index += 1;
    }

    None
}

fn identifier_before(text: &str, before: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut end = before;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    let mut start = end;
    while start > 0 && is_ident_continue_byte(bytes[start - 1]) {
        start -= 1;
    }
    if start == end || !is_ident_start_byte(bytes[start]) {
        return None;
    }
    Some((&text[start..end], start))
}

fn identifier_after(text: &str, after: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut start = after;
    while start < bytes.len() && bytes[start].is_ascii_whitespace() {
        start += 1;
    }
    if start >= bytes.len() || !is_ident_start_byte(bytes[start]) {
        return None;
    }
    let mut end = start + 1;
    while end < bytes.len() && is_ident_continue_byte(bytes[end]) {
        end += 1;
    }
    Some((&text[start..end], end))
}

/// Whether `expression` reads a field or an item, or calls a method, as
/// `cfg.x`, `rows[0][1]`, or `queue.pop(0)`.
fn reads_a_container(expression: &str) -> bool {
    let expression = expression.trim();
    let starts_with_name = expression
        .as_bytes()
        .first()
        .is_some_and(|byte| is_ident_start_byte(*byte));
    starts_with_name
        && expression
            .bytes()
            .find(|byte| !is_ident_continue_byte(*byte))
            .is_some_and(|byte| matches!(byte, b'.' | b'['))
}

fn storage_chain_root_before_subscript(text: &str, base_start: usize) -> Option<(&str, usize)> {
    let bytes = text.as_bytes();
    let mut cursor = base_start;
    let mut chain_start = base_start;

    loop {
        let mut dot = cursor;
        while dot > 0 && bytes[dot - 1].is_ascii_whitespace() {
            dot -= 1;
        }
        if dot == 0 || bytes[dot - 1] != b'.' {
            break;
        }

        let segment_start = storage_chain_segment_start_before(text, dot - 1)?;
        chain_start = segment_start;
        cursor = segment_start;
    }

    if chain_start == base_start {
        return None;
    }
    identifier_after(text, chain_start)
}

fn storage_chain_segment_start_before(text: &str, before_dot: usize) -> Option<usize> {
    let bytes = text.as_bytes();
    let mut end = before_dot;
    while end > 0 && bytes[end - 1].is_ascii_whitespace() {
        end -= 1;
    }
    if end == 0 {
        return None;
    }

    // `rows[0][1].s` has several subscripts before the dot.
    while bytes[end - 1] == b']' {
        end = matching_open_bracket(text, end - 1)?;
        while end > 0 && bytes[end - 1].is_ascii_whitespace() {
            end -= 1;
        }
        if end == 0 {
            return None;
        }
    }

    let (_name, start) = identifier_before(text, end)?;
    Some(start)
}

fn matching_close_bracket(expression: &str, open_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().skip(open_index) {
        match byte {
            b'[' => depth += 1,
            b']' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn matching_open_bracket(expression: &str, close_index: usize) -> Option<usize> {
    let bytes = expression.as_bytes();
    let mut depth = 0usize;

    for (index, byte) in bytes.iter().enumerate().take(close_index + 1).rev() {
        match byte {
            b']' => depth += 1,
            b'[' => {
                depth = depth.saturating_sub(1);
                if depth == 0 {
                    return Some(index);
                }
            }
            _ => {}
        }
    }

    None
}

fn is_literal_storage_index(index_expression: &str) -> bool {
    let index_expression = index_expression.trim();
    is_numeric_literal(index_expression) || is_quoted_string_literal(index_expression)
}

fn is_quoted_string_literal(index_expression: &str) -> bool {
    let index_expression = index_expression.trim();
    index_expression.len() >= 2
        && matches!(index_expression.as_bytes()[0], b'"' | b'\'')
        && index_expression
            .as_bytes()
            .last()
            .is_some_and(|last| *last == index_expression.as_bytes()[0])
}

fn is_constant_storage_index(index_expression: &str, constant_env: &HashMap<String, f64>) -> bool {
    let index_expression = index_expression.trim();
    is_simple_module_name(index_expression) && constant_env.contains_key(index_expression)
}

pub(super) fn evaluate_numeric_const_for_diagnostics(
    expression: &str,
    constant_env: &HashMap<String, f64>,
) -> Option<f64> {
    let expression = strip_balanced_outer_parens(expression.trim());
    if expression.is_empty() {
        return None;
    }

    if let Ok(value) = expression.parse::<f64>() {
        return Some(value);
    }
    if is_simple_module_name(expression) {
        return constant_env.get(expression).copied();
    }

    if let Some(rest) = expression.strip_prefix('+') {
        return evaluate_numeric_const_for_diagnostics(rest, constant_env);
    }
    if let Some(rest) = expression.strip_prefix('-') {
        return evaluate_numeric_const_for_diagnostics(rest, constant_env).map(|value| -value);
    }

    let operator_groups: [&[&str]; 3] = [&["+", "-"], &["*", "/", "%"], &["^"]];
    for operators in operator_groups {
        if let Some((left, operator, right)) =
            split_top_level_binary_operator(expression, operators)
        {
            let left = evaluate_numeric_const_for_diagnostics(left, constant_env)?;
            let right = evaluate_numeric_const_for_diagnostics(right, constant_env)?;
            return match operator {
                "+" => Some(left + right),
                "-" => Some(left - right),
                "*" => Some(left * right),
                "/" if right != 0.0 => Some(left / right),
                "%" if (right as i32) != 0 => Some(((left as i32) % (right as i32)) as f64),
                // Negative constant exponents cannot be folded to an integer
                // result; leave them unfolded instead of casting through u32.
                "^" if right >= 0.0 => Some((left as i32).checked_pow(right as u32)? as f64),
                _ => None,
            };
        }
    }

    None
}

fn strip_balanced_outer_parens(expression: &str) -> &str {
    let mut expression = expression.trim();
    loop {
        if !expression.starts_with('(') || !expression.ends_with(')') {
            return expression;
        }
        let Some(close_index) = matching_close_paren(expression, 0) else {
            return expression;
        };
        if close_index != expression.len() - 1 {
            return expression;
        }
        expression = expression[1..expression.len() - 1].trim();
    }
}

fn split_top_level_binary_operator<'a>(
    expression: &'a str,
    operators: &[&'static str],
) -> Option<(&'a str, &'static str, &'a str)> {
    let mut delimiter_depth = 0usize;

    for (index, ch) in expression.char_indices().rev() {
        match ch {
            ')' | ']' | '}' => {
                delimiter_depth += 1;
                continue;
            }
            '(' | '[' | '{' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                continue;
            }
            _ => {}
        }

        if delimiter_depth != 0 {
            continue;
        }

        for operator in operators {
            if !expression[index..].starts_with(operator) {
                continue;
            }
            if matches!(*operator, "+" | "-") && is_unary_sign(expression, index) {
                continue;
            }
            let right_start = index + operator.len();
            if expression[..index].trim().is_empty() || expression[right_start..].trim().is_empty()
            {
                continue;
            }
            return Some((&expression[..index], *operator, &expression[right_start..]));
        }
    }

    None
}

fn is_unary_sign(expression: &str, index: usize) -> bool {
    expression[..index]
        .chars()
        .rev()
        .find(|ch| !ch.is_whitespace())
        .is_none_or(|ch| matches!(ch, '(' | '[' | '{' | '+' | '-' | '*' | '/' | '%' | '^'))
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct TypeAssignmentSpan {
    target: String,
    value: String,
    value_offset: usize,
    column: usize,
    is_const: bool,
    /// Set for compound assignments, whose `value` is the synthesized
    /// `target op (rhs)` text rather than a slice of the source line.
    compound: Option<CompoundValueMapping>,
}

/// Where the parts of a synthesized `target op (rhs)` value sit in the
/// trimmed source line `target op= rhs`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CompoundValueMapping {
    target_offset: usize,
    target_len: usize,
    /// Length of the synthesized `target op (` prefix.
    prefix_len: usize,
    rhs_offset: usize,
}

impl TypeAssignmentSpan {
    /// Map a byte offset inside `value` back to a byte offset in the trimmed
    /// source line. Plain assignments slice the line, so the mapping is a
    /// shift; compound assignments map the synthesized target and rhs parts
    /// back to where they were written (`x+=m[a]` has no `(` to point at).
    fn trimmed_offset(&self, offset_in_value: usize) -> usize {
        let Some(mapping) = self.compound else {
            return self.value_offset + offset_in_value;
        };
        if offset_in_value < mapping.target_len {
            mapping.target_offset + offset_in_value
        } else if offset_in_value >= mapping.prefix_len {
            mapping.rhs_offset + (offset_in_value - mapping.prefix_len)
        } else {
            mapping.target_offset + mapping.target_len
        }
    }
}

fn assignment_span_for_type_check(
    line: &str,
    masked_trimmed: &str,
    indent: usize,
) -> Option<TypeAssignmentSpan> {
    if should_skip_assignment_symbol_scan(masked_trimmed) {
        return None;
    }

    if let Some(compound) = compound_assignment_span(masked_trimmed) {
        if !is_simple_module_name(&compound.target) {
            return None;
        }

        let raw_trimmed = line.trim_start();
        let raw_rhs = raw_trimmed.get(compound.value_offset..)?;
        let stripped_rhs = strip_comment_preserving_strings(raw_rhs);
        let rhs_trim_start = stripped_rhs.len() - stripped_rhs.trim_start().len();
        let value = stripped_rhs.trim();
        if value.is_empty() {
            return None;
        }

        return Some(TypeAssignmentSpan {
            target: compound.target.clone(),
            value: format!("{} {} ({})", compound.target, compound.operator, value),
            value_offset: compound.target_offset,
            column: indent + column_from_byte(masked_trimmed, compound.target_offset),
            is_const: false,
            compound: Some(CompoundValueMapping {
                target_offset: compound.target_offset,
                target_len: compound.target.len(),
                prefix_len: compound.target.len() + compound.operator.len() + 3,
                rhs_offset: compound.value_offset + rhs_trim_start,
            }),
        });
    }

    let equals_index = single_equals_index(masked_trimmed)?;
    let raw_target = masked_trimmed[..equals_index].trim();
    let is_const = raw_target.starts_with("const ");
    let target = raw_target
        .strip_prefix("const ")
        .or_else(|| raw_target.strip_prefix("persist "))
        .unwrap_or(raw_target)
        .trim();
    if !is_simple_module_name(target) {
        return None;
    }

    let raw_trimmed = line.trim_start();
    let raw_rhs = raw_trimmed.get(equals_index + 1..)?;
    let stripped_rhs = strip_comment_preserving_strings(raw_rhs);
    let trim_start = stripped_rhs.len() - stripped_rhs.trim_start().len();
    let value = stripped_rhs.trim().to_string();
    if value.is_empty() {
        return None;
    }

    Some(TypeAssignmentSpan {
        target: target.to_string(),
        value,
        value_offset: equals_index + 1 + trim_start,
        column: indent + column_from_byte(masked_trimmed, raw_target.find(target).unwrap_or(0)),
        is_const,
        compound: None,
    })
}

fn infer_expression_type_for_diagnostics(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
) -> Option<CobbleType> {
    let expression = expression.trim();
    if expression.is_empty() {
        return None;
    }
    // `(a + b)` has the type of `a + b`.
    if expression.starts_with('(')
        && matching_close_paren(&mask_non_code(expression), 0) == Some(expression.len() - 1)
    {
        return infer_expression_type_for_diagnostics(
            &expression[1..expression.len() - 1],
            type_env,
        );
    }

    // A conditional expression can give either type.
    if contains_top_level_keyword(expression, "if") {
        return Some(CobbleType::Unknown);
    }
    // A comparison or `in` gives 1 or 0 whatever it compares, as in
    // `{"k": 1} == m` or `"k" in m`.
    if contains_top_level_operator(expression, &["==", "!=", "<=", ">=", "<", ">"])
        || contains_top_level_keyword(expression, "in")
    {
        return Some(CobbleType::Boolean);
    }

    if expression.starts_with('"') || expression.starts_with('\'') {
        return Some(CobbleType::String);
    }
    if expression == "True" || expression == "False" {
        return Some(CobbleType::Boolean);
    }
    if expression.starts_with('[') {
        return Some(CobbleType::List);
    }
    if expression.starts_with('{') {
        return Some(CobbleType::Map);
    }
    let function_calls = function_calls_in_expression(expression);
    if let Some(builtin) = function_calls.first().filter(|call| {
        expression.starts_with(&format!("{}(", call.name))
            && matching_close_paren(expression, call.name.len()) == Some(expression.len() - 1)
    }) {
        match builtin.name.as_str() {
            "sorted" => return Some(CobbleType::List),
            "str" => return Some(CobbleType::String),
            "abs" | "max" | "min" | "sum" => return Some(CobbleType::Integer),
            _ => {}
        }
    }
    if expression.starts_with("len(") && function_calls.len() == 1 {
        let call = &function_calls[0];
        if call.name == "len"
            && call.arg_count == 1
            && expression_references_are_known(expression, type_env)
        {
            return Some(CobbleType::Integer);
        }
    }
    if expression.starts_with("math.") && function_calls.len() == 1 {
        let call = &function_calls[0];
        if math_value_function_arity(&call.name).is_some_and(|arity| arity == call.arg_count)
            && expression_references_are_known(expression, type_env)
        {
            return Some(CobbleType::Integer);
        }
    }
    if is_numeric_literal(expression) {
        return Some(CobbleType::Integer);
    }
    if is_simple_module_name(expression) {
        return type_env.get(expression).cloned();
    }
    // A character of a string is a string; an item of a list or a field of
    // a map can be anything.
    if let Some(open) = expression.find('[') {
        let base = &expression[..open];
        if is_simple_module_name(base)
            && matching_close_bracket(expression, open) == Some(expression.len() - 1)
        {
            match type_env.get(base) {
                Some(CobbleType::String) => return Some(CobbleType::String),
                Some(CobbleType::List | CobbleType::Map | CobbleType::Unknown) => {
                    return Some(CobbleType::Unknown)
                }
                _ => {}
            }
        }
    }
    // `a or b` gives an operand, which is a boolean only for comparisons.
    if !contains_top_level_operator(expression, &["==", "!=", "<=", ">=", "<", ">"])
        && (find_word(expression, "and").is_some() || find_word(expression, "or").is_some())
    {
        return Some(CobbleType::Unknown);
    }
    if expression_contains_boolean_operator(expression)
        && expression_references_are_known(expression, type_env)
    {
        return Some(CobbleType::Boolean);
    }
    if expression_contains_arithmetic_operator(expression)
        && expression_references_are_known(expression, type_env)
    {
        // `+` and `*` also join and repeat text and lists.
        let holds_values = expression.contains(['"', '\'', '[', '{'])
            || identifier_references_in_expression(expression)
                .iter()
                .any(|reference| {
                    matches!(
                        type_env.get(&reference.name),
                        Some(CobbleType::String | CobbleType::List | CobbleType::Map)
                    )
                });
        if holds_values {
            return Some(CobbleType::Unknown);
        }
        return Some(CobbleType::Integer);
    }

    None
}

/// Whether `keyword` stands as a word outside brackets and strings.
fn contains_top_level_keyword(expression: &str, keyword: &str) -> bool {
    let mut depth = 0usize;
    let mut quote = None;
    let mut previous_is_ident = false;
    for (index, ch) in expression.char_indices() {
        if let Some(active) = quote {
            if ch == active {
                quote = None;
            }
            previous_is_ident = false;
            continue;
        }
        match ch {
            '"' | '\'' => quote = Some(ch),
            '(' | '[' | '{' => depth += 1,
            ')' | ']' | '}' => depth = depth.saturating_sub(1),
            _ => {}
        }
        if depth == 0 && !previous_is_ident && expression[index..].starts_with(keyword) {
            let after = expression[index + keyword.len()..].chars().next();
            if !after.is_some_and(|next| next.is_alphanumeric() || next == '_') {
                return true;
            }
        }
        previous_is_ident = ch.is_alphanumeric() || ch == '_';
    }
    false
}

fn expression_references_are_known(
    expression: &str,
    type_env: &HashMap<String, CobbleType>,
) -> bool {
    // A word inside a string literal, as `z` in `s + "z"`, is no reference.
    identifier_references_in_expression(&mask_non_code(expression))
        .into_iter()
        .all(|reference| {
            is_builtin_symbol(&reference.name) || type_env.contains_key(&reference.name)
        })
}

/// `text` with each comment replaced by spaces, so offsets stay the same.
fn blank_comments(text: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut quote = None;
    let mut escaped = false;
    let mut in_comment = false;
    for ch in text.chars() {
        if in_comment {
            in_comment = ch != '\n';
        } else if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
        } else if ch == '"' || ch == '\'' {
            quote = Some(ch);
        } else if ch == '#' {
            in_comment = true;
        }
        if in_comment {
            result.extend(std::iter::repeat_n(' ', ch.len_utf8()));
        } else {
            result.push(ch);
        }
    }
    result
}

fn strip_comment_preserving_strings(text: &str) -> String {
    let mut output = String::with_capacity(text.len());
    let mut quote = None;
    let mut escaped = false;

    for (index, ch) in text.char_indices() {
        if let Some(active_quote) = quote {
            output.push(ch);
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                output.push(ch);
            }
            '#' if !crate::parser::tokenizer::hash_starts_comment(text, index) => {
                output.push(ch);
            }
            '#' => break,
            _ => output.push(ch),
        }
    }

    output
}

pub(super) fn is_numeric_literal(expression: &str) -> bool {
    let expression = expression.trim();
    if expression.is_empty() {
        return false;
    }

    let digits = expression.strip_prefix('-').unwrap_or(expression);
    digits.parse::<f64>().is_ok()
}

fn expression_contains_boolean_operator(expression: &str) -> bool {
    contains_top_level_operator(expression, &["==", "!=", "<=", ">=", "<", ">"])
        || find_word(expression, "and").is_some()
        || find_word(expression, "or").is_some()
        || expression
            .trim_start()
            .strip_prefix("not ")
            .is_some_and(|rest| !rest.trim().is_empty())
}

fn expression_contains_arithmetic_operator(expression: &str) -> bool {
    contains_top_level_operator(expression, &["+", "-", "*", "/", "%", "^"])
        || expression
            .trim_start()
            .strip_prefix('-')
            .is_some_and(|rest| !rest.trim().is_empty())
        || expression
            .trim_start()
            .strip_prefix('+')
            .is_some_and(|rest| !rest.trim().is_empty())
}

fn contains_top_level_operator(expression: &str, operators: &[&str]) -> bool {
    let bytes = expression.as_bytes();
    let mut delimiter_depth = 0usize;
    let mut index = 0;

    while index < bytes.len() {
        match bytes[index] {
            b'(' | b'[' | b'{' => {
                delimiter_depth += 1;
                index += 1;
                continue;
            }
            b')' | b']' | b'}' => {
                delimiter_depth = delimiter_depth.saturating_sub(1);
                index += 1;
                continue;
            }
            _ => {}
        }

        if delimiter_depth == 0 {
            for operator in operators {
                // Compare bytes: `index` walks raw byte positions, so slicing
                // `expression` here would panic inside a multi-byte character.
                // Every operator is ASCII, so a match also proves `index` sits
                // on a char boundary.
                if bytes[index..].starts_with(operator.as_bytes()) {
                    if *operator == "-" || *operator == "+" {
                        let previous = previous_non_whitespace_byte(expression, index);
                        if previous.is_none_or(|byte| {
                            matches!(
                                byte,
                                b'=' | b'!'
                                    | b'<'
                                    | b'>'
                                    | b'+'
                                    | b'-'
                                    | b'*'
                                    | b'/'
                                    | b'%'
                                    | b'^'
                                    | b'('
                                    | b'['
                                    | b'{'
                            )
                        }) {
                            continue;
                        }
                    }
                    return true;
                }
            }
        }

        index += 1;
    }

    false
}

// ===========================================================================
// Raw command placeholders (hybrid: command statements anchored on the span
// sidecar; placeholder columns from an in-line scan of the raw command text)
// ===========================================================================

/// Scope state for the placeholder pass. Unlike the undefined-variable
/// check's whole-file forward-referencing table, this pass is INCREMENTAL by
/// design: a placeholder resolves only against symbols defined ABOVE the
/// command (pinned by `reports_forward_raw_command_placeholders`),
/// module-level names accumulate for the rest of the file, function-local
/// names live for the enclosing definition only, and `for` targets for the
/// loop body only. Import items from the whole file seed the module scope up
/// front.
struct PlaceholderScopes {
    module: HashSet<String>,
    /// Innermost function scope last; a command resolves against the LAST
    /// entry only (nested definitions do not inherit outer locals or
    /// parameters).
    function_stack: Vec<HashSet<String>>,
    /// Targets of every enclosing `for` loop, as a stack; they stay visible
    /// inside nested definitions.
    loop_targets: Vec<String>,
}

impl PlaceholderScopes {
    /// Where a new assignment/`global` name lands: the innermost enclosing
    /// function scope, or the module scope outside any definition.
    fn defining_set(&mut self) -> &mut HashSet<String> {
        self.function_stack.last_mut().unwrap_or(&mut self.module)
    }

    /// Membership is a plain name check that ignores builtin symbols.
    fn is_visible(&self, name: &str) -> bool {
        self.module.contains(name)
            || self
                .function_stack
                .last()
                .is_some_and(|scope| scope.contains(name))
            || self.loop_targets.iter().any(|target| target == name)
    }
}

/// Reports unclosed/invalid/undefined placeholders in raw `/` command
/// statements. Commands are anchored on their `MinecraftCommand` span nodes;
/// placeholder extraction and columns come from a quote/escape-aware scan of
/// the raw command line. Statements on docstring-range lines are skipped
/// wholesale (symbol collection and command scanning).
pub(super) fn check_raw_command_placeholders(
    context: &SemanticContext<'_>,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let mut module = collect_interpolation_import_symbols(context);
    // A function body runs after the whole module, so a module name declared
    // below it is visible, as in Python.
    module.extend(collect_module_scope_names(context));
    let mut scopes = PlaceholderScopes {
        module,
        function_stack: Vec::new(),
        loop_targets: Vec::new(),
    };
    walk_placeholder_statements(
        context,
        &context.program.statements,
        context.spans.map(|tree| tree.statements.as_slice()),
        &mut scopes,
        diagnostics,
    );
}

/// Import items (`from X import a, b`) seed the placeholder symbol table;
/// plain `import X` module names deliberately do NOT (pinned by
/// `does_not_treat_imported_modules_as_raw_command_placeholders`) — the
/// asymmetry is inverted from the undefined-variable collector's
/// `add_import_symbols`. Imports on docstring-range lines are skipped.
fn collect_interpolation_import_symbols(context: &SemanticContext<'_>) -> HashSet<String> {
    let mut symbols = HashSet::new();

    for (index, import) in context.program.imports.iter().enumerate() {
        let in_docstring = context
            .import_spans
            .get(index)
            .is_some_and(|span| context.in_docstring(span.line));
        if !in_docstring {
            add_interpolation_import_items(import, &mut symbols);
        }
    }
    context.walk(&mut |statement, node| {
        if let Statement::Import(import) = statement {
            if !node.is_some_and(|node| context.in_docstring(node.span.line)) {
                add_interpolation_import_items(import, &mut symbols);
            }
        }
    });

    symbols
}

/// Names bound at module level anywhere in the file, and names a function
/// declares `global`.
fn collect_module_scope_names(context: &SemanticContext<'_>) -> HashSet<String> {
    let mut names: HashSet<String> = collect_module_variable_defs(context)
        .into_iter()
        .map(|variable| variable.name)
        .collect();
    for statement in &context.program.statements {
        if let Statement::ConstAssignment(assignment) = statement {
            if is_simple_module_name(&assignment.target) {
                names.insert(assignment.target.clone());
            }
        }
    }
    context.walk(&mut |statement, node| {
        if let Statement::Global(globals) = statement {
            if !node.is_some_and(|node| context.in_docstring(node.span.line)) {
                names.extend(
                    globals
                        .iter()
                        .filter(|name| is_simple_module_name(name))
                        .cloned(),
                );
            }
        }
    });
    names
}

fn add_interpolation_import_items(import: &Import, symbols: &mut HashSet<String>) {
    for item in &import.items {
        if is_simple_module_name(item) {
            symbols.insert(item.clone());
        }
    }
}

/// Source-order walk with incremental bookkeeping: assignment/`global` names
/// land in the innermost scope as they are passed, and each raw command is
/// checked against what is visible at that point.
fn walk_placeholder_statements(
    context: &SemanticContext<'_>,
    statements: &[Statement],
    nodes: Option<&[StatementSpanNode]>,
    scopes: &mut PlaceholderScopes,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    for (index, statement) in statements.iter().enumerate() {
        let node = nodes
            .and_then(|nodes| nodes.get(index))
            .filter(|node| node.kind == StatementSpanKind::of(statement));
        // Docstring prose parses into real statements; skip every statement
        // on a docstring line.
        if node.is_some_and(|node| context.in_docstring(node.span.line)) {
            continue;
        }

        match statement {
            Statement::FunctionDef(function_def) => {
                scopes.function_stack.push(
                    function_def
                        .params
                        .iter()
                        .map(|parameter| parameter.name.clone())
                        .collect(),
                );
                walk_placeholder_statements(
                    context,
                    &function_def.body,
                    node.map(|node| node.children.as_slice()),
                    scopes,
                    diagnostics,
                );
                scopes.function_stack.pop();
            }
            Statement::MultiAssign(multi) => {
                for target in &multi.targets {
                    if let Expression::Identifier(name) = target {
                        scopes.defining_set().insert(name.clone());
                    }
                }
            }
            Statement::For(for_loop) => {
                let scoped_target = is_simple_module_name(&for_loop.target);
                let loop_targets = scopes.loop_targets.len();
                if scoped_target {
                    scopes.loop_targets.push(for_loop.target.clone());
                }
                scopes.loop_targets.extend(for_loop.index.iter().cloned());
                walk_placeholder_statements(
                    context,
                    &for_loop.body,
                    node.map(|node| node.children.as_slice()),
                    scopes,
                    diagnostics,
                );
                scopes.loop_targets.truncate(loop_targets);
                // As in Python, the targets keep their last value after the
                // loop.
                let targets = scoped_target.then(|| for_loop.target.clone());
                scopes
                    .defining_set()
                    .extend(targets.into_iter().chain(for_loop.index.iter().cloned()));
            }
            Statement::While(while_loop) => {
                walk_placeholder_statements(
                    context,
                    &while_loop.body,
                    node.map(|node| node.children.as_slice()),
                    scopes,
                    diagnostics,
                );
            }
            Statement::Execute(execute_block) => {
                walk_placeholder_statements(
                    context,
                    &execute_block.body,
                    node.map(|node| node.children.as_slice()),
                    scopes,
                    diagnostics,
                );
            }
            Statement::If(if_statement) => {
                walk_placeholder_statements(
                    context,
                    &if_statement.then_branch,
                    node.map(|node| node.children.as_slice()),
                    scopes,
                    diagnostics,
                );
                for (branch_index, (_, body)) in if_statement.elif_branches.iter().enumerate() {
                    let branch_nodes = node
                        .and_then(|node| node.elif_branches.get(branch_index))
                        .map(|branch| branch.children.as_slice());
                    walk_placeholder_statements(context, body, branch_nodes, scopes, diagnostics);
                }
                if let Some(body) = &if_statement.else_branch {
                    let branch_nodes = node
                        .and_then(|node| node.else_branch.as_ref())
                        .map(|branch| branch.children.as_slice());
                    walk_placeholder_statements(context, body, branch_nodes, scopes, diagnostics);
                }
            }
            Statement::Match(match_statement) => {
                for (case_index, case) in match_statement.cases.iter().enumerate() {
                    let case_nodes = node
                        .and_then(|node| node.cases.get(case_index))
                        .map(|branch| branch.children.as_slice());
                    walk_placeholder_statements(
                        context,
                        &case.body,
                        case_nodes,
                        scopes,
                        diagnostics,
                    );
                }
            }
            Statement::Assignment(assignment) => {
                // Compound assignments desugar to plain assignments in the
                // AST, so their targets are collected here too; non-name
                // targets (`x[0]`, `x.y`) fall out of the simple-name filter.
                if is_simple_module_name(&assignment.target) {
                    scopes.defining_set().insert(assignment.target.clone());
                }
            }
            Statement::ConstAssignment(assignment) => {
                if is_simple_module_name(&assignment.target) {
                    scopes.defining_set().insert(assignment.target.clone());
                }
            }
            Statement::Global(names) => {
                for name in names {
                    if is_simple_module_name(name) {
                        scopes.defining_set().insert(name.clone());
                    }
                }
            }
            Statement::MinecraftCommand(_) => {
                if let Some(node) = node {
                    scan_command_placeholders(context, node.span, scopes, diagnostics);
                }
            }
            // Imports seed the module scope up front; selector aliases,
            // entity templates, `create`, `pass`, `return`, and expression
            // statements define no placeholder symbols.
            _ => {}
        }
    }
}

fn scan_command_placeholders(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    scopes: &PlaceholderScopes,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let line_number = span.line;
    let line = context.line_text(line_number);
    let indent = span.start.saturating_sub(context.line_start(line_number));

    for placeholder in raw_command_placeholders(line, indent) {
        if placeholder.name.is_empty() {
            diagnostics.push(
                SourceDiagnostic::error(
                    "unclosed-placeholder",
                    line_number,
                    placeholder.column,
                    "Command placeholder or brace expression is not closed",
                )
                .with_help("Close the brace expression, or write `{{name}}` for literal braces."),
            );
            continue;
        }

        if !is_simple_module_name(&placeholder.name) {
            diagnostics.push(
                SourceDiagnostic::error(
                    "invalid-placeholder",
                    line_number,
                    placeholder.column,
                    format!("Invalid command placeholder `{}`", placeholder.name),
                )
                .with_help(
                    "Use identifier placeholders such as `{player_name}`, or write doubled braces like `{{literal}}` for literal text.",
                ),
            );
            continue;
        }

        if scopes.is_visible(&placeholder.name) {
            continue;
        }

        diagnostics.push(
            SourceDiagnostic::error(
                "undefined-placeholder",
                line_number,
                placeholder.column,
                format!("Undefined command placeholder `{}`", placeholder.name),
            )
            .with_help(format!(
                "Define `{}` before using it, pass it as a function parameter, or write `{{{{{}}}}}` for literal braces.",
                placeholder.name, placeholder.name
            )),
        );
    }
}

/// Every raw command placeholder named `name`, as (line, column), in source
/// order — the position source for `unsupported-placeholder-symbol` in the
/// cross-file import validation.
pub(super) fn raw_command_placeholder_locations(
    context: &SemanticContext<'_>,
    name: &str,
) -> Vec<(usize, usize)> {
    let mut locations = Vec::new();
    context.walk(&mut |statement, node| {
        if !matches!(statement, Statement::MinecraftCommand(_)) {
            return;
        }
        let Some(node) = node else {
            return;
        };
        if context.in_docstring(node.span.line) {
            return;
        }
        let line = context.line_text(node.span.line);
        let indent = node
            .span
            .start
            .saturating_sub(context.line_start(node.span.line));
        for placeholder in raw_command_placeholders(line, indent) {
            if placeholder.name == name {
                locations.push((node.span.line, placeholder.column));
            }
        }
    });
    locations
}

// --- Placeholder extraction: a quote/escape-aware scan of one command line.
// It defines what counts as a placeholder and its column; the statement walk
// above only decides WHICH lines are command lines.

#[derive(Debug, Clone, PartialEq, Eq)]
struct CommandPlaceholder {
    name: String,
    column: usize,
}

fn raw_command_placeholders(line: &str, indent: usize) -> Vec<CommandPlaceholder> {
    let mut placeholders = Vec::new();
    let mut index = indent;
    let mut quote = None;
    let mut escaped = false;
    // Both scans would otherwise start over for every brace or placeholder,
    // which a long line makes quadratic.
    let mut closes: HashMap<usize, Option<usize>> = HashMap::new();
    let mut columns = ColumnCounter::new(line);
    let mut reported_unclosed = false;

    while index < line.len() {
        let ch = line[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
                index = next_index;
                continue;
            }
            if ch == '\\' {
                escaped = true;
                index = next_index;
                continue;
            }
            if ch == '{' {
                // `{{` is a literal brace; a placeholder between escapes is
                // still a placeholder, as in a Python format string.
                if line[index..].starts_with("{{") {
                    index += 2;
                    continue;
                }
                if let Some(close_index) = placeholder_close_before_quote(line, index, active_quote)
                {
                    let content = line[index + 1..close_index].trim();
                    if is_potential_command_placeholder(content) {
                        placeholders.push(CommandPlaceholder {
                            name: content.to_string(),
                            column: columns.at(index),
                        });
                        index = close_index + 1;
                        continue;
                    }
                }
            }
            if ch == active_quote {
                quote = None;
            }
            index = next_index;
            continue;
        }

        if matches!(ch, '"' | '\'') {
            quote = Some(ch);
            escaped = false;
            index = next_index;
            continue;
        }

        if ch != '{' {
            index = next_index;
            continue;
        }

        if line[index..].starts_with("{{") {
            index += 2;
            continue;
        }

        let Some(close_index) = matching_brace_quote_aware(line, index, &mut closes) else {
            // One report per line: every brace after an unclosed one is
            // unclosed too, and each report repeats the line.
            if !reported_unclosed {
                reported_unclosed = true;
                placeholders.push(CommandPlaceholder {
                    name: String::new(),
                    column: columns.at(index),
                });
            }
            index = next_index;
            continue;
        };

        let content = line[index + 1..close_index].trim();
        if is_potential_command_placeholder(content) {
            placeholders.push(CommandPlaceholder {
                name: content.to_string(),
                column: columns.at(index),
            });
            index = close_index + 1;
        } else {
            index = next_index;
        }
    }

    placeholders
}

fn is_potential_command_placeholder(content: &str) -> bool {
    !content.is_empty()
        && !content.contains(':')
        && !content.contains(',')
        && !content.contains('{')
        && !content.contains('}')
        && !content.contains('=')
        && !content.contains('"')
        && !content.contains('\'')
}

/// Counts the column of byte offsets that mostly come in order, counting on
/// from the last one.
struct ColumnCounter<'a> {
    line: &'a str,
    last: (usize, usize),
}

impl<'a> ColumnCounter<'a> {
    fn new(line: &'a str) -> Self {
        Self { line, last: (0, 1) }
    }

    fn at(&mut self, byte_index: usize) -> usize {
        let (last_byte, last_column) = self.last;
        let column = if byte_index >= last_byte && self.line.is_char_boundary(byte_index) {
            last_column + self.line[last_byte..byte_index].chars().count()
        } else {
            column_from_byte(self.line, byte_index)
        };
        if self.line.is_char_boundary(byte_index.min(self.line.len())) {
            self.last = (byte_index.min(self.line.len()), column);
        }
        column
    }
}

/// The `}` that closes the `{` at `open_index`, skipping braces in quotes
/// that open after it. Results are kept in `closes`, so a nested brace is
/// matched once however many braces enclose it.
fn matching_brace_quote_aware(
    text: &str,
    open_index: usize,
    closes: &mut HashMap<usize, Option<usize>>,
) -> Option<usize> {
    struct Frame {
        open: usize,
        index: usize,
        quote: Option<char>,
        escaped: bool,
    }
    if let Some(close) = closes.get(&open_index) {
        return *close;
    }
    let mut stack = vec![Frame {
        open: open_index,
        index: open_index + 1,
        quote: None,
        escaped: false,
    }];
    'frames: while let Some(mut frame) = stack.pop() {
        while frame.index < text.len() {
            let ch = text[frame.index..].chars().next().unwrap();
            let next_index = frame.index + ch.len_utf8();
            if let Some(active_quote) = frame.quote {
                if frame.escaped {
                    frame.escaped = false;
                } else if ch == '\\' {
                    frame.escaped = true;
                } else if ch == active_quote {
                    frame.quote = None;
                }
                frame.index = next_index;
                continue;
            }
            match ch {
                '"' | '\'' => frame.quote = Some(ch),
                '{' => match closes.get(&frame.index) {
                    Some(Some(close)) => {
                        frame.index = close + 1;
                        continue;
                    }
                    Some(None) => {
                        closes.insert(frame.open, None);
                        continue 'frames;
                    }
                    None => {
                        // Match the inner brace first, then come back here.
                        let inner = frame.index;
                        stack.push(frame);
                        stack.push(Frame {
                            open: inner,
                            index: inner + 1,
                            quote: None,
                            escaped: false,
                        });
                        continue 'frames;
                    }
                },
                '}' => {
                    closes.insert(frame.open, Some(frame.index));
                    continue 'frames;
                }
                _ => {}
            }
            frame.index = next_index;
        }
        closes.insert(frame.open, None);
    }
    closes.get(&open_index).copied().flatten()
}

fn placeholder_close_before_quote(text: &str, open_index: usize, quote: char) -> Option<usize> {
    let mut index = open_index + 1;
    let mut escaped = false;

    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();
        if escaped {
            escaped = false;
        } else if ch == '\\' {
            escaped = true;
        } else if ch == quote {
            return None;
        } else if ch == '}' {
            return Some(index);
        }
        index = next_index;
    }

    None
}

// ===========================================================================
// Datapack resource-argument shapes (hybrid: statements anchored on the span
// sidecar; argument and resource-id columns from in-statement text scans).
// One pass covers single-line AND bracket-continued multi-line calls — a
// statement span covers the whole call. Offsets are mapped through
// `raw_offset_from_masked` before touching raw text, so non-ASCII strings
// cannot drift columns.
// ===========================================================================

/// Output of [`check_datapack_resource_arguments`], split by statement shape:
/// diagnostics for single-line `datapack.*` calls co-report with the
/// pre-parse per-line loop, while diagnostics for statements spanning
/// several lines are deferred and reported only when nothing else was found
/// (the `is_empty` gate in `analyze_source_stages`).
pub(super) struct DatapackArgumentDiagnostics {
    pub(super) inline: Vec<SourceDiagnostic>,
    pub(super) deferred: Vec<SourceDiagnostic>,
}

/// Validates `datapack.*` helper call shapes (argument counts, JSON object /
/// tag array / replace-boolean shapes) and resource-ID strings, pointing at
/// the offending argument byte — inside string literals for resource-ID
/// character positions. Raw command statements and docstring ranges are
/// skipped.
pub(super) fn check_datapack_resource_arguments(
    context: &SemanticContext<'_>,
) -> DatapackArgumentDiagnostics {
    let mut result = DatapackArgumentDiagnostics {
        inline: Vec::new(),
        deferred: Vec::new(),
    };

    let mut regions: Vec<SourceSpan> = Vec::new();
    context.walk(&mut |statement, node| {
        if matches!(statement, Statement::MinecraftCommand(_)) {
            return;
        }
        let Some(node) = node else {
            return;
        };
        if !context.in_docstring(node.span.line) {
            regions.push(node.span);
        }
        for branch in node
            .elif_branches
            .iter()
            .chain(node.cases.iter())
            .chain(node.else_branch.iter())
        {
            if !context.in_docstring(branch.header.line) {
                regions.push(branch.header);
            }
        }
    });
    regions.sort_by_key(|span| span.start);

    for span in regions {
        let masked = context.masked_statement_slice(span);
        if !masked.contains("datapack.") {
            continue;
        }
        let out = if context.last_line_of_span(span) > span.line {
            &mut result.deferred
        } else {
            &mut result.inline
        };
        check_datapack_calls_in_region(context, span, &masked, out);
    }

    result
}

fn check_datapack_calls_in_region(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    masked: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    let slice = context.statement_slice(span);

    for call in function_calls_in_expression(masked) {
        let Some(helper) = call.name.strip_prefix("datapack.") else {
            continue;
        };

        if let Some(resource_type) = datapack_json_resource_type(helper) {
            if call.arg_count != 2 {
                let (line, column) = region_position(context, span, slice, masked, call.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line,
                        column,
                        format!("datapack.{resource_type}() takes 2 arguments"),
                    )
                    .with_help("Pass a resource name and an object JSON value."),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    context,
                    span,
                    slice,
                    masked,
                    name_arg,
                    "resource name",
                    diagnostics,
                );
            }

            let Some(json_arg) = call.arguments.get(1) else {
                continue;
            };
            let json_text = json_arg.text.trim_start();
            let list_allowed = resource_type == "item_modifier" && json_text.starts_with('[');
            if !json_text.starts_with('{') && !list_allowed {
                let (line, column) = region_position(context, span, slice, masked, json_arg.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line,
                        column,
                        format!("datapack.{resource_type}() JSON value must be an object"),
                    )
                    .with_help("Use an object literal such as `{ \"condition\": \"...\" }`."),
                );
            }
            continue;
        }

        if let Some(tag_type) = datapack_tag_type(helper) {
            if !(2..=3).contains(&call.arg_count) {
                let (line, column) = region_position(context, span, slice, masked, call.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line,
                        column,
                        format!("datapack.{tag_type}_tag() takes 2 or 3 arguments"),
                    )
                    .with_help(
                        "Pass a tag name, an array of resource IDs, and optional replace boolean.",
                    ),
                );
                continue;
            }

            if let Some(name_arg) = call.arguments.first() {
                check_datapack_resource_id_argument(
                    context,
                    span,
                    slice,
                    masked,
                    name_arg,
                    "tag name",
                    diagnostics,
                );
            }

            let Some(values_arg) = call.arguments.get(1) else {
                continue;
            };
            if !values_arg.text.trim_start().starts_with('[') {
                let (line, column) =
                    region_position(context, span, slice, masked, values_arg.offset);
                diagnostics.push(
                    SourceDiagnostic::error(
                        "datapack-resource-argument",
                        line,
                        column,
                        "Tag values must be an array",
                    )
                    .with_help("Use an array literal such as `[\"minecraft:stone\"]`."),
                );
            } else {
                check_datapack_tag_value_resource_ids(
                    context,
                    span,
                    slice,
                    masked,
                    values_arg,
                    diagnostics,
                );
            }

            if let Some(replace_arg) = call.arguments.get(2) {
                let replace = replace_arg.text.trim();
                if replace != "True" && replace != "False" {
                    let (line, column) =
                        region_position(context, span, slice, masked, replace_arg.offset);
                    diagnostics.push(
                        SourceDiagnostic::error(
                            "datapack-resource-argument",
                            line,
                            column,
                            "Tag replace must be a literal boolean",
                        )
                        .with_help("Use `True` to replace vanilla tag values or `False` to preserve the explicit setting."),
                    );
                }
            }
        }
    }
}

/// 1-based (line, column) of a masked in-statement byte offset, mapped back
/// to the raw source before positioning so masked/raw byte drift (non-ASCII
/// string literals earlier in the statement) cannot skew columns.
fn region_position(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    slice: &str,
    masked: &str,
    masked_offset: usize,
) -> (usize, usize) {
    context.position_at(span.start + raw_offset_from_masked(slice, masked, masked_offset))
}

fn check_datapack_resource_id_argument(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    slice: &str,
    masked: &str,
    argument: &FunctionCallArgumentSpan,
    label: &str,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // `argument.offset` is a byte offset into the masked statement slice;
    // map it to the raw slice before reading the literal or reporting.
    let raw_offset = raw_offset_from_masked(slice, masked, argument.offset);
    let Some(value) = quoted_string_literal_at(slice, raw_offset) else {
        return;
    };
    let Some(diagnostic) = validate_datapack_resource_id(label, value) else {
        return;
    };

    // +1 skips the opening quote byte; `diagnostic.offset` is a byte offset
    // inside the raw string literal.
    let (line, column) = context.position_at(span.start + raw_offset + 1 + diagnostic.offset);
    diagnostics.push(
        SourceDiagnostic::error("datapack-resource-id", line, column, diagnostic.message)
            .with_help(diagnostic.help),
    );
}

fn check_datapack_tag_value_resource_ids(
    context: &SemanticContext<'_>,
    span: SourceSpan,
    slice: &str,
    masked: &str,
    argument: &FunctionCallArgumentSpan,
    diagnostics: &mut Vec<SourceDiagnostic>,
) {
    // Map both ends of the masked argument span to raw byte offsets so the
    // raw sub-slice and every reported column stay drift-free; item offsets
    // below are raw offsets within `raw_argument`.
    let raw_offset = raw_offset_from_masked(slice, masked, argument.offset);
    let raw_end = raw_offset_from_masked(slice, masked, argument.offset + argument.text.len());
    let raw_argument = slice.get(raw_offset..raw_end).unwrap_or(&argument.text);

    for item in array_literal_item_spans(raw_argument) {
        if item.text.starts_with('{') {
            for diagnostic in datapack_tag_object_diagnostics(&item.text) {
                let (line, column) =
                    context.position_at(span.start + raw_offset + item.offset + diagnostic.offset);
                diagnostics.push(
                    SourceDiagnostic::error(diagnostic.kind, line, column, diagnostic.message)
                        .with_help(diagnostic.help),
                );
            }
            continue;
        }

        if !item.text.starts_with('"') && !item.text.starts_with('\'') {
            let (line, column) = context.position_at(span.start + raw_offset + item.offset);
            diagnostics.push(
                SourceDiagnostic::error(
                    "datapack-resource-argument",
                    line,
                    column,
                    "Tag values must be string resource IDs or object entries",
                )
                .with_help(
                    "Use string values such as `[\"minecraft:stone\"]` or object entries such as `[{\"id\": \"minecraft:stone\", \"required\": False}]`.",
                ),
            );
            continue;
        }

        let Some(value) = quoted_string_literal_at(raw_argument, item.offset) else {
            continue;
        };
        let (value, tag_offset) = strip_tag_reference(value);
        let Some(diagnostic) = validate_datapack_resource_id("tag value", value) else {
            continue;
        };
        let (line, column) = context.position_at(
            span.start + raw_offset + item.offset + 1 + tag_offset + diagnostic.offset,
        );
        diagnostics.push(
            SourceDiagnostic::error("datapack-resource-id", line, column, diagnostic.message)
                .with_help(diagnostic.help),
        );
    }
}

// --- Datapack validation: resource-ID and tag-object rules. Offsets they
// return are byte offsets inside the raw text they were handed; the callers
// above anchor them.

struct ResourceIdDiagnostic {
    message: String,
    help: String,
    offset: usize,
}

/// Tag values may reference another tag of the same type (`#minecraft:logs`);
/// returns the ID without the `#` and how many bytes were stripped.
fn strip_tag_reference(value: &str) -> (&str, usize) {
    value.strip_prefix('#').map_or((value, 0), |rest| (rest, 1))
}

fn validate_datapack_resource_id(label: &str, id: &str) -> Option<ResourceIdDiagnostic> {
    let (namespace, path, path_offset) = if let Some((namespace, path)) = id.split_once(':') {
        if let Some(extra_colon) = path.find(':') {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': resource IDs may contain at most one ':' separator"
                ),
                help: "Use one optional namespace separator, for example `minecraft:load`."
                    .to_string(),
                offset: namespace.len() + 1 + extra_colon,
            });
        }
        (Some(namespace), path, namespace.len() + 1)
    } else {
        (None, id, 0)
    };

    if let Some(namespace) = namespace {
        let invalid_namespace = namespace.is_empty()
            || namespace.chars().any(|c| {
                !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_' || c == '-' || c == '.')
            });
        if invalid_namespace {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': use lowercase namespace:path resource IDs"
                ),
                help: "Namespaces may contain lowercase letters, digits, `_`, `-`, or `.`."
                    .to_string(),
                offset: 0,
            });
        }
    } else if let Some((prefix, suffix)) = path.split_once('/') {
        if prefix == "minecraft" {
            return Some(ResourceIdDiagnostic {
                message: format!(
                    "Invalid {label} '{id}': '{prefix}' looks like a namespace. Use '{prefix}:{suffix}' instead of a slash-separated namespace prefix."
                ),
                help: "Use `namespace:path` for explicit namespaces, not `namespace/path`."
                    .to_string(),
                offset: prefix.len(),
            });
        }
    }

    if let Some((index, c)) = path.char_indices().find(|(_, c)| c.is_ascii_uppercase()) {
        return Some(ResourceIdDiagnostic {
            message: format!(
                "Invalid {label} '{id}': uppercase character '{c}' at position {}. Use lowercase resource paths or namespace:path IDs.",
                index + 1
            ),
            help: "Use lowercase resource IDs such as `minecraft:load` or `checks/ready`."
                .to_string(),
            offset: path_offset + index,
        });
    }

    if let Some(index) = path.find('\\') {
        return Some(ResourceIdDiagnostic {
            message: format!("Invalid {label} '{id}': use '/' path separators, not '\\'"),
            help: "Resource paths use forward slashes, for example `story/root`.".to_string(),
            offset: path_offset + index,
        });
    }

    let invalid_segment = path
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..");
    let invalid_char = path.chars().any(|c| {
        !(c.is_ascii_lowercase()
            || c.is_ascii_digit()
            || c == '_'
            || c == '-'
            || c == '.'
            || c == '/')
    });
    if path.is_empty() || invalid_segment || invalid_char {
        return Some(ResourceIdDiagnostic {
            message: format!(
                "Invalid {label} '{id}': use lowercase resource paths or namespace:path IDs with letters, digits, '/', '_', '-', or '.', and no empty, '.', or '..' segments"
            ),
            help: "Use a relative path such as `checks/ready` or an explicit ID such as `minecraft:load`."
                .to_string(),
            offset: path_offset,
        });
    }

    None
}

struct TagObjectDiagnostic {
    kind: &'static str,
    message: String,
    help: String,
    offset: usize,
}

fn datapack_tag_object_diagnostics(text: &str) -> Vec<TagObjectDiagnostic> {
    let Some(Expression::Map(entries)) = parse_diagnostic_expression(text) else {
        return vec![TagObjectDiagnostic {
            kind: "datapack-resource-argument",
            message: "Tag object values must be valid object entries".to_string(),
            help:
                "Use object entries such as `{\"id\": \"minecraft:stone\", \"required\": False}`."
                    .to_string(),
            offset: 0,
        }];
    };

    let mut diagnostics = Vec::new();
    let mut id = None;
    let mut required = None;

    for (key, value) in &entries {
        match key.as_str() {
            "id" => id = Some(value),
            "required" => required = Some(value),
            _ => diagnostics.push(TagObjectDiagnostic {
                kind: "datapack-resource-argument",
                message: "Tag object values may only contain \"id\" and \"required\" fields"
                    .to_string(),
                help:
                    "Remove extra fields or use a string tag entry such as `\"minecraft:stone\"`."
                        .to_string(),
                offset: object_field_key_offset(text, key).unwrap_or(0),
            }),
        }
    }

    match id {
        Some(Expression::String(value)) => {
            let (value, tag_offset) = strip_tag_reference(value);
            if let Some(diagnostic) = validate_datapack_resource_id("tag value", value) {
                let value_offset = object_field_value_offset(text, "id").unwrap_or(0);
                let quote_offset = text
                    .get(value_offset..)
                    .and_then(|rest| rest.chars().next())
                    .filter(|quote| matches!(quote, '"' | '\''))
                    .map_or(0, char::len_utf8);
                diagnostics.push(TagObjectDiagnostic {
                    kind: "datapack-resource-id",
                    message: diagnostic.message,
                    help: diagnostic.help,
                    offset: value_offset + quote_offset + tag_offset + diagnostic.offset,
                });
            }
        }
        Some(_) | None => diagnostics.push(TagObjectDiagnostic {
            kind: "datapack-resource-argument",
            message: "Tag object values must include a string \"id\" resource ID".to_string(),
            help:
                "Use object entries such as `{\"id\": \"minecraft:stone\", \"required\": False}`."
                    .to_string(),
            offset: object_field_value_offset(text, "id").unwrap_or(0),
        }),
    }

    if let Some(value) = required {
        if !matches!(value, Expression::Boolean(_)) {
            diagnostics.push(TagObjectDiagnostic {
                kind: "datapack-resource-argument",
                message: "Tag object value \"required\" must be a boolean".to_string(),
                help: "Use `required: True` or `required: False`.".to_string(),
                offset: object_field_value_offset(text, "required").unwrap_or(0),
            });
        }
    }

    diagnostics
}

fn parse_diagnostic_expression(text: &str) -> Option<Expression> {
    let source = format!("__cobble_diagnostic_value = {}\n", text);
    let program = crate::parser::parse(&source).ok()?;
    match program.statements.into_iter().next()? {
        Statement::Assignment(assignment) => Some(assignment.value),
        Statement::Expression(expression) => Some(expression),
        _ => None,
    }
}

fn object_field_key_offset(text: &str, key: &str) -> Option<usize> {
    for pattern in [
        format!("\"{}\"", key),
        format!("'{}'", key),
        key.to_string(),
    ] {
        if let Some(offset) = text.find(&pattern) {
            return Some(offset);
        }
    }
    None
}

fn object_field_value_offset(text: &str, key: &str) -> Option<usize> {
    let key_offset = object_field_key_offset(text, key)?;
    let colon_offset = text.get(key_offset..)?.find(':')? + key_offset;
    let value_start = colon_offset + ':'.len_utf8();
    let whitespace = text
        .get(value_start..)?
        .chars()
        .take_while(|ch| ch.is_whitespace())
        .map(char::len_utf8)
        .sum::<usize>();
    Some(value_start + whitespace)
}

fn quoted_string_literal_at(text: &str, offset: usize) -> Option<&str> {
    let quote = text.get(offset..)?.chars().next()?;
    if !matches!(quote, '"' | '\'') {
        return None;
    }

    let value_start = offset + quote.len_utf8();
    let end = find_string_end(text, quote, value_start)?;
    Some(&text[value_start..end - quote.len_utf8()])
}

fn array_literal_item_spans(text: &str) -> Vec<ArgumentSpan> {
    let text = &blank_comments(text);
    let mut result = Vec::new();
    let mut start = None;
    let mut index = 0usize;
    let mut depth = 0usize;
    let mut quote = None;
    let mut escaped = false;

    while index < text.len() {
        let ch = text[index..].chars().next().unwrap();
        let next_index = index + ch.len_utf8();

        if let Some(active_quote) = quote {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == active_quote {
                quote = None;
            }
            index = next_index;
            continue;
        }

        match ch {
            '"' | '\'' => {
                quote = Some(ch);
                if depth == 1 && start.is_none() {
                    start = Some(index);
                }
                index = next_index;
            }
            '[' => {
                depth += 1;
                if depth == 1 {
                    start = Some(next_index);
                }
                index = next_index;
            }
            ']' => {
                if depth == 1 {
                    if let Some(start_index) = start {
                        push_array_item_span(text, start_index, index, &mut result);
                    }
                    start = None;
                }
                depth = depth.saturating_sub(1);
                index = next_index;
            }
            ',' if depth == 1 => {
                if let Some(start_index) = start {
                    push_array_item_span(text, start_index, index, &mut result);
                }
                start = Some(next_index);
                index = next_index;
            }
            '(' | '{' if depth >= 1 => {
                if depth == 1 && start.is_none() {
                    start = Some(index);
                }
                depth += 1;
                index = next_index;
            }
            ')' | '}' if depth > 1 => {
                depth = depth.saturating_sub(1);
                index = next_index;
            }
            _ => {
                if depth == 1 && start.is_none() && !ch.is_whitespace() {
                    start = Some(index);
                }
                index = next_index;
            }
        }
    }

    result
}

fn push_array_item_span(text: &str, start: usize, end: usize, result: &mut Vec<ArgumentSpan>) {
    if start > end || end > text.len() {
        return;
    }
    let span = argument_span(text, start, end);
    if !span.text.is_empty() {
        result.push(span);
    }
}

#[cfg(test)]
mod brace_matching_tests {
    use super::*;

    /// The matcher before it kept results: one scan from the brace.
    fn reference(text: &str, open_index: usize) -> Option<usize> {
        let mut depth = 0usize;
        let mut index = open_index;
        let mut quote = None;
        let mut escaped = false;
        while index < text.len() {
            let ch = text[index..].chars().next().unwrap();
            let next_index = index + ch.len_utf8();
            if let Some(active_quote) = quote {
                if escaped {
                    escaped = false;
                } else if ch == '\\' {
                    escaped = true;
                } else if ch == active_quote {
                    quote = None;
                }
                index = next_index;
                continue;
            }
            match ch {
                '"' | '\'' => quote = Some(ch),
                '{' => depth += 1,
                '}' => {
                    depth = depth.saturating_sub(1);
                    if depth == 0 {
                        return Some(index);
                    }
                }
                _ => {}
            }
            index = next_index;
        }
        None
    }

    #[test]
    fn kept_matches_agree_with_a_scan_from_each_brace() {
        let alphabet = ['{', '}', '"', '\'', '\\', 'a', ':', 'é'];
        let mut seed = 0x2545_f491_4f6c_dd1du64;
        for _ in 0..3000 {
            let mut text = String::new();
            seed ^= seed << 13;
            seed ^= seed >> 7;
            seed ^= seed << 17;
            let len = (seed % 24) as usize;
            for _ in 0..len {
                seed ^= seed << 13;
                seed ^= seed >> 7;
                seed ^= seed << 17;
                text.push(alphabet[(seed % alphabet.len() as u64) as usize]);
            }
            let mut closes = HashMap::new();
            for (index, ch) in text.char_indices() {
                if ch == '{' {
                    assert_eq!(
                        matching_brace_quote_aware(&text, index, &mut closes),
                        reference(&text, index),
                        "{text:?} at {index}"
                    );
                }
            }
        }
    }

    #[test]
    fn columns_count_characters_from_the_line_start() {
        let line = "/say é {a} 안녕 {b}";
        let mut columns = ColumnCounter::new(line);
        for (index, _) in line.char_indices() {
            assert_eq!(columns.at(index), column_from_byte(line, index), "{index}");
        }
        assert_eq!(columns.at(0), 1);
    }
}
