//! Regression tests for diagnostics fixes:
//! - column drift when non-ASCII text precedes a finding on the same line
//!   (columns must be computed against the same text the byte index came from)
//! - `return`-line function calls now get arity/undefined-function checks
//! - `def` name column computed from the `def ` prefix, not `trimmed.find(name)`
//! - canaries pinning each consolidated keyword skip list's effective set

use cobble::diagnostics::{analyze_source, SourceDiagnostic};

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

    // ...while real tuple assignments still are.
    let diagnostics = analyze_source("def main():\n    a, b = 1, 2\n");
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
