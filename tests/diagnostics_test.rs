//! Regression tests for diagnostics fixes:
//! - column drift when non-ASCII text precedes a finding on the same line
//!   (columns must be computed against the same text the byte index came from)
//! - `return`-line function calls now get arity/undefined-function checks
//! - `def` name column computed from the `def ` prefix, not `trimmed.find(name)`
//! - canaries pinning each consolidated keyword skip list's effective set
//! - a vendored diagnostic corpus (`tests/fixtures/diag_corpus`) pinning the
//!   exact kind@line:column output of the migrated AST-anchored engine

use cobble::diagnostics::{analyze_source, SourceDiagnostic};
use std::path::Path;

fn find_kind<'a>(diagnostics: &'a [SourceDiagnostic], kind: &str) -> Option<&'a SourceDiagnostic> {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == kind)
}

fn expect_kind<'a>(diagnostics: &'a [SourceDiagnostic], kind: &str) -> &'a SourceDiagnostic {
    find_kind(diagnostics, kind).unwrap_or_else(|| {
        panic!("expected a `{kind}` diagnostic, got: {diagnostics:?}");
    })
}

fn assert_no_kind(diagnostics: &[SourceDiagnostic], kind: &str) {
    assert!(
        find_kind(diagnostics, kind).is_none(),
        "expected no `{kind}` diagnostic, got: {diagnostics:?}"
    );
}

#[test]
fn argument_count_column_counts_chars_after_non_ascii_string() {
    // The Korean string is 6 bytes but 2 chars; the second `f(...)` call must
    // still be reported at its char column, not a byte-drifted one.
    let source = "def f(a):\n    /say {a}\n\ndef main():\n    t = f(\"안녕\") + f(1, 2)\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "function-argument-count");
    assert!(
        diagnostic
            .message
            .contains("Function `f` expects 1 argument(s), but 2 provided"),
        "unexpected message: {}",
        diagnostic.message
    );
    assert_eq!((diagnostic.line, diagnostic.column), (5, 19));
}

#[test]
fn comprehension_column_counts_chars_after_non_ascii_string() {
    // Also guards against slicing the raw line at a masked byte index that is
    // not a char boundary (which used to panic).
    let source = "def main():\n    y = [\"안녕\" for k in items]\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "unsupported-comprehension");
    assert_eq!((diagnostic.line, diagnostic.column), (2, 15));
}

#[test]
fn return_line_reports_user_function_argument_count() {
    let source = "def f(a):\n    /say {a}\n\ndef main():\n    return f(1, 2)\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "function-argument-count");
    assert!(
        diagnostic
            .message
            .contains("Function `f` expects 1 argument(s), but 2 provided"),
        "unexpected message: {}",
        diagnostic.message
    );
    assert_eq!((diagnostic.line, diagnostic.column), (5, 12));
}

#[test]
fn return_line_reports_undefined_function_by_name() {
    let source = "def main():\n    return nosuch()\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "undefined-function");
    assert!(
        diagnostic.message.contains("Undefined function `nosuch`"),
        "unexpected message: {}",
        diagnostic.message
    );
    assert_eq!((diagnostic.line, diagnostic.column), (2, 12));
}

#[test]
fn return_line_reports_math_intrinsic_arity() {
    let source = "def main():\n    return math.sqrt(1, 2)\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "function-argument-count");
    assert!(
        diagnostic
            .message
            .contains("math.sqrt() takes 1 argument, but 2 provided"),
        "unexpected message: {}",
        diagnostic.message
    );
    assert_eq!((diagnostic.line, diagnostic.column), (2, 12));
}

#[test]
fn duplicate_function_column_points_at_name_even_inside_def_prefix() {
    // `de` also occurs inside `def `; the name column must come from the
    // `def ` prefix width, not the first occurrence of the name text.
    let source = "def de(a):\n    /say {a}\n\ndef de(b):\n    /say {b}\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "duplicate-function");
    assert_eq!((diagnostic.line, diagnostic.column), (4, 5));
    let help = diagnostic.help.as_deref().unwrap_or("");
    assert!(help.contains("line 1, column 5"), "unexpected help: {help}");
}

// --- Skip-list canaries: one representative construct per consolidated site ---

#[test]
fn canary_assignment_target_scan_still_skips_return_lines() {
    // `return 1, 2` must not be reported as a destructuring assignment target.
    let diagnostics = analyze_source("def main():\n    return 1, 2\n");
    assert_no_kind(&diagnostics, "unsupported-assignment-target");

    // ...while unpacking a number still is.
    let diagnostics = analyze_source("def main():\n    a, b = 2\n");
    expect_kind(&diagnostics, "unsupported-assignment-target");
}

#[test]
fn canary_call_argument_scan_still_skips_raw_and_resource_lines() {
    let source = "def f(a):\n    /say {a}\n\ndef main():\n    /say f(1, 2)\nend f(1, 2)\n";
    let diagnostics = analyze_source(source);
    assert_no_kind(&diagnostics, "function-argument-count");
}

#[test]
fn canary_expression_reference_scan_covers_control_flow_and_skips_global() {
    // Control-flow headers are still scanned for undefined references.
    let diagnostics = analyze_source("def main():\n    if mystery > 0:\n        /say hi\n");
    let diagnostic = expect_kind(&diagnostics, "undefined-variable");
    assert!(
        diagnostic.message.contains("mystery"),
        "unexpected message: {}",
        diagnostic.message
    );
    assert_eq!(diagnostic.line, 2);

    // `global` statements are still skipped.
    let diagnostics = analyze_source("def main():\n    global mystery\n");
    assert_no_kind(&diagnostics, "undefined-variable");
}

#[test]
fn canary_assignment_symbol_scan_still_collects_assigned_names() {
    let diagnostics = analyze_source("def main():\n    x = 1\n    /say {x}\n");
    assert_no_kind(&diagnostics, "undefined-variable");
    assert_no_kind(&diagnostics, "undefined-placeholder");
}

#[test]
fn canary_noop_expression_scan_flags_bare_values_and_skips_pass() {
    let diagnostics = analyze_source("def main():\n    42\n");
    expect_kind(&diagnostics, "no-op-expression");

    let diagnostics = analyze_source("def main():\n    pass\n");
    assert_no_kind(&diagnostics, "no-op-expression");
}

#[test]
fn canary_assignment_rhs_command_helper_still_flagged_but_not_in_return() {
    // Command helpers on an assignment RHS keep their diagnostic...
    let diagnostics = analyze_source("def main():\n    v = score.set(\"obj\", 1)\n");
    let diagnostic = expect_kind(&diagnostics, "unsupported-function-call-expression");
    assert!(
        diagnostic.message.contains("command helper"),
        "unexpected message: {}",
        diagnostic.message
    );

    // ...but a returned module call is left to the undefined-function scan and
    // the transpiler instead of being misreported as an expression-value error.
    let diagnostics = analyze_source("def main():\n    return score.set(\"obj\", 1)\n");
    assert_no_kind(&diagnostics, "unsupported-function-call-expression");
}

fn expect_kind_with_message<'a>(
    diagnostics: &'a [SourceDiagnostic],
    kind: &str,
    message_fragment: &str,
) -> &'a SourceDiagnostic {
    diagnostics
        .iter()
        .find(|diagnostic| diagnostic.kind == kind && diagnostic.message.contains(message_fragment))
        .unwrap_or_else(|| {
            panic!("expected a `{kind}` diagnostic containing `{message_fragment}`, got: {diagnostics:?}");
        })
}

#[test]
fn a_quoted_subscript_placeholder_is_reported() {
    // `{m["k"]}` is an expression, not a JSON component; it used to print
    // literally because the scan skipped any brace content with a quote.
    let source = "m = {\"k\": 1}\n\ndef go():\n    /say {m[\"k\"]}\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "invalid-placeholder");
    assert!(
        diagnostic.message.contains("m[\"k\"]"),
        "the message names the expression: {}",
        diagnostic.message
    );
}

#[test]
fn joined_string_map_key_is_not_reported() {
    // A key joined from a literal and a parameter is text; the map-key check
    // must not infer it as a number from the masked source.
    let source = r#"b = {"ab": 7}
out = 0

def f(k):
    global out
    out = b["a" + k]

def main():
    f("b")
"#;
    let diagnostics = analyze_source(source);
    assert_no_kind(&diagnostics, "unsupported-storage-access");
}

#[test]
fn datapack_tag_value_column_counts_chars_after_non_ascii_tag_name() {
    // The Korean tag name is 6 bytes but 2 chars; the flagged tag value after
    // it must be reported at its char column, not a byte-drifted one.
    let source = "def main():\n    datapack.block_tag(\"한국\", [\"BAD\"])\n";
    let diagnostics = analyze_source(source);
    let value_diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag value 'BAD'",
    );
    assert_eq!((value_diagnostic.line, value_diagnostic.column), (2, 32));
    let name_diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag name '한국'",
    );
    assert_eq!((name_diagnostic.line, name_diagnostic.column), (2, 25));
}

#[test]
fn datapack_function_tag_value_column_counts_chars_after_non_ascii_tag_name() {
    let source = "def main():\n    datapack.function_tag(\"한국\", [\"pack:Load\"])\n";
    let diagnostics = analyze_source(source);
    let value_diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag value 'pack:Load'",
    );
    assert_eq!((value_diagnostic.line, value_diagnostic.column), (2, 40));
    let name_diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag name '한국'",
    );
    assert_eq!((name_diagnostic.line, name_diagnostic.column), (2, 28));
}

#[test]
fn datapack_tag_name_column_counts_chars_after_non_ascii_string() {
    // A non-ASCII string earlier in the expression drifted the masked byte
    // offset used to read the name argument, silently dropping the finding.
    let source = "def main():\n    note = \"한국\" + datapack.block_tag(\"BadName\", [\"minecraft:stone\"])\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag name 'BadName'",
    );
    assert_eq!((diagnostic.line, diagnostic.column), (2, 39));
}

/// Vendored diagnostic fixtures: each row pins the complete, ordered
/// `kind@line:column` output of `analyze_source` for one source file from
/// `tests/fixtures/diag_corpus`. Together they exercise the migrated
/// AST-anchored engine end-to-end — call arity and undefined-function
/// resolution, duplicate functions and parameters, value-helper misuse on
/// assignment/return lines, no-op expressions, `None` usage, storage access,
/// type inference, undefined variables, raw-command placeholders, and
/// single-/multi-line datapack argument checks with non-ASCII columns —
/// so the repo keeps corpus-level regression power after the session
/// scratchpad corpus is gone.
#[test]
fn vendored_diag_corpus_pins_kinds_and_positions() {
    /// One expected diagnostic: (kind, line, column).
    type ExpectedDiagnostic = (&'static str, usize, usize);
    // (file, expected diagnostics in emission order)
    const EXPECTED: &[(&str, &[ExpectedDiagnostic])] = &[
        ("arity_02.cbl", &[("undefined-function", 3, 13)]),
        ("arity_04.cbl", &[("function-argument-count", 5, 12)]),
        (
            "callexpr_02.cbl",
            &[("unsupported-function-call-expression", 2, 9)],
        ),
        ("dupfunc_01.cbl", &[("duplicate-function", 4, 5)]),
        (
            "dupparam_02.cbl",
            &[("duplicate-function-parameter", 1, 28)],
        ),
        (
            "multiline_01.cbl",
            &[
                ("datapack-resource-id", 4, 16),
                ("datapack-resource-argument", 4, 27),
            ],
        ),
        ("none_01.cbl", &[("unsupported-none", 3, 13)]),
        ("noop_01.cbl", &[("no-op-expression", 3, 5)]),
        ("placeholder_01.cbl", &[("undefined-placeholder", 3, 33)]),
        (
            "storage_03.cbl",
            &[
                ("unsupported-storage-access", 3, 13),
                ("unsupported-storage-access", 4, 12),
            ],
        ),
        ("typemis_01.cbl", &[("type-mismatch", 4, 5)]),
        ("undeffunc_02.cbl", &[("undefined-function", 3, 5)]),
        ("undefvar_01.cbl", &[("undefined-variable", 3, 13)]),
        (
            // The Korean tag name is 6 bytes / 2 chars: both columns must be
            // char columns, not byte-drifted ones.
            "unicode_06.cbl",
            &[
                ("datapack-resource-id", 2, 28),
                ("datapack-resource-id", 2, 40),
            ],
        ),
    ];

    let fixtures = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("diag_corpus");
    for (file, expected) in EXPECTED {
        let path = fixtures.join(file);
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let diagnostics = analyze_source(&source);
        let actual: Vec<(&str, usize, usize)> = diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.kind.as_str(), diagnostic.line, diagnostic.column))
            .collect();
        assert_eq!(
            &actual, expected,
            "{file}: diagnostics diverged: {diagnostics:?}"
        );
    }
}

#[test]
fn datapack_tag_value_column_is_unchanged_for_ascii_lines() {
    let source = "def main():\n    datapack.block_tag(\"blocks/custom\", [\"BAD\"])\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind_with_message(
        &diagnostics,
        "datapack-resource-id",
        "Invalid tag value 'BAD'",
    );
    assert_eq!((diagnostic.line, diagnostic.column), (2, 43));
}

#[test]
fn multibyte_identifiers_do_not_panic_the_compound_assignment_scanner() {
    // `top_level_compound_assignment` walks raw byte positions; slicing the
    // source with one of them panicked inside a multi-byte character whenever
    // a non-ASCII identifier preceded a potential compound-assignment operator.
    for source in [
        "def main():\n    val한e = 1\n",
        "한글 += 1\n",
        "def main():\n    x[\"한\"] += 1\n",
        "def main():\n    한글 -= 2\n",
        "def main():\n    résultat *= 3\n",
        "def main():\n    🎯 /= 4\n",
    ] {
        // The contract is "no panic"; whether the source is accepted or
        // rejected is checked elsewhere.
        let _ = analyze_source(source);
    }
}

#[test]
fn multibyte_expressions_do_not_panic_the_top_level_operator_scanner() {
    // Same byte-index-into-str hazard in `contains_top_level_operator`, which
    // scans for comparison and arithmetic operators outside delimiters.
    for source in [
        "def main():\n    value = helper.doubée(6)\n",
        "def main():\n    y = 한 + 漢\n",
        "def main():\n    z = a[\"🎯\"] == b\n",
        "def main():\n    if 한글 > 3:\n        /say hi\n",
        "def main():\n    q = \"한\" + '漢'\n",
    ] {
        let _ = analyze_source(source);
    }
}

#[test]
fn block_header_with_trailing_comment_or_whitespace_is_not_missing_a_colon() {
    for source in [
        "def main():  # entry point\n    /say hi\n",
        "def main():   \n    /say hi\n",
        "def main():\n    x = 1\n    if x > 0:  # positive\n        /say yes\n    else:  # other\n        /say no\n",
    ] {
        let diagnostics = analyze_source(source);
        assert!(diagnostics.is_empty(), "{source:?}: {diagnostics:?}");
    }

    // A header that really lacks its colon is still reported.
    let diagnostics = analyze_source("def main()  # no colon\n    /say hi\n");
    expect_kind(&diagnostics, "missing-block-colon");

    // `for ... else` tracking still sees a commented loop header.
    let diagnostics = analyze_source(
        "def main():\n    for x in range(2):  # loop\n        /say a\n    else:\n        /say b\n",
    );
    expect_kind(&diagnostics, "unsupported-control-flow");
}

#[test]
fn unicode_identifiers_are_single_names() {
    // The tokenizer accepts Unicode alphanumerics after an ASCII start; the
    // diagnostics scanners must not split `résultat` or read `força` as the
    // `for` keyword.
    let source = "@Größe = @a[tag=big]\n\ndef main():\n    força = 1\n    résultat = força + 1\n    /say {résultat}\n    as @Größe:\n        /say big\n";
    let diagnostics = analyze_source(source);
    assert!(diagnostics.is_empty(), "{diagnostics:?}");

    // A misspelled Unicode name is still undefined, reported as one name.
    let diagnostics = analyze_source("def main():\n    força = 1\n    y = forca + 1\n");
    let diagnostic = expect_kind(&diagnostics, "undefined-variable");
    assert!(diagnostic.message.contains("`forca`"), "{diagnostic:?}");
}

#[test]
fn compound_assignment_with_non_ascii_operand_does_not_panic() {
    // The type-check pass maps offsets from a synthesized `x + (rhs)` string
    // back to the source line; with no spaces around `+=` that mapping used
    // to land inside the multi-byte `한` and panic.
    let source = "def main():\n    m = {\"k\": 1}\n    a = 1\n    x = 0\n    x+=m[a한 + 1]\n";
    let _ = analyze_source(source);
}

#[test]
fn diagnostics_after_a_continued_command_keep_their_lines() {
    let source = "def main():\n    /say a \\\n        b \\\n        c\n    t = f(1)\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "undefined-function");
    assert_eq!((diagnostic.line, diagnostic.column), (5, 9));

    // The same with CRLF, and when the next line starts a new block.
    let source = "def main():\r\n    /say a \\\r\n        b\r\n\r\ndef g():\r\n    t = f(1)\r\n";
    let diagnostics = analyze_source(source);
    let diagnostic = expect_kind(&diagnostics, "undefined-function");
    assert_eq!((diagnostic.line, diagnostic.column), (6, 9));
}

#[test]
fn a_continued_command_takes_no_statement_checks() {
    // The lines after the `\` are part of the command, not statements.
    let source = "def main():\n    /say a \\\n  x = (\n    /say b \\\n'\n    /say c\n";
    assert_eq!(analyze_source(source), []);
}

#[test]
fn a_line_of_unclosed_braces_is_reported_once() {
    let source = format!("def f():\n    /say {}\n", "{a:".repeat(50));
    let diagnostics = cobble::diagnostics::analyze_source(&source);
    let unclosed = diagnostics
        .iter()
        .filter(|diagnostic| diagnostic.kind == "unclosed-placeholder")
        .count();
    assert_eq!(unclosed, 1, "{diagnostics:?}");
}
