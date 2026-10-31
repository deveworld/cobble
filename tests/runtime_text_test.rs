//! Text built while the pack runs: joining, repeating, `str()`, runtime map
//! keys, and reads of missing items and fields.
//!
//! A macro line reads backslashes and quotes in the text it pastes, so these
//! run in the command simulator, which reads SNBT escapes and path keys as
//! Minecraft 26.3 does.

mod common;

use common::mcsim::Sim;
use serde_json::{json, Value};

fn var(sim: &Sim, name: &str) -> Value {
    sim.storage["vars"][name].clone()
}

fn said(sim: &Sim) -> Vec<&str> {
    sim.log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line))
        .collect()
}

#[test]
fn joined_text_keeps_backslashes_and_quotes() {
    let mut sim = Sim::load(
        r#"
s = "a\\b"
t = "\\"
q = "say \"hi\""
j = "unset"
k = "unset"
l = "unset"
m = "unset"
n = "unset"
same = 0

def go():
    global j, k, l, m, n, same
    j = s + "z"
    k = t + "z"
    l = "tail\\" + "!"
    m = q + " and " + s
    n = "back\\slash" + "!"
    if s + "" == s:
        same = 1
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(var(&sim, "j"), json!("a\\bz"));
    assert_eq!(var(&sim, "k"), json!("\\z"));
    assert_eq!(var(&sim, "l"), json!("tail\\!"));
    assert_eq!(var(&sim, "m"), json!("say \"hi\" and a\\b"));
    assert_eq!(var(&sim, "n"), json!("back\\slash!"));
    assert_eq!(sim.var("same"), 1);
}

#[test]
fn repeated_and_shown_text_keeps_backslashes() {
    let mut sim = Sim::load(
        r#"
s = "a\\sb"
count = 3
r = "unset"
e = "unset"
v = "unset"
w = "unset"
x = "unset"

def go():
    global r, e, v, w, x
    r = s * count
    e = s * 0
    v = str(s)
    w = str(count) + "-" * count
    x = str("say \"hi\"")
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(var(&sim, "r"), json!("a\\sba\\sba\\sb"));
    assert_eq!(var(&sim, "e"), json!(""));
    assert_eq!(var(&sim, "v"), json!("a\\sb"));
    assert_eq!(var(&sim, "w"), json!("3---"));
    assert_eq!(var(&sim, "x"), json!("say \"hi\""));
}

#[test]
fn long_text_with_many_backslashes_joins_in_order() {
    // Enough pieces for two passes of the eight-piece join.
    let piece = "ab\\\\c\\\"d";
    let mut sim = Sim::load(
        &format!(
            r#"
unit = "{piece}"
out = "unset"

def go():
    global out
    out = unit * 12 + "end"
"#
        ),
        &[],
    );
    sim.call("go");
    let expected = "ab\\c\"d".repeat(12) + "end";
    assert_eq!(var(&sim, "out"), json!(expected));
}

#[test]
fn a_join_with_a_number_parameter_shows_the_number() {
    let mut sim = Sim::load(
        r#"
out = "unset"

def show(p, q):
    global out
    out = str(p) + q

def go():
    show("a\\", 3)
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(var(&sim, "out"), json!("a\\3"));
}

#[test]
fn a_failed_join_never_takes_another_joins_text() {
    let mut sim = Sim::load(
        r#"
cfg = {}
name = "Steve"
first = ""
second = ""

def go():
    global first, second
    first = "hello " + name
    second = "bye " + cfg.nick
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(var(&sim, "first"), json!("hello Steve"));
    assert_eq!(var(&sim, "second"), json!("bye "));
}

#[test]
fn say_shows_joined_text_with_a_backslash() {
    let mut sim = Sim::load(
        r#"
def go():
    t = "back\\slash"
    b = t + "!"
    /say [{b}]
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["[back\\slash!]"]);
}

#[test]
fn runtime_map_keys_keep_backslashes_and_quotes() {
    let mut sim = Sim::load(
        r#"
box = {}
out = 0
two = 0
quoted = 0
found = 0

def put(key, value):
    box[key] = value

def get(key):
    return box[key]

def go():
    global out, two, quoted, found
    put("a\\b", 5)
    put("\\\\", 6)
    put("say \"hi\"", 7)
    put("line\nbreak", 8)
    out = get("a\\b")
    two = get("\\\\")
    quoted = get("say \"hi\"")
    k = "a\\b"
    if k in box:
        found = 1
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(
        sim.storage["vars"]["box"],
        json!({"a\\b": 5, "\\\\": 6, "say \"hi\"": 7, "line\nbreak": 8})
    );
    assert_eq!(sim.var("out"), 5);
    assert_eq!(sim.var("two"), 6);
    assert_eq!(sim.var("quoted"), 7);
    assert_eq!(sim.var("found"), 1);
}

#[test]
fn a_parameter_key_into_a_parameter_keeps_its_backslash() {
    let mut sim = Sim::load(
        r#"
box = {"a\\b": "found", "x": [7, 8]}

def look(c, k):
    v = c[k]
    /say [{v}]

def nested(c, k, i):
    w = c[k][i]
    /say [{w}]

def go():
    look(box, "a\\b")
    nested(box, "x", 1)
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["[found]", "[8]"]);
}

#[test]
fn an_empty_runtime_key_is_refused_in_chat() {
    let mut sim = Sim::load(
        r#"
def go():
    m = {}
    k = "back\\slash"
    e = ""
    m[k] = 1
    m[e] = 2
    n = len(m)
    /say {n}
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["main.cbl:7: a map key can't be empty", "1"]);
}

#[test]
fn in_finds_literal_keys_with_backslashes_and_quotes() {
    let mut sim = Sim::load(
        r#"
m = {"back\\slash": 1, "say \"hi\"": 2}

def go():
    a = "back\\slash" in m
    b = "say \"hi\"" in m
    c = m["back\\slash"]
    d = m["say \"hi\""]
    e = "back\\s" in m
    /say {a} {b} {c} {d} {e}
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["1 1 1 2 0"]);
}

#[test]
fn no_map_holds_an_empty_key() {
    let mut sim = Sim::load(
        r#"
m = {"a": 2}

def go():
    e = ""
    a = e in m
    b = "" in m
    /say {a} {b}
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["0 0"]);
}

#[test]
fn pop_on_an_empty_list_gives_0_or_empty_text() {
    let mut sim = Sim::load(
        r#"
q = []
n = 0
names = ["a"]
s = ""
words = ["x", "y"]

def take():
    global n
    n = q.pop(0)
    /say took {n}

def takes():
    global s
    s = names.pop(0)
    /say took [{s}]

def take_at(i):
    w = words.pop(i)
    /say at [{w}]

def go():
    q.append(8)
    take()
    take()
    takes()
    takes()
    z = q.pop(0)
    /say got [{z}]
    take_at(1)
    take_at(1)
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(
        said(&sim),
        ["took 8", "took 0", "took [a]", "took []", "got [0]", "at [y]", "at []"]
    );
}

#[test]
fn a_missing_field_or_item_never_keeps_an_earlier_value() {
    let mut sim = Sim::load(
        r#"
items = ["p", "q"]
cfg = {"name": "Steve"}

def show(entry):
    d = entry.dir
    /say {d}

def item(i):
    t = items[i]
    /say [{t}]

def field(key):
    v = cfg[key]
    /say [{v}]

def go():
    show({"dir": "up"})
    show({})
    item(1)
    item(5)
    field("name")
    field("nope")
"#,
        &[],
    );
    sim.call("go");
    assert_eq!(said(&sim), ["up", "0", "[q]", "[]", "[Steve]", "[]"]);
}
