use cobble::diagnostics::{analyze_source, parse_source};
use cobble::parser::parse;

#[test]
fn supported_language_surface_parses_representative_programs() {
    let cases = [
        (
            "literals and expressions",
            r#"
score = 10
offset = -2
power = 2 ^ 3 ^ 2
flag = not False or True and score >= 10
items = [1, 2, 3]
config = {"foo": 1, enabled: True}
"#,
        ),
        (
            "functions and control flow",
            r#"
def tick(player, amount):
    global total
    total = total + amount
    if total > 10:
        /say high
    elif total == 10:
        /say equal
    else:
        pass
    while total < 20:
        total = total + 1
    for i in range(3) by 1:
        /say loop
"#,
        ),
        (
            "match and execute blocks",
            r#"
def route(player):
    mode = 1
    match mode:
        case 0:
            /say zero
        case 1 to 3:
            /say range
        case _:
            /say other
    asat @s:
        /say asat
    as @a at @s if entity @s:
        /say execute
"#,
        ),
        (
            "imports selectors entities and resources",
            r#"
import stdlib
from stdlib import event

@Players = @a[type=player]

define @Marker = @e[type=marker]
create {"Tags": ["cobble"]}
end

create @Marker

datapack.function_tag("minecraft:load", ["cobble:setup"])
datapack.predicate("always", {"condition": "minecraft:random_chance", "chance": 1})

def setup():
    /say setup
"#,
        ),
    ];

    for (name, source) in cases {
        assert!(
            parse(source).is_ok(),
            "supported language surface should parse: {name}"
        );
    }
}

#[test]
fn unsupported_python_like_syntax_is_not_silently_accepted_by_parser() {
    let cases = [
        (
            "default parameters",
            r#"
def reward(player, amount=1):
    /say reward
"#,
        ),
        (
            "varargs",
            r#"
def reward(*players):
    /say reward
"#,
        ),
        (
            "compound assignment",
            r#"
def tick():
    score += 1
"#,
        ),
        (
            "list comprehension",
            r#"
def tick():
    values = [i for i in range(3)]
"#,
        ),
        (
            "class definition",
            r#"
class Game:
    pass
"#,
        ),
        (
            "try statement",
            r#"
try:
    /say risky
"#,
        ),
        (
            "dotted import",
            r#"
import foo.bar
"#,
        ),
    ];

    for (name, source) in cases {
        assert!(
            parse(source).is_err(),
            "unsupported Python-like syntax should not parse yet: {name}"
        );
    }
}

#[test]
fn unsupported_python_like_syntax_reports_actionable_diagnostics() {
    let source = r#"
@event.tick
def reward(player, amount=1, *extra):
    values = [i for i in range(3)]
    score += 1
    obj.field = 2
    break
    continue
    assert score
    raise score
    del score
import foo.bar
import helpers as h
from helpers import *
from helpers import setup as renamed
"#;

    let diagnostics = analyze_source(source);
    let messages = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.message.as_str())
        .collect::<Vec<_>>();

    assert!(messages
        .iter()
        .any(|message| message.contains("Decorators")));
    assert!(messages
        .iter()
        .any(|message| message.contains("Default parameter values")));
    assert!(messages.iter().any(|message| message.contains("`*args`")));
    assert!(messages
        .iter()
        .any(|message| message.contains("comprehensions")));
    assert!(messages
        .iter()
        .any(|message| message.contains("Compound assignment")));
    assert!(messages
        .iter()
        .any(|message| message.contains("simple identifier assignment")));
    assert!(messages
        .iter()
        .any(|message| message.contains("Dotted imports")));
    assert!(messages
        .iter()
        .any(|message| message.contains("Import aliases are not supported")));
    assert!(messages
        .iter()
        .any(|message| message.contains("Wildcard imports are not supported")));
    assert!(messages.iter().any(|message| message.contains("`break`")));
    assert!(messages
        .iter()
        .any(|message| message.contains("`continue`")));
    assert!(messages.iter().any(|message| message.contains("`assert`")));
    assert!(messages.iter().any(|message| message.contains("`raise`")));
    assert!(messages.iter().any(|message| message.contains("`del`")));
}

#[test]
fn parse_source_rejects_for_else_with_location() {
    let source = r#"
def main():
    for i in range(3):
        pass
    else:
        pass
"#;

    let diagnostics = parse_source(source).expect_err("for-else should be rejected");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, "unsupported-control-flow");
    assert_eq!(diagnostics[0].line, 5);
    assert_eq!(diagnostics[0].column, 5);
}

#[test]
fn parse_source_reports_structural_syntax_diagnostics() {
    let source = r#"
def main():
    value = (1 + 2
"#;

    let diagnostics = parse_source(source).expect_err("unclosed delimiter should be rejected");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, "unclosed-delimiter");
    assert_eq!(diagnostics[0].line, 3);
    assert_eq!(diagnostics[0].column, 13);
}

#[test]
fn parse_source_reports_semantic_preflight_diagnostics() {
    let source = r#"
def helper():
    pass

def helper():
    return

def main():
    result = helper()
"#;

    let direct_diagnostics = analyze_source(source);
    assert!(
        !direct_diagnostics.is_empty(),
        "direct analyzer should reject source"
    );

    let diagnostics = parse_source(source).expect_err("semantic preflight should reject source");
    let kinds = diagnostics
        .iter()
        .map(|diagnostic| diagnostic.kind.as_str())
        .collect::<Vec<_>>();

    assert!(kinds.contains(&"duplicate-function"));
    assert!(kinds.contains(&"unsupported-return"));
    assert!(kinds.contains(&"unsupported-function-call-expression"));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("Return statements are not supported")));
    assert!(diagnostics.iter().any(|diagnostic| diagnostic
        .message
        .contains("Function calls in expressions are not supported")));
}

#[test]
fn parse_source_reports_user_function_argument_count() {
    let source = r#"
def main():
    greet("@a")

def greet(player, message):
    /tellraw {player} {"text":"{message}"}
"#;

    let diagnostics = parse_source(source).expect_err("argument count should be rejected");

    assert_eq!(diagnostics.len(), 1);
    assert_eq!(diagnostics[0].kind, "function-argument-count");
    assert!(diagnostics[0]
        .message
        .contains("Function `greet` expects 2 argument(s), but 1 provided"));
}
