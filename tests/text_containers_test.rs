//! Text inside lists and maps, characters of text, and reads of items and
//! fields that aren't there, run in the command simulator.
//!
//! Python raises an error for a read that finds nothing; Cobble gives 0, or
//! empty text, an empty list, or an empty map when it knows the value is
//! one (python.md), and the target never keeps what it held before.

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

fn run(source: &str) -> Sim {
    let mut sim = Sim::load(source, &[]);
    sim.call("go");
    sim
}

#[test]
fn a_character_has_a_length_and_a_truth() {
    let sim = run(r#"
cfg = {"s": "hi"}
names = [["ab", "cd"]]
s0 = "hi"
n1 = 0
n2 = 0
n3 = 0
n4 = 0
t1 = 0
t2 = 0
t3 = 0
t4 = 0

def go():
    global n1, n2, n3, n4, t1, t2, t3, t4
    n1 = len(cfg.s[1])
    n2 = len(names[0][1][0])
    n3 = len(s0[1])
    n4 = len(cfg.s[7])
    if cfg.s[1]:
        t1 = 1
    if not cfg.s[1]:
        t2 = 1
    if s0[1]:
        t3 = 1
    if cfg.s[7]:
        t4 = 1
"#);
    assert_eq!(
        [sim.var("n1"), sim.var("n2"), sim.var("n3"), sim.var("n4")],
        [1, 1, 1, 0]
    );
    assert_eq!(
        [sim.var("t1"), sim.var("t2"), sim.var("t3"), sim.var("t4")],
        [1, 0, 1, 0]
    );
}

#[test]
fn a_character_of_a_character_is_itself() {
    let sim = run(r#"
cfg = {"s": "hi"}
words = {"name": "Bob"}
s0 = "hi"
c1 = "unset"
c2 = "unset"
c3 = "unset"

def go():
    global c1, c2, c3
    c1 = cfg.s[1][0]
    c2 = s0[1][0]
    c3 = words.name[0][0]
    /say [{c3}]
"#);
    assert_eq!(var(&sim, "c1"), json!("i"));
    assert_eq!(var(&sim, "c2"), json!("i"));
    assert_eq!(var(&sim, "c3"), json!("B"));
    assert_eq!(said(&sim), ["[B]"]);
}

#[test]
fn a_character_past_the_end_is_empty_text() {
    let sim = run(r#"
cfg = {"s": "hi"}
names = ["hello"]
word = "hello"
empty = ""
r1 = "unset"
r2 = "unset"
r3 = "unset"
r4 = "unset"
r5 = "unset"
r6 = "unset"
r7 = "unset"
r8 = "unset"
same = 0

def go():
    global r1, r2, r3, r4, r5, r6, r7, r8, same
    i = 5
    j = -9
    r1 = cfg.s[5]
    r2 = cfg.s[-9]
    r3 = cfg.s[i]
    r4 = names[0][5]
    r5 = word[1]
    r5 = word[7]
    r6 = word[j]
    r7 = word[-9]
    r8 = empty[0]
    if word[7] == "":
        same = 1
"#);
    for name in ["r1", "r2", "r3", "r4", "r5", "r6", "r7", "r8"] {
        assert_eq!(var(&sim, name), json!(""), "{name}");
    }
    assert_eq!(sim.var("same"), 1);
}

#[test]
fn a_negative_index_counts_from_the_end_once() {
    let sim = run(r#"
word = "hello"
cfg = {"s": "hello"}
r1 = "unset"
r2 = "unset"
r3 = "unset"

def pick(p, k):
    global r3
    r3 = p[k]

def go():
    global r1, r2
    i = -4
    r1 = word[i]
    r2 = cfg.s[i]
    pick("hello", -6)
"#);
    assert_eq!(var(&sim, "r1"), json!("e"));
    assert_eq!(var(&sim, "r2"), json!("e"));
    assert_eq!(var(&sim, "r3"), json!(""));
}

#[test]
fn a_character_read_into_its_own_text_reads_first() {
    let sim = run(r#"
s = "hello"
t = "hello"

def go():
    global s, t
    s = s[1]
    t = t[9]
"#);
    assert_eq!(var(&sim, "s"), json!("e"));
    assert_eq!(var(&sim, "t"), json!(""));
}

#[test]
fn characters_of_a_text_parameter_and_of_its_items() {
    let sim = run(r#"
n = 0
t = 0
c = "unset"
m = 0
u = 0
d = "unset"

def text(p):
    global n, t, c
    n = len(p[1])
    if p[1]:
        t = 1
    c = p[1][0]

def items(p):
    global m, u, d
    m = len(p[1])
    if p[1]:
        u = 1
    d = p[1][0]

def go():
    text("hi")
    items(["ab", "cd"])
"#);
    assert_eq!([sim.var("n"), sim.var("t")], [1, 1]);
    assert_eq!(var(&sim, "c"), json!("i"));
    assert_eq!([sim.var("m"), sim.var("u")], [2, 1]);
    assert_eq!(var(&sim, "d"), json!("c"));
}

#[test]
fn unpacked_text_can_be_indexed() {
    let sim = run(r#"
pair = ["ab", "cd"]
rows = [{"s": "ab"}, {"s": "cd"}]
r1 = "unset"
r2 = "unset"
r3 = "unset"

def go():
    global r1, r2, r3
    a, b = pair
    r1 = b[1]
    c = b
    r2 = c[1]
    x, y = rows
    r3 = y.s[1]
"#);
    assert_eq!(var(&sim, "r1"), json!("d"));
    assert_eq!(var(&sim, "r2"), json!("d"));
    assert_eq!(var(&sim, "r3"), json!("d"));
}

#[test]
fn joined_text_can_be_indexed() {
    let sim = run(r#"
s = "ab"
r = "unset"
q = "unset"

def go():
    global r, q
    v = s + "z"
    r = v[1]
    w = "q" + s
    q = w[0] + v[2]
"#);
    assert_eq!(var(&sim, "r"), json!("b"));
    assert_eq!(var(&sim, "q"), json!("qz"));
}

#[test]
fn a_missing_read_into_a_field_or_an_item_clears_it() {
    let sim = run(r#"
names = ["ab", "cd"]
conf = {"a": "x"}
m = {"a": "x", "t": "old", "u": "old", "v": "old", "w": "old"}
l = ["old", "old"]

def go():
    k = "zz"
    m.t = m.nope
    l[0] = m.nope
    m.u = names[7]
    l[1] = names[7]
    m.v = conf[k]
    conf[k] = conf.nope
    m.w = conf.nope
"#);
    assert_eq!(
        var(&sim, "m"),
        json!({"a": "x", "t": 0, "u": "", "v": 0, "w": 0})
    );
    assert_eq!(var(&sim, "l"), json!([0, ""]));
    assert_eq!(var(&sim, "conf"), json!({"a": "x", "zz": 0}));
}

#[test]
fn a_missing_read_appended_adds_an_empty_value() {
    let sim = run(r#"
word = "hello"
conf = {"a": "x"}
out = []

def go():
    out.append(word[7])
    out.append(conf.nope)
    out.append(word[1])
"#);
    assert_eq!(var(&sim, "out"), json!(["", 0, "e"]));
}

#[test]
fn a_missing_field_read_in_a_second_call_gives_zero() {
    let sim = run(r#"
cache = {"score": 5}
out = {"v": 0}
first = 0
second = 7

def put(c):
    out.v = c.score

def go():
    global first, second
    put(cache)
    first = out.v
    put({})
    second = out.v
"#);
    assert_eq!(sim.var("first"), 5);
    assert_eq!(sim.var("second"), 0);
}

#[test]
fn an_empty_runtime_key_reads_as_a_missing_key() {
    let sim = run(r#"
box = {}
c = {"a": {"b": 1}, "n": {"m": 4}}
cfg = {"a": "q"}
nums = {"q": 3}
out = 7
x = 7
y = 7
z = 9
w = 7
p = 7

def read(key):
    return c[key].b

def go():
    global out, x, y, z, w, p
    k = ""
    box[k] = 5
    out = box[k]
    x = c[k].b
    y = c.n[k]
    z = nums[cfg.nope]
    w = read("")
    local = 7
    local = box[k]
    p = local
"#);
    for name in ["out", "x", "y", "z", "w", "p"] {
        assert_eq!(sim.var(name), 0, "{name}");
    }
    assert!(said(&sim)
        .iter()
        .all(|line| line.ends_with("a map key can't be empty")));
}

#[test]
fn a_missing_read_at_module_level_keeps_later_lines() {
    let sim = run(r#"
cfg = {}
left = cfg.value
word = "hi"
ch = word[5]

def go():
    /say before
    /say got {left}
    /say [{ch}]
    /say after
"#);
    assert_eq!(said(&sim), ["before", "got 0", "[]", "after"]);
}

#[test]
fn text_in_a_field_or_an_item_repeats() {
    let sim = run(r#"
m = {"s": "ab", "l": [1], "k": "ab"}
items = ["cd", "ef"]
a = {"s": "x"}

def go():
    m.s = 2 * m.s
    items[0] = 2 * items[0]
    m["k"] = 2 * m["k"]
    t = items[1]
    t *= 2
    items[1] = t
    w = m.l
    w *= 2
    m.l = w
    a.s = a.s + a.s + "!"
"#);
    assert_eq!(
        var(&sim, "m"),
        json!({"s": "abab", "l": [1, 1], "k": "abab"})
    );
    assert_eq!(var(&sim, "items"), json!(["cdcd", "efef"]));
    assert_eq!(var(&sim, "a"), json!({"s": "xx!"}));
}

#[test]
fn text_set_while_the_pack_runs_repeats() {
    let sim = run(r#"
m = {}
items = []
r = ""

def go():
    global r
    m.s = "ab"
    m.s *= 2
    items.append("cd")
    items[0] *= 2
    n = {}
    n.t = "x"
    n.t *= 3
    r = n.t
"#);
    assert_eq!(var(&sim, "m"), json!({"s": "abab"}));
    assert_eq!(var(&sim, "items"), json!(["cdcd"]));
    assert_eq!(var(&sim, "r"), json!("xxx"));
}

#[test]
fn a_parameter_repeats_or_multiplies_by_what_it_holds() {
    let sim = run(r#"
out = {"x": 0, "l": 0, "p": 0, "n": 0, "m": 0, "q": 0}
rows = [{"s": "ab"}]
r = ""

def text(x):
    x *= 2
    out.x = x

def item(x):
    x[0] *= 2
    out.l = x

def field(p):
    p.s *= 2
    out.p = p

def number(n):
    n *= 3
    out.n = n

def numbers(m):
    m[0] *= -2
    out.m = m

def items(q):
    q *= 2
    out.q = q

def go():
    global r
    text("ab")
    item(["cd"])
    field({"s": "ab"})
    number(7)
    numbers([5])
    items([1, 2])
    for x in rows:
        r = x.s * 2
"#);
    assert_eq!(
        var(&sim, "out"),
        json!({"x": "abab", "l": ["cdcd"], "p": {"s": "abab"}, "n": 21, "m": [-10], "q": [1, 2, 1, 2]})
    );
    assert_eq!(var(&sim, "r"), json!("abab"));
}

#[test]
fn arithmetic_python_refuses_on_text_is_a_compile_error() {
    for (body, needle) in [
        ("m.s -= 1", "`m.s` holds text, and `-=`"),
        ("m.s += 1", "`m.s` holds text, and `+=`"),
        ("m.l += 1", "`m.l` holds a list, and `+=`"),
        ("m.s *= m.s", "`m.s` holds text, and `*=`"),
        ("m.s[1] = \"x\"", "can't be changed in place"),
        ("t[0] += \"x\"", "can't be changed in place"),
    ] {
        let source =
            format!("m = {{\"s\": \"ab\", \"l\": [1]}}\nt = \"hi\"\n\ndef go():\n    {body}\n");
        let error = Sim::try_load(&source, &[])
            .err()
            .unwrap_or_else(|| panic!("`{body}` built"));
        assert!(error.contains(needle), "`{body}`: {error}");
        assert!(error.contains("main.cbl:5"), "`{body}`: {error}");
    }
}

#[test]
fn arithmetic_on_numbers_in_fields_and_items_still_works() {
    let sim = run(r#"
m = {"n": 5, "s": "ab"}
l = [3]

def go():
    m.n -= 1
    m.n *= 3
    m.s += "c"
    l[0] += 4
    l[0] %= 4
"#);
    assert_eq!(var(&sim, "m"), json!({"n": 12, "s": "abc"}));
    assert_eq!(var(&sim, "l"), json!([3]));
}

// The cases below were found by the differential test's text shapes.

#[test]
fn text_copied_between_items_and_fields_stays_text() {
    let sim = run(r#"
gs = "a"
gn = ["q r", "ab"]
gr = {"s": "Mc", "t": "ab"}
s = ""
n = 0

def other():
    gr.s = gr.t
    gr.t = gr.t

def go():
    global s, n
    gn[0] = gs[0]
    for v in [gn[0]]:
        gn[0] += v[0]
    gn[1] = gn[1][0] * 0
    s = gn[0][0] + gr.s[0]
    n = len(gr.t[0] * 0)
"#);
    assert_eq!(var(&sim, "gn"), json!(["aa", ""]));
    assert_eq!(var(&sim, "s"), json!("aM"));
    assert_eq!(sim.var("n"), 0);
}

#[test]
fn values_of_unknown_type_join_add_and_repeat_by_what_they_hold() {
    let sim = run(r#"
o = {"t": 0, "l": 0, "j": 0}
gn = ["Mc", "q r"]
r = 0
n = 0

def total(a, b):
    a += b
    return a

def text(a, b):
    a += b
    o.t = a

def items(a, b):
    a += b
    o.l = a

def keep(ps):
    gn[0] = ps
    gn[0] += gn[0][0]
    o.j = "Mc" + (ps[0] + ps[0])

def size(ps):
    global n
    n = len(ps * 2) + len(ps[0] * 0)

def go():
    global r
    r = total(3, 4)
    text("ab", "c")
    items([1], [2])
    keep("q r")
    size("ab")
"#);
    assert_eq!(sim.var("r"), 7);
    assert_eq!(
        var(&sim, "o"),
        json!({"t": "abc", "l": [1, 2], "j": "Mcqq"})
    );
    assert_eq!(var(&sim, "gn"), json!(["q rq", "q r"]));
    assert_eq!(sim.var("n"), 4);
}

#[test]
fn a_key_that_calls_a_function_runs_once_in_augmented_assignment() {
    let sim = run(r#"
calls = 0
gn = ["Mc", "hello"]
q = ["b", "a"]
d = {"a": 1, "b": 2}

def pick():
    global calls
    calls += 1
    return 0

def go():
    gn[pick()] += "Mc"
    d[q.pop(0)] += 5
"#);
    assert_eq!(sim.var("calls"), 1);
    assert_eq!(var(&sim, "gn"), json!(["McMc", "hello"]));
    assert_eq!(var(&sim, "d"), json!({"a": 1, "b": 7}));
    assert_eq!(var(&sim, "q"), json!(["a"]));
}

#[test]
fn literal_text_has_a_truth() {
    let sim = run(r#"
a = 0
b = 0

def go():
    global a, b
    if "hello":
        a = 1
    if not "":
        b = 1
"#);
    assert_eq!([sim.var("a"), sim.var("b")], [1, 1]);
}
