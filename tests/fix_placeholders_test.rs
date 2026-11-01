//! Placeholders, stdlib helper arguments, macro text, and the `temp`
//! objective: the cases the second recheck found one step away from the
//! first round of fixes.

mod common;

use common::mcsim::{compile, load_functions, Sim};

fn compile_error(source: &str) -> String {
    match compile(source) {
        Ok(_) => panic!("expected a compile error for:\n{source}"),
        Err(error) => error,
    }
}

fn function(source: &str, name: &str) -> String {
    let (_temp, output) = compile(source).unwrap_or_else(|error| panic!("{error}"));
    load_functions(&output)
        .get(name)
        .unwrap_or_else(|| panic!("no function {name}"))
        .join("\n")
}

// --- `temp` is rewritten only where the position holds an objective ---

#[test]
fn temp_in_user_data_stays_as_written() {
    let mut sim = Sim::load(
        r#"x = 0
def main():
    /data modify storage sim:a q set value {scores:{temp:36,wind:5},score:"temp"}
    /scoreboard players set x temp 4
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(
        sim.storage["q"],
        serde_json::json!({"scores": {"temp": 36, "wind": 5}, "score": "temp"})
    );
    assert_eq!(sim.var("x"), 4);

    let give = function(
        r#"def main():
    /give @s stone[custom_data={scores:{temp:1}}]
    /give @s stone[custom_data={score:"temp"}]
    /execute if predicate {type:"minecraft:entity_scores",entity:"this",scores:{temp:1}} run say a
    /tellraw @a {"text":"x","click_event":{"action":"run_command","command":"/trigger temp set 1"}}
"#,
        "main",
    );
    assert_eq!(
        give,
        "give @s stone[custom_data={scores:{temp:1}}]\n\
         give @s stone[custom_data={score:\"temp\"}]\n\
         execute if predicate {type:\"minecraft:entity_scores\",entity:\"this\",scores:{sim.vars:1}} run say a\n\
         tellraw @a {\"text\":\"x\",\"click_event\":{\"action\":\"run_command\",\"command\":\"/trigger sim.vars set 1\"}}"
    );
}

#[test]
fn temp_in_resource_custom_data_stays_as_written() {
    let (_temp, output) = compile(
        r#"from stdlib import datapack
datapack.item_modifier("m", {"type": "minecraft:set_custom_data", "tag": {"scores": {"temp": 1}, "score": "temp"}})
datapack.predicate("p", {"type": "minecraft:entity_scores", "entity": "this", "scores": {"temp": 1}})
"#,
    )
    .unwrap();
    let modifier = std::fs::read_to_string(output.join("data/sim/item_modifier/m.json")).unwrap();
    assert!(modifier.contains(r#""temp": 1"#), "{modifier}");
    assert!(modifier.contains(r#""score": "temp""#), "{modifier}");
    let predicate = std::fs::read_to_string(output.join("data/sim/predicate/p.json")).unwrap();
    assert!(predicate.contains(r#""sim.vars": 1"#), "{predicate}");
}

#[test]
fn temp_after_a_placeholder_for_a_subcommand_names_the_objective() {
    let source = r#"x = 0
def check(m):
    /execute {m} score x temp matches 7 run scoreboard players set hit temp 1
def sb(group, op):
    /scoreboard {group} {op} coins temp 1
def main():
    /scoreboard players set x temp 7
    /scoreboard players set hit temp 0
    /scoreboard players set coins temp 6
    check("if")
    sb("players", "add")
"#;
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    assert_eq!(sim.var("hit"), 1);
    assert_eq!(sim.var("coins"), 7);
}

// --- Stdlib helpers take a parameter as a macro argument ---

#[test]
fn a_parameter_in_helper_display_text_is_a_macro_argument() {
    let mk = function(
        r#"from stdlib import team, bossbar, score.objective, text
def mk(d):
    team.add("t", d)
    team.add("t2", "Team {d}")
    bossbar.add("b", d)
    bossbar.set_name("b", text.plain(d))
    score.objective.add("o", "dummy", d)
def main():
    mk("Real Name")
"#,
        "mk",
    );
    assert_eq!(
        mk,
        "$team add t {\"text\":\"$(d)\"}\n\
         $team add t2 {\"text\":\"Team $(d)\"}\n\
         $bossbar add sim:b {\"text\":\"$(d)\"}\n\
         $bossbar set sim:b name {\"text\":\"$(d)\"}\n\
         $scoreboard objectives add o dummy {\"text\":\"$(d)\"}"
    );
}

#[test]
fn helper_number_and_word_slots_take_a_parameter() {
    let source = r#"from stdlib import score, bossbar, timer, random
def slots(n, c, op):
    score.set("x", n)
    score.add("x", n)
    score.operation("x", op, "y")
    timer.set("tm", n)
    random.int("r", n, n)
    bossbar.set_value("b", n)
    bossbar.set_color("b", c)
def main():
    /scoreboard players set y temp 10
    slots(3, "red", "+=")
"#;
    let slots = function(source, "slots");
    assert!(
        slots.contains("$scoreboard players set x sim.vars $(n)"),
        "{slots}"
    );
    assert!(slots.contains("$bossbar set sim:b color $(c)"), "{slots}");
    assert!(
        slots.contains("$scoreboard players operation x sim.vars $(op) y sim.vars"),
        "{slots}"
    );
    let error =
        compile_error("from stdlib import score\nx = 1\ndef main():\n    score.set(\"x\", x)\n");
    assert!(
        error.contains("score value must be a literal integer, a const, or a parameter"),
        "{error}"
    );
}

#[test]
fn a_function_name_is_text_only_in_schedule_helpers() {
    let error = compile_error(
        "from stdlib import entity\ndef helper():\n    pass\ndef main():\n    entity.tag_add(\"@s\", helper)\n",
    );
    assert!(error.contains("the function `helper`"), "{error}");
    let main = function(
        "from stdlib import schedule\ndef helper():\n    pass\ndef main():\n    schedule.once(helper, \"1t\")\n",
        "main",
    );
    assert_eq!(main, "schedule function sim:helper 1t");
}

#[test]
fn a_helper_target_escape_is_read_once() {
    let b = function(
        "from stdlib import text\ndef b(zname):\n    text.tellraw(\"{{zname}}\", \"hi\")\ndef main():\n    b(\"x\")\n",
        "b",
    );
    assert_eq!(b, "tellraw {zname} {\"text\":\"hi\"}");
}

#[test]
fn a_parameter_in_resource_text_is_a_compile_error() {
    for source in [
        "from stdlib import datapack, text\ndef f(p):\n    datapack.dialog(\"d\", {\"type\": \"minecraft:notice\", \"title\": text.plain(p)})\n",
        "from stdlib import datapack, text, item_component\ndef g(p):\n    datapack.item_modifier(\"m\", {\"type\": \"minecraft:set_components\", \"components\": item_component.custom_name(text.plain(p))})\n",
    ] {
        let error = compile_error(source);
        assert!(error.contains("`{p}` has no value"), "{error}");
        assert!(error.contains(":3:"), "{error}");
    }
}

// --- Brace escapes ---

#[test]
fn plain_text_starting_with_an_escaped_brace_is_wrapped() {
    let source = r#"def p1(x):
    /tellraw @a {{{x}}}
    /tellraw @a {{ {x} }}
    /tellraw @a Hi {{{x}}}
def main():
    p1("v")
"#;
    let mut sim = Sim::load(source, &[]);
    assert_eq!(
        sim.function_text("p1"),
        "$tellraw @a {\"text\":\"{$(x)}\"}\n\
         $tellraw @a {\"text\":\"{ $(x) }\"}\n\
         $tellraw @a {\"text\":\"Hi {$(x)}\"}"
    );
    sim.call("main");
    assert_eq!(sim.log, ["{v}", "{ v }", "Hi {v}"]);
}

#[test]
fn a_single_close_inside_an_escape_is_a_compile_error() {
    for source in [
        "def k(k):\n    /data merge storage a:c4 {{k}:1}\n",
        "def k(k):\n    /data merge storage a:c4 {{k}:1,c:{d:1}}\n",
    ] {
        let error = compile_error(source);
        assert!(
            error.contains("A single `}` closes the `{{` escape"),
            "{error}"
        );
        assert!(error.contains(":2:"), "{error}");
    }
    let k = function(
        "def k(k):\n    /data merge storage a:c4 {{{k}:1}}\ndef main():\n    k(\"zz\")\n",
        "k",
    );
    assert_eq!(k, "$data merge storage a:c4 {$(k):1}");
}

#[test]
fn an_alias_escape_is_read_once_in_a_header() {
    let main = function(
        r#"@A = @e[name="{{a}}"]
@B = @e[type=minecraft:pig,name="{{{{a}}}}"]
def main():
    /kill @A
    as @A:
        /say hi
    /execute as @B run say inraw
    as @B:
        /say hdr
"#,
        "main",
    );
    assert_eq!(
        main,
        "kill @e[name=\"{a}\"]\n\
         execute as @e[name=\"{a}\"] run say hi\n\
         execute as @e[type=minecraft:pig,name=\"{{a}}\"] run say inraw\n\
         execute as @e[type=minecraft:pig,name=\"{{a}}\"] run say hdr"
    );
}

#[test]
fn a_loop_value_is_placed_as_it_is() {
    let mut sim = Sim::load(
        r#"def main():
    for m in ["{{a}}", "{b}"]:
        /say {m}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: {{a}}", "Server: {b}"]);
    let main = function(
        r#"def main():
    for n in ["{a}", "b"]:
        as @e[name="{n}"]:
            /say x
"#,
        "main",
    );
    assert_eq!(
        main,
        "execute as @e[name=\"{a}\"] run say x\nexecute as @e[name=\"b\"] run say x"
    );
}

// --- Macro text ---

#[test]
fn a_whole_message_parameter_works_with_inline_arguments() {
    let source = r#"def rawtell(msg):
    /tellraw @a {msg}
def tell_each(msg):
    /execute as @a run tellraw @s {msg}
def main():
    rawtell("hello world")
"#;
    let mut sim = Sim::load(source, &["Alex"]);
    assert_eq!(
        sim.function_text("rawtell"),
        "$tellraw @a {\"text\":\"$(msg)\"}"
    );
    assert_eq!(
        sim.function_text("tell_each"),
        "$execute as @a run tellraw @s {\"text\":\"$(msg)\"}"
    );
    sim.call("main");
    sim.call_with("rawtell", serde_json::json!({"msg": "other text"}));
    assert_eq!(sim.log, ["hello world", "other text"]);
}

#[test]
fn a_title_action_placeholder_keeps_the_text_wrapper() {
    let t = function(
        "def t(kind, a):\n    /title @a {kind} {a}\ndef main():\n    t(\"actionbar\", \"hello world\")\n",
        "t",
    );
    assert_eq!(t, "$title @a $(kind) {\"text\":\"$(a)\"}");
}

#[test]
fn a_variable_in_title_times_is_a_compile_error() {
    let error = compile_error("count = 3\ndef main():\n    /title @a times {count} 20 10\n");
    assert!(
        error.contains("Cannot interpolate scoreboard variable count"),
        "{error}"
    );
}

#[test]
fn a_parameter_in_a_quoted_selector_value_is_a_macro_argument() {
    let f = function(
        "def f(m):\n    /tellraw @a[name=\"{m}\", tag=x] {\"text\":\"hit\"}\ndef main():\n    f(\"hb\")\n",
        "f",
    );
    assert_eq!(f, "$tellraw @a[name=\"$(m)\", tag=x] {\"text\":\"hit\"}");
    assert!(!f.contains(['\u{E000}', '\u{E001}']));
}

// --- Execute headers ---

#[test]
fn a_keyword_word_can_be_a_score_holder_in_a_header() {
    let source = r#"x = 0
def main():
    /scoreboard players set facing temp 1
    /scoreboard players set as temp 1
    /scoreboard players set if temp 1
    /scoreboard players set #a temp 2
    /scoreboard players set #b temp 2
    if score facing temp matches 1:
        /say facing
    if score as temp matches 1:
        /say as
    if score if temp matches 1:
        /say if
    unless score summon temp matches 1:
        /say summon
    if score #a temp = #b temp:
        /say equal
    if score #a temp = #b temp and score facing temp = as temp:
        /say both
    as @a if score #a temp = #b temp unless score facing temp = if temp:
        /say never
    store result score positioned temp:
        /scoreboard players get #a temp
"#;
    let mut sim = Sim::load(source, &["Alex"]);
    sim.call("main");
    assert_eq!(
        sim.log,
        [
            "Server: facing",
            "Server: as",
            "Server: if",
            "Server: summon",
            "Server: equal",
            "Server: both"
        ]
    );
    assert_eq!(sim.var("positioned"), 2);
}

#[test]
fn unless_is_a_name_outside_execute_headers() {
    let source = r#"unless = 1
def f(unless):
    return unless + 1
def main():
    global unless
    unless = f(unless)
    if unless > 1:
        /say big
    unless entity @a[tag=nobody]:
        /say nobody
"#;
    let mut sim = Sim::load(source, &["Alex"]);
    sim.call("main");
    assert_eq!(sim.var("unless"), 2);
    assert_eq!(sim.log, ["Server: big", "Server: nobody"]);
}

#[test]
fn a_placeholder_may_be_part_of_a_header_word() {
    let f = function(
        r#"def f(k, a):
    summon minecraft:{k}:
        /say s
    align x{a}:
        /say a
    on {a}s:
        /say o
    anchored {a}s:
        /say n
def main():
    f("pig", "y")
"#,
        "f",
    );
    assert_eq!(
        f,
        "$execute summon minecraft:$(k) run say s\n\
         $execute align x$(a) run say a\n\
         $execute on $(a)s run say o\n\
         $execute anchored $(a)s run say n"
    );
}

#[test]
fn header_and_alias_errors_name_their_line() {
    let error =
        compile_error("def go():\n    /say hi\n    execute as @a[tag=t{nope}]:\n        /say x\n");
    assert!(error.contains("main.cbl:3:5"), "{error}");
    let error = compile_error("x = 1\n\ndef go():\n    /say hi\n@Far = @e[distance=..{NOPE}]\n");
    assert!(error.contains("main.cbl:5:1"), "{error}");
}

#[test]
fn elif_and_while_with_a_minecraft_condition_name_the_rewrite() {
    for (source, keyword) in [
        (
            "x = 1\ndef go():\n    if x > 9:\n        pass\n    elif entity @a and x > 3:\n        pass\n",
            "elif",
        ),
        (
            "x = 1\ndef go():\n    if x > 9:\n        pass\n    elif entity @a:\n        pass\n",
            "elif",
        ),
        (
            "x = 1\ndef go():\n    while entity @a and x < 3:\n        x += 1\n",
            "while",
        ),
    ] {
        let error = compile_error(source);
        assert!(
            error.contains(&format!("`{keyword}` can't test a Minecraft condition")),
            "{error}"
        );
        assert!(error.contains("store result score found temp:"), "{error}");
    }
}

#[test]
fn a_mixed_condition_in_an_expanded_loop_reads_the_loop_value() {
    let source = r#"x = 5
n = 0
def main():
    global n
    n = 0
    for i in range(3):
        if entity @a and x > i:
            n += 1
        if entity @a[tag=nobody] or x > i:
            n += 1
        if x > i and entity @a:
            n += 1
        if not entity @a[tag=nobody] and x > i:
            n += 1
        if entity @a if x > i:
            n += 1
        as @a if entity @s and x > i:
            n += 1
    for j in [1, 2, 6]:
        if entity @a and x > j:
            n += 10
"#;
    let mut sim = Sim::load(source, &["Alex"]);
    sim.call("main");
    assert_eq!(sim.var("n"), 18 + 20);
    assert!(!sim.function_text("main").contains("main.i"));
}

// --- Raw command text ---

#[test]
fn a_unicode_space_does_not_start_a_comment() {
    let main = function(
        "def main():\n    /say a\u{a0}#\u{a0}b\n    /say a #\u{a0}b\n    /say c\u{3000}#\u{3000}d\n    /say e # real comment\n",
        "main",
    );
    assert_eq!(
        main,
        "say a\u{a0}#\u{a0}b\nsay a #\u{a0}b\nsay c\u{3000}#\u{3000}d\nsay e"
    );
}

#[test]
fn show_entity_and_show_item_hover_strings_keep_quotes_and_backslashes() {
    for (component, pointer) in [
        (
            r#"{"text":"e","hover_event":{"action":"show_entity","id":"minecraft:pig","uuid":[0,0,0,1],"name":"{note}"}}"#,
            "/hover_event/name",
        ),
        (
            r#"{text:"e",hover_event:{action:"show_entity",id:"minecraft:pig",uuid:[I;0,0,0,1],name:{text:"{note}"}}}"#,
            "/hover_event/name/text",
        ),
        (
            r#"{"text":"i","hover_event":{"action":"show_item","id":"minecraft:stone","components":{"minecraft:custom_name":"{note}"}}}"#,
            "/hover_event/components/minecraft:custom_name",
        ),
    ] {
        for note in ["plain", "say \"hi\"", "back\\slash", "", "한글 \\\"end\\"] {
            let source = format!(
                "def hover(note):\n    /tellraw @a {component}\ndef main():\n    hover({})\n",
                serde_json::to_string(note).unwrap()
            );
            let mut sim = Sim::load(&source, &[]);
            // Capture the macro's fully expanded component as NBT, before
            // Minecraft renders it, so hover content is observable here.
            let mut captured = false;
            for (name, commands) in &mut sim.functions {
                if name.starts_with("_cobble_hover_") {
                    commands[0] = commands[0].replacen(
                        "$return run tellraw @a ",
                        "$data modify storage sim:global captured set value ",
                        1,
                    );
                    captured = true;
                }
            }
            assert!(captured, "{source}");
            sim.call("main");
            assert_eq!(
                sim.storage["captured"]
                    .pointer(pointer)
                    .and_then(|v| v.as_str()),
                Some(note),
                "{source}"
            );
        }
    }
}

#[test]
fn an_explicit_hover_macro_keeps_the_authors_text() {
    let command = r#"$tellraw @a {"text":"h","hover_event":{"action":"show_item","id":"minecraft:stone","components":{"minecraft:custom_name":"$(note)"}}}"#;
    let source = format!("def hover(note):\n    /{}\n", &command[1..]);
    assert_eq!(function(&source, "hover"), command);
}

#[test]
fn a_selector_alias_names_a_score_component_holder() {
    let main = function(
        r#"from stdlib import text
@Near = @a[limit=1]
def main():
    /tellraw @a {"score":{"name":"@Near","objective":"x"}}
    text.tellraw("@a", text.score("@Near", "x"))
"#,
        "main",
    );
    assert_eq!(
        main,
        "tellraw @a {\"score\":{\"name\":\"@a[limit=1]\",\"objective\":\"x\"}}\n\
         tellraw @a {\"score\":{\"name\":\"@a[limit=1]\",\"objective\":\"x\"}}"
    );
    let error = compile_error(
        "def main():\n    /tellraw @a {\"score\":{\"name\":\"@Nobody\",\"objective\":\"x\"}}\n",
    );
    assert!(error.contains("Unknown selector `@Nobody`"), "{error}");
}

#[test]
fn a_placeholder_cobble_cannot_fill_is_a_compile_error() {
    for source in [
        "l = [4, 6]\nfor v in l:\n    pass\ndef go():\n    /tellraw @a {\"text\":\"{v}\"}\n",
        "def go():\n    /scoreboard players set x temp {LATER}\nconst LATER = 3\n",
    ] {
        match compile(source) {
            // Another fix may give these names a value; then the text must
            // not show the braces.
            Ok((_temp, output)) => {
                let functions = load_functions(&output);
                assert!(
                    !functions["go"].join("\n").contains("{v}")
                        && !functions["go"].join("\n").contains("{LATER}"),
                    "{:?}",
                    functions["go"]
                );
            }
            Err(error) => assert!(error.contains("can't fill the placeholder"), "{error}"),
        }
    }
}
