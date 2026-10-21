//! Alignment contract for the AST-shaped statement span tree
//! (`ProgramSpanTable::statement_tree`, built by
//! `parse_located_with_diagnostics`).
//!
//! The tree must mirror `Program.statements` exactly — one node per AST
//! statement, `elif`/`else` folded into `If`, `case` folded into `Match` — so
//! a semantic checker can walk program and spans in lockstep. The property
//! sweep below enforces that over every fixture that parses; the targeted
//! tests pin the merge rules and exact spans (byte offsets and line/column,
//! including CRLF and multi-byte sources).

use cobble::ast::SourceSpan;
use cobble::parser::{
    parse_located_with_diagnostics, verify_span_tree_alignment, LocatedProgram, StatementSpanKind,
    StatementSpanNode, StatementSpanTree,
};
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Diagnostics-migration corpus (session workspace). The vendored subset in
/// `tests/fixtures/span_corpus`, the in-repo `examples/` tree, and the
/// `tests/fixtures/projects` QA fixtures are always swept, so the sweep stays meaningful when this directory is absent;
/// override with `COBBLE_SPAN_TREE_CORPUS` to sweep a relocated full corpus
/// in addition.
const DIAGMIG_CORPUS_DIR: &str = "/tmp/claude-1000/-home-world-coding-cobble/9633a053-dd4f-446b-bea5-ef21aa9633ec/scratchpad/diagmig/corpus";

/// Vendored, always-present subset of the diagnostics-migration corpus:
/// plain `.cbl` sources covering every [`StatementSpanKind`] and the tricky
/// shapes (if/elif/else, match cases, multiline datapack calls, CRLF,
/// Korean text, docstrings, `define ... create ... end` entity blocks, deep
/// nesting, top-level and nested imports).
fn vendored_span_corpus() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("span_corpus")
}

fn collect_cbl_files(dir: &Path, files: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_cbl_files(&path, files);
        } else if path.extension().is_some_and(|extension| extension == "cbl") {
            files.push(path);
        }
    }
}

fn parse_tree(source: &str) -> (LocatedProgram, StatementSpanTree) {
    let located = parse_located_with_diagnostics(source).expect("source must parse");
    let tree = located
        .statement_spans()
        .expect("aligned statement span tree must be built")
        .clone();
    verify_span_tree_alignment(&located.program, &tree).expect("span tree must align");
    (located, tree)
}

fn kinds(nodes: &[StatementSpanNode]) -> Vec<StatementSpanKind> {
    nodes.iter().map(|node| node.kind).collect()
}

fn tally_kinds(
    nodes: &[StatementSpanNode],
    kinds: &mut BTreeSet<&'static str>,
    branches: &mut BTreeSet<&'static str>,
) {
    for node in nodes {
        kinds.insert(kind_name(node.kind));
        if !node.elif_branches.is_empty() {
            branches.insert("elif");
        }
        if node.else_branch.is_some() {
            branches.insert("else");
        }
        if !node.cases.is_empty() {
            branches.insert("case");
        }
        tally_kinds(&node.children, kinds, branches);
        for branch in node
            .elif_branches
            .iter()
            .chain(node.else_branch.iter())
            .chain(node.cases.iter())
        {
            tally_kinds(&branch.children, kinds, branches);
        }
    }
}

fn kind_name(kind: StatementSpanKind) -> &'static str {
    match kind {
        StatementSpanKind::Import => "Import",
        StatementSpanKind::FunctionDef => "FunctionDef",
        StatementSpanKind::Assignment => "Assignment",
        StatementSpanKind::ConstAssignment => "ConstAssignment",
        StatementSpanKind::Expression => "Expression",
        StatementSpanKind::If => "If",
        StatementSpanKind::For => "For",
        StatementSpanKind::While => "While",
        StatementSpanKind::Match => "Match",
        StatementSpanKind::Return => "Return",
        StatementSpanKind::Pass => "Pass",
        StatementSpanKind::MinecraftCommand => "MinecraftCommand",
        StatementSpanKind::Global => "Global",
        StatementSpanKind::Execute => "Execute",
        StatementSpanKind::SelectorDef => "SelectorDef",
        StatementSpanKind::EntityDef => "EntityDef",
        StatementSpanKind::CreateEntity => "CreateEntity",
    }
}

const ALL_KINDS: &[&str] = &[
    "Import",
    "FunctionDef",
    "Assignment",
    "ConstAssignment",
    "Expression",
    "If",
    "For",
    "While",
    "Match",
    "Return",
    "Pass",
    "MinecraftCommand",
    "Global",
    "Execute",
    "SelectorDef",
    "EntityDef",
    "CreateEntity",
];

#[test]
fn every_parseable_fixture_yields_an_aligned_statement_span_tree() {
    // Always swept: the vendored corpus subset, the in-repo examples, and
    // the QA fixture projects.
    let vendored = vendored_span_corpus();
    let mut files = Vec::new();
    collect_cbl_files(&vendored, &mut files);
    let vendored_count = files.len();
    assert!(
        vendored_count >= 40,
        "vendored span corpus at {} holds only {vendored_count} .cbl fixtures",
        vendored.display()
    );
    let repo_root = Path::new(env!("CARGO_MANIFEST_DIR"));
    collect_cbl_files(&repo_root.join("examples"), &mut files);
    collect_cbl_files(&repo_root.join("tests/fixtures/projects"), &mut files);
    // Swept in addition when present: the full session corpus (or any
    // directory named by COBBLE_SPAN_TREE_CORPUS).
    let corpus = std::env::var("COBBLE_SPAN_TREE_CORPUS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from(DIAGMIG_CORPUS_DIR));
    collect_cbl_files(&corpus, &mut files);
    files.sort();
    files.dedup();

    let mut parsed = 0usize;
    let mut vendored_parsed = 0usize;
    let mut vendored_kinds: BTreeSet<&'static str> = BTreeSet::new();
    let mut vendored_branches: BTreeSet<&'static str> = BTreeSet::new();
    for path in &files {
        // Some corpus fixtures are deliberately unreadable (import-read
        // error fixtures); the tree contract only applies to readable files.
        let Ok(source) = fs::read_to_string(path) else {
            continue;
        };
        let Ok(located) = parse_located_with_diagnostics(&source) else {
            // Negative fixtures are expected to fail the parse; the tree
            // contract only applies to successful parses.
            continue;
        };
        parsed += 1;
        let tree = located.statement_spans().unwrap_or_else(|| {
            panic!(
                "{}: parse succeeded but no aligned statement span tree was built",
                path.display()
            )
        });
        if let Err(error) = verify_span_tree_alignment(&located.program, tree) {
            panic!("{}: span tree misaligned: {error}", path.display());
        }
        if path.starts_with(&vendored) {
            vendored_parsed += 1;
            tally_kinds(
                &tree.statements,
                &mut vendored_kinds,
                &mut vendored_branches,
            );
        }
    }
    eprintln!(
        "span-tree sweep: {parsed} parsed / {} fixtures scanned ({vendored_parsed} vendored)",
        files.len()
    );
    assert!(
        parsed >= 20,
        "sweep parsed only {parsed} of {} fixtures; fixture set looks broken",
        files.len()
    );
    assert!(
        vendored_parsed >= 40,
        "only {vendored_parsed} vendored fixtures parsed; the vendored set must stay parseable"
    );

    // The vendored set must keep covering every statement kind and every
    // branch flavor, so the sweep retains its regression power without the
    // session corpus.
    let missing: Vec<_> = ALL_KINDS
        .iter()
        .filter(|kind| !vendored_kinds.contains(**kind))
        .collect();
    assert!(
        missing.is_empty(),
        "vendored span corpus no longer covers statement kinds: {missing:?}"
    );
    for branch in ["elif", "else", "case"] {
        assert!(
            vendored_branches.contains(branch),
            "vendored span corpus no longer covers a `{branch}` branch"
        );
    }
}

#[test]
fn if_elif_elif_else_folds_into_one_node() {
    let source = "\
def main():
    if x == 1:
        /say one
    elif x == 2:
        /say two
    elif x == 3:
        if y:
            pass
    else:
        /say other
";
    let (located, tree) = parse_tree(source);
    assert_eq!(located.program.statements.len(), 1);
    assert_eq!(
        kinds(&tree.statements),
        vec![StatementSpanKind::FunctionDef]
    );

    let function = &tree.statements[0];
    assert_eq!(kinds(&function.children), vec![StatementSpanKind::If]);

    let if_node = &function.children[0];
    assert_eq!((if_node.span.line, if_node.span.column), (2, 5));
    assert_eq!(if_node.end_line, 2);
    assert_eq!(
        kinds(&if_node.children),
        vec![StatementSpanKind::MinecraftCommand]
    );

    assert_eq!(if_node.elif_branches.len(), 2);
    let first_elif = &if_node.elif_branches[0];
    assert_eq!((first_elif.header.line, first_elif.header.column), (4, 5));
    assert_eq!(
        kinds(&first_elif.children),
        vec![StatementSpanKind::MinecraftCommand]
    );
    let second_elif = &if_node.elif_branches[1];
    assert_eq!(second_elif.header.line, 6);
    assert_eq!(kinds(&second_elif.children), vec![StatementSpanKind::If]);
    let nested_if = &second_elif.children[0];
    assert_eq!((nested_if.span.line, nested_if.span.column), (7, 9));
    assert_eq!(kinds(&nested_if.children), vec![StatementSpanKind::Pass]);
    assert!(nested_if.elif_branches.is_empty());
    assert!(nested_if.else_branch.is_none());

    let else_branch = if_node.else_branch.as_ref().expect("else branch");
    assert_eq!((else_branch.header.line, else_branch.header.column), (9, 5));
    assert_eq!(
        kinds(&else_branch.children),
        vec![StatementSpanKind::MinecraftCommand]
    );
}

#[test]
fn match_cases_fold_into_one_node() {
    let source = "\
def main():
    match score:
        case 1:
            /say a
        case 2 to 5:
            /say b
        case 7:
            pass
        case _:
            /say d
";
    let (located, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(kinds(&function.children), vec![StatementSpanKind::Match]);

    let match_node = &function.children[0];
    assert_eq!((match_node.span.line, match_node.span.column), (2, 5));
    assert!(match_node.children.is_empty());
    assert_eq!(match_node.cases.len(), 4);
    for (index, expected_line) in [3usize, 5, 7, 9].into_iter().enumerate() {
        let case = &match_node.cases[index];
        assert_eq!((case.header.line, case.header.column), (expected_line, 9));
        assert_eq!(case.children.len(), 1);
    }
    if let cobble::ast::Statement::FunctionDef(function) = &located.program.statements[0] {
        if let cobble::ast::Statement::Match(match_statement) = &function.body[0] {
            assert_eq!(match_statement.cases.len(), 4);
        } else {
            panic!("expected match statement");
        }
    } else {
        panic!("expected function def");
    }
}

#[test]
fn execute_blocks_are_distinguished_from_if_statements() {
    let source = "\
def main():
    if block ~ ~-1 ~ minecraft:stone:
        /say on stone
    if x:
        /say cond
    asat @s:
        /say here
    as @a at @s:
        /particle flame
";
    let (_, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(
        kinds(&function.children),
        vec![
            StatementSpanKind::Execute,
            StatementSpanKind::If,
            StatementSpanKind::Execute,
            StatementSpanKind::Execute,
        ]
    );
    for child in &function.children {
        assert_eq!(child.children.len(), 1);
        assert!(child.elif_branches.is_empty());
        assert!(child.else_branch.is_none());
    }
}

#[test]
fn nested_bodies_keep_lockstep_shape() {
    let source = "\
def main():
    for i in range(3):
        while i < 2:
            if i:
                asat @s:
                    /say deep
            else:
                pass
";
    let (_, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(kinds(&function.children), vec![StatementSpanKind::For]);
    let for_node = &function.children[0];
    assert_eq!(kinds(&for_node.children), vec![StatementSpanKind::While]);
    let while_node = &for_node.children[0];
    assert_eq!(kinds(&while_node.children), vec![StatementSpanKind::If]);
    let if_node = &while_node.children[0];
    assert_eq!(kinds(&if_node.children), vec![StatementSpanKind::Execute]);
    assert_eq!(
        kinds(&if_node.children[0].children),
        vec![StatementSpanKind::MinecraftCommand]
    );
    let else_branch = if_node.else_branch.as_ref().expect("else branch");
    assert_eq!(kinds(&else_branch.children), vec![StatementSpanKind::Pass]);
    assert_eq!(if_node.children[0].children[0].span.line, 6);
}

#[test]
fn multiline_datapack_call_is_one_header() {
    let source = "\
import stdlib

datapack.predicate(\"gameplay/lucky\", {
    \"condition\": \"minecraft:random_chance\",
    \"chance\": 0.5
})
";
    let (located, tree) = parse_tree(source);
    // The top-level import is diverted to `spans.imports`, mirroring how
    // `Program.statements` excludes top-level imports.
    assert_eq!(located.program.imports.len(), 1);
    assert_eq!(located.spans.imports.len(), 1);
    assert_eq!(kinds(&tree.statements), vec![StatementSpanKind::Expression]);

    let call = &tree.statements[0];
    let start = source.find("datapack").unwrap();
    let end = source.rfind(')').unwrap() + 1;
    assert_eq!(call.span, SourceSpan::new(start, end, 3, 1));
    assert_eq!(call.end_line, 6);
}

#[test]
fn crlf_sources_keep_exact_byte_offsets() {
    let source = "def main():\r\n    total = 1 + 2\r\n";
    let (_, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(function.span, SourceSpan::new(0, 11, 1, 1));
    assert_eq!(function.end_line, 1);
    assert_eq!(
        kinds(&function.children),
        vec![StatementSpanKind::Assignment]
    );
    assert_eq!(function.children[0].span, SourceSpan::new(17, 30, 2, 5));
    assert_eq!(function.children[0].end_line, 2);
}

#[test]
fn korean_strings_and_comments_keep_byte_offsets_and_char_columns() {
    let source = "greeting = \"안녕하세요\"\nif greeting == \"인사\":\n    /say 안녕\n# 한국어 주석\ncount = 1\n";
    let (_, tree) = parse_tree(source);
    assert_eq!(
        kinds(&tree.statements),
        vec![
            StatementSpanKind::Assignment,
            StatementSpanKind::If,
            StatementSpanKind::Assignment,
        ]
    );

    // `안녕하세요` is 15 bytes / 5 chars: byte offsets and 1-based char
    // columns must diverge exactly as the tokenizer records them.
    assert_eq!(tree.statements[0].span, SourceSpan::new(0, 28, 1, 1));
    assert_eq!(tree.statements[0].end_line, 1);

    let if_node = &tree.statements[1];
    assert_eq!(if_node.span, SourceSpan::new(29, 53, 2, 1));
    assert_eq!(if_node.end_line, 2);
    assert_eq!(
        kinds(&if_node.children),
        vec![StatementSpanKind::MinecraftCommand]
    );
    assert_eq!(if_node.children[0].span, SourceSpan::new(58, 69, 3, 5));

    let count = &tree.statements[2];
    let start = source.find("count").unwrap();
    assert_eq!(count.span, SourceSpan::new(start, start + 9, 5, 1));
}

#[test]
fn single_line_docstring_yields_one_pass_node() {
    // `"""Spawn the boss"""` is one string token, so the grammar turns it
    // into a single `Statement::Pass` spanning the whole literal.
    let source = "def spawn():\n    \"\"\"Spawn the boss\"\"\"\n    /say hi\n";
    let (located, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(
        kinds(&function.children),
        vec![StatementSpanKind::Pass, StatementSpanKind::MinecraftCommand]
    );
    assert_eq!(function.children[0].span, SourceSpan::new(17, 37, 2, 5));
    if let cobble::ast::Statement::FunctionDef(function) = &located.program.statements[0] {
        assert_eq!(function.body.len(), 2);
    } else {
        panic!("expected function def");
    }
}

#[test]
fn multi_line_docstring_bodies_stay_aligned() {
    // The whole multi-line literal is one token: its body lines are not
    // statements, and the next statement keeps its own source line.
    let source = "\
def helper():
    \"\"\"
    Multi line docs
    \"\"\"
    pass
";
    let (_, tree) = parse_tree(source);
    let function = &tree.statements[0];
    assert_eq!(
        kinds(&function.children),
        vec![StatementSpanKind::Pass, StatementSpanKind::Pass]
    );
    assert_eq!(function.children[0].span, SourceSpan::new(18, 49, 2, 5));
    assert_eq!(function.children[0].end_line, 4);
    assert_eq!(
        (
            function.children[1].span.line,
            function.children[1].span.column
        ),
        (5, 5)
    );
}

#[test]
fn entity_define_create_end_is_one_leaf_node() {
    let source = "\
define @Boss = @e[type=wither]
create {name: \"Boss\"}
end
create @Boss
";
    let (_, tree) = parse_tree(source);
    assert_eq!(
        kinds(&tree.statements),
        vec![
            StatementSpanKind::EntityDef,
            StatementSpanKind::CreateEntity,
        ]
    );
    let entity = &tree.statements[0];
    let end = source.find("\nend").unwrap() + 4;
    assert_eq!(entity.span, SourceSpan::new(0, end, 1, 1));
    assert_eq!(entity.end_line, 3);
    assert!(entity.children.is_empty());

    let create = &tree.statements[1];
    assert_eq!((create.span.line, create.span.column), (4, 1));
}

#[test]
fn simple_statement_kinds_and_nested_imports_align() {
    let source = "\
import stdlib

@Guards = @a[team=guards]
const LIMIT = 10

def main():
    import helper_module
    global counter
    counter = LIMIT
    counter += 2
    scores[\"a\"] = 1
    return counter
";
    let (located, tree) = parse_tree(source);
    // Top-level import diverted; nested import stays a statement.
    assert_eq!(located.program.imports.len(), 1);
    assert_eq!(located.spans.imports.len(), 1);
    assert_eq!(
        kinds(&tree.statements),
        vec![
            StatementSpanKind::SelectorDef,
            StatementSpanKind::ConstAssignment,
            StatementSpanKind::FunctionDef,
        ]
    );
    let function = &tree.statements[2];
    assert_eq!(
        kinds(&function.children),
        vec![
            StatementSpanKind::Import,
            StatementSpanKind::Global,
            StatementSpanKind::Assignment,
            StatementSpanKind::Assignment,
            StatementSpanKind::Assignment,
            StatementSpanKind::Return,
        ]
    );
    for (node, line) in function.children.iter().zip(7usize..) {
        assert_eq!(node.span.line, line);
        assert_eq!(node.span.column, 5);
        assert_eq!(node.end_line, line);
    }
}

#[test]
fn verifier_rejects_shape_mismatches() {
    let source = "\
def main():
    if x:
        pass
    else:
        pass
";
    let (located, tree) = parse_tree(source);

    // Dropping the else branch must be caught.
    let mut broken = tree.clone();
    broken.statements[0].children[0].else_branch = None;
    let error = verify_span_tree_alignment(&located.program, &broken).unwrap_err();
    assert!(error.contains("else branch"), "unexpected error: {error}");

    // A wrong variant tag must be caught.
    let mut broken = tree.clone();
    broken.statements[0].children[0].kind = StatementSpanKind::Execute;
    let error = verify_span_tree_alignment(&located.program, &broken).unwrap_err();
    assert!(error.contains("Execute"), "unexpected error: {error}");

    // A missing nested statement must be caught.
    let mut broken = tree;
    broken.statements[0].children[0].children.clear();
    assert!(verify_span_tree_alignment(&located.program, &broken).is_err());
}
