use std::collections::HashSet;

const PYTHON_COMPAT_DEF_FUNCTIONS: &str = "def functions with Cobble-compatible bodies";
const PYTHON_COMPAT_IF_BLOCKS: &str = "if/elif/else blocks";
const PYTHON_COMPAT_FOR_RANGE: &str = "for range(...) loops with literal bounds";
const PYTHON_COMPAT_BOOLEAN_GUARDS: &str =
    "boolean and/or/not expressions in supported command guards";
const PYTHON_COMPAT_COMPARISONS: &str = "comparison expressions over scores and literals";
const PYTHON_COMPAT_PASS: &str = "pass statement as an explicit no-op";

const PYTHON_COMPAT_SUPPORTED_CONSTRUCTS: &[&str] = &[
    PYTHON_COMPAT_DEF_FUNCTIONS,
    PYTHON_COMPAT_IF_BLOCKS,
    PYTHON_COMPAT_FOR_RANGE,
    PYTHON_COMPAT_BOOLEAN_GUARDS,
    PYTHON_COMPAT_COMPARISONS,
    PYTHON_COMPAT_PASS,
];

pub fn python_compat_supported_constructs() -> Vec<String> {
    PYTHON_COMPAT_SUPPORTED_CONSTRUCTS
        .iter()
        .map(|construct| (*construct).to_string())
        .collect()
}

pub fn python_compat_observed_constructs(source: &str) -> Vec<String> {
    let mut seen = HashSet::new();

    for line in source.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }

        if line.starts_with("def ") {
            seen.insert(PYTHON_COMPAT_DEF_FUNCTIONS);
        }
        if line == "pass" {
            seen.insert(PYTHON_COMPAT_PASS);
        }
        if line.starts_with("if ") || line.starts_with("elif ") || line == "else:" {
            seen.insert(PYTHON_COMPAT_IF_BLOCKS);
        }
        if line.starts_with("for ") && line.contains(" in range(") {
            seen.insert(PYTHON_COMPAT_FOR_RANGE);
        }

        if line.starts_with('/') {
            continue;
        }
        if contains_python_word(line, "and")
            || contains_python_word(line, "or")
            || contains_python_word(line, "not")
        {
            seen.insert(PYTHON_COMPAT_BOOLEAN_GUARDS);
        }
        if contains_comparison_operator(line) {
            seen.insert(PYTHON_COMPAT_COMPARISONS);
        }
    }

    PYTHON_COMPAT_SUPPORTED_CONSTRUCTS
        .iter()
        .filter(|construct| seen.contains(**construct))
        .map(|construct| (*construct).to_string())
        .collect()
}

pub fn python_compat_suggestion_for_kind(
    kind: &str,
    message: &str,
    help: Option<&str>,
) -> Option<String> {
    if let Some(keyword) = unsupported_python_keyword_from_message(message) {
        return Some(python_compat_suggestion_for_keyword(keyword).to_string());
    }

    let suggestion = match kind {
        "unsupported-function-parameter" => {
            "Use plain positional parameters and assign defaults inside the function body."
        }
        "duplicate-function-parameter" => "Use each function parameter name once.",
        "unsupported-return" => {
            "Use commands, scoreboard values, storage, or helper functions instead of returning Python values."
        }
        "unsupported-function-call-expression" => {
            "Call Cobble helper functions as statements, or use documented value helpers where expressions are accepted."
        }
        "unsupported-function-call-argument" => {
            "Use literal values, constants, or supported value-helper calls for this argument."
        }
        "unsupported-assignment" => {
            "Assign simple Cobble expressions, constants, or supported JSON/resource values."
        }
        "unsupported-assignment-target" => {
            "Assign to a simple variable name; destructuring, attributes, and subscripts are not assignment targets."
        }
        "unsupported-import" => {
            "Use simple Cobble imports such as `import helpers` or `from stdlib import text`."
        }
        "unsupported-comprehension" => {
            "Use an explicit compile-time `for` loop or a literal array/map instead of a comprehension."
        }
        "unsupported-control-flow" => {
            "Use Cobble `if`, `elif`, `else`, and compile-time `for` blocks; early loop exits are not supported."
        }
        "unsupported-none" => {
            "Use `null` only inside supported JSON resource values, or omit the value."
        }
        "unsupported-storage-access" => {
            "Use the `storage.path()`, `storage.child()`, or `storage.index()` helpers for storage paths."
        }
        "unsupported-placeholder-symbol" => {
            "Use named placeholders with identifiers such as `{player}` or `$(player)`."
        }
        "unsupported-decorator" => {
            "Use explicit Cobble declarations or helper calls; decorators are not part of the language."
        }
        "unsupported-python-syntax" => {
            "Rewrite this Python construct using Cobble functions, commands, resources, or compile-time loops."
        }
        _ => {
            return help
                .filter(|help| !help.is_empty())
                .map(std::string::ToString::to_string);
        }
    };

    Some(suggestion.to_string())
}

pub fn python_compat_suggestion_for_keyword(keyword: &str) -> &'static str {
    match keyword {
        "class" => "Use Cobble functions and explicit data/resource declarations instead of classes.",
        "try" | "except" | "finally" | "raise" => {
            "Minecraft functions do not have exception handling; use explicit conditions and commands."
        }
        "with" => "Use explicit function calls or resource declarations instead of context managers.",
        "break" => {
            "Cobble compile-time loops cannot break early; restructure the loop bounds or guard generated commands."
        }
        "continue" => {
            "Cobble compile-time loops cannot continue early; use an `if` guard inside the loop body."
        }
        "assert" => "Use an explicit `if` block and command output to report failed conditions.",
        "del" => "Use explicit scoreboard/storage updates; delete statements are not supported.",
        "nonlocal" => "Use module-level state or function parameters; Cobble has no nested runtime scopes.",
        "lambda" => "Define a named Cobble function instead of a lambda expression.",
        "yield" => {
            "Cobble functions cannot yield runtime values; write commands or generated resources directly."
        }
        "await" | "async" => {
            "Cobble has no async runtime; use scheduled functions for delayed Minecraft behavior."
        }
        "decorator" => {
            "Use explicit Cobble declarations or helper calls; decorators are not part of the language."
        }
        "comprehension" => {
            "Use an explicit compile-time `for` loop or a literal array/map instead of a comprehension."
        }
        _ => "Rewrite this Python construct using supported Cobble syntax.",
    }
}

fn unsupported_python_keyword_from_message(message: &str) -> Option<&str> {
    let keyword = message.strip_prefix('`')?.split_once('`')?.0;
    Some(keyword)
}

fn contains_python_word(line: &str, word: &str) -> bool {
    line.split(|ch: char| !ch.is_ascii_alphanumeric() && ch != '_')
        .any(|part| part == word)
}

fn contains_comparison_operator(line: &str) -> bool {
    line.contains("==")
        || line.contains("!=")
        || line.contains(">=")
        || line.contains("<=")
        || line.contains(" > ")
        || line.contains(" < ")
}
