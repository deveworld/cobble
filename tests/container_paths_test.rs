//! Keys, indexes, and value types in lists and maps: a subscript picks a map
//! field or a list item by the type of its key at any depth, decided when
//! the pack runs for a key or a container whose type Cobble can't know, and
//! a value read from a field, a deep item, `pop`, or unpacking keeps its
//! type.

mod common;

use common::mcsim::{compile, Sim};
use serde_json::{json, Value};

fn storage(sim: &Sim, name: &str) -> Value {
    sim.storage["vars"][name].clone()
}

/// The `say` lines `main` prints.
fn said(source: &str) -> Vec<String> {
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    sim.log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect()
}

// --- Keys and indexes ---

#[test]
fn parameter_key_after_three_subscripts_reads_and_writes_the_field() {
    let mut sim = Sim::load(
        r#"tree = [[[{"k": 7}]]]
cfg = {"x": {"y": {"z": {"k": 3}}}}
out = 0
deep = 0

def use(key):
    global out, deep
    out = tree[0][0][0][key]
    tree[0][0][0][key] = 9
    deep = cfg.x.y.z[key]
    cfg.x.y.z[key] = 4

def main():
    use("k")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("out"), 7);
    assert_eq!(sim.var("deep"), 3);
    assert_eq!(storage(&sim, "tree"), json!([[[{"k": 9}]]]));
    assert_eq!(storage(&sim, "cfg"), json!({"x": {"y": {"z": {"k": 4}}}}));
}

#[test]
fn string_key_after_three_subscripts_is_a_map_key() {
    let mut sim = Sim::load(
        r#"tree = [[[{"k": 7}]]]
cfg = {"x": {"y": {"z": {"n": 1}}}}
name = "n"

def main():
    key = "k"
    tree[0][0][0][key] = 9
    cfg.x.y.z[name] = 5
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "tree"), json!([[[{"k": 9}]]]));
    assert_eq!(storage(&sim, "cfg"), json!({"x": {"y": {"z": {"n": 5}}}}));
}

#[test]
fn integer_parameter_after_two_subscripts_is_a_list_index() {
    let mut sim = Sim::load(
        r#"grid = [[[5, 6, 7]]]
m = {"x": {"y": [1, 2, 3]}}
out = 0
both = 0
field = 0
last = 0
taken = 0

def use(i, row, col):
    global out, both, field, last, taken
    out = grid[0][0][i]
    grid[0][0][i] = 9
    both = grid[0][row][col]
    field = m.x.y[i]
    m["x"]["y"][i] = 8
    last = grid[0][0][-i]
    taken = m.x.y.pop(i)

def main():
    use(1, 0, 2)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("out"), 6);
    assert_eq!(sim.var("both"), 7);
    assert_eq!(sim.var("field"), 2);
    assert_eq!(sim.var("last"), 7);
    assert_eq!(sim.var("taken"), 8);
    assert_eq!(storage(&sim, "grid"), json!([[[5, 9, 7]]]));
    assert_eq!(storage(&sim, "m"), json!({"x": {"y": [1, 3]}}));
}

#[test]
fn string_keys_that_are_not_plain_variables_pick_map_fields() {
    let mut sim = Sim::load(
        r#"tree = [[{"k": 7}, {}, {}, {}]]
cfg = {"key": "k"}
names = ["k"]
a = 0
b = 0
c = 0

def use(key):
    global a, b, c
    other = key
    a = tree[0][0][other]
    b = tree[0][0][cfg.key]
    c = tree[0][0][names[0]]
    tree[0][1][other] = 1
    tree[0][2][cfg.key] = 1
    tree[0][3][names[0]] = 1

def main():
    use("k")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!((sim.var("a"), sim.var("b"), sim.var("c")), (7, 7, 7));
    assert_eq!(
        storage(&sim, "tree"),
        json!([[{"k": 7}, {"k": 1}, {"k": 1}, {"k": 1}]])
    );
}

#[test]
fn a_key_read_from_a_map_filled_at_run_time_is_a_map_key() {
    let mut sim = Sim::load(
        r#"tree = [[{"k": 7}]]
request = {}
out = 0

def fill():
    request.key = "k"

def main():
    global out
    fill()
    key = request.key
    out = tree[0][0][key]
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("out"), 7);
}

#[test]
fn assigning_into_a_container_elsewhere_keeps_string_keys_map_keys() {
    let mut sim = Sim::load(
        r#"tree = [[{}]]
blank = [[{}]]

def reset():
    tree[0] = blank[0]

def store(key):
    tree[0][0][key] = 5

def main():
    key = "k"
    tree[0][0][key] = 1
    store("p")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "tree"), json!([[{"k": 1, "p": 5}]]));
}

#[test]
fn map_parameter_indexed_by_a_parameter() {
    let mut sim = Sim::load(
        r#"out = 0
local = 0
lst = 0
written = {}

def read(table, key):
    return table[key]

def read_local(table):
    key = "n"
    return table[key]

def write(table, key):
    global written
    table[key] = 9
    written = table

def main():
    global out, local, lst
    out = read({"k": 7}, "k")
    local = read_local({"n": 4})
    lst = read([4, 5, 6], 2)
    write({"k": 1}, "k")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("out"), 7);
    assert_eq!(sim.var("local"), 4);
    assert_eq!(sim.var("lst"), 6);
    assert_eq!(storage(&sim, "written"), json!({"k": 9}));
}

#[test]
fn attribute_or_item_used_directly_as_a_key() {
    let mut sim = Sim::load(
        r#"box = {"a": 1, "b": 1}
cfg = {"key": "a"}
names = ["b"]
rows = [{}, {"a": 0}]

def main():
    box[cfg.key] = 2
    box[names[0]] = 3
    rows[1][cfg.key] = 4
    box[cfg.key] += 10
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "box"), json!({"a": 12, "b": 3}));
    assert_eq!(storage(&sim, "rows"), json!([{}, {"a": 4}]));
}

#[test]
fn augmented_assignment_with_a_string_key_after_three_subscripts() {
    let mut sim = Sim::load(
        r#"tree = [[[{"a": 1}]]]

def bump(key):
    tree[0][0][0][key] += 5

def main():
    key = "a"
    tree[0][0][0][key] += 1
    bump("a")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "tree"), json!([[[{"a": 7}]]]));
}

#[test]
fn key_errors_on_augmented_assignment_name_the_line() {
    let error = compile(
        r#"box = {"a": 1}

def main():
    n = 5
    box[n] += 1
"#,
    )
    .expect_err("a number can't be a map key");
    assert!(error.contains("main.cbl:5:5"), "{error}");
}

#[test]
fn map_of_unknown_type_subscripted_by_a_string_parameter() {
    assert_eq!(
        said(
            r#"gear = {}

def lookup(box, key):
    return box[key]

def set_field(box, key, value):
    box[key] = value
    return box[key]

def read(k):
    v = gear.x.y[k]
    /say read {v}

def main():
    global gear
    a = lookup({"a": 7}, "a")
    b = set_field({"a": 1}, "a", 5)
    /say {a} {b}
    gear = {"x": {"y": {"a": 7}}}
    read("a")
"#
        ),
        ["7 5", "read 7"]
    );
}

#[test]
fn parameter_holding_a_map_takes_a_runtime_string_key() {
    assert_eq!(
        said(
            r#"def f(entry):
    n = "a"
    x = entry[n]
    y = entry.A[n]
    /say {x} {y}

def main():
    f({"a": 3, "A": {"a": 4}})
"#
        ),
        ["3 4"]
    );
}

#[test]
fn a_number_is_never_a_text_key_in_in() {
    assert_eq!(
        said(
            r#"m = {"5": 1}

def check(v):
    g = v in m
    /say {g}

def main():
    n = 5
    f = n in m
    /say {f}
    check(5)
    check("5")
"#
        ),
        ["0", "0", "1"]
    );
}

#[test]
fn each_key_of_unknown_kind_is_decided_when_the_pack_runs() {
    assert_eq!(
        said(
            r#"res = []

def pick(t, a, b):
    return t[a][b]

def put(t, a, b):
    global res
    t[a][b] = 9
    res = t

def main():
    x = pick({"x": [1, 2]}, "x", 1)
    y = pick([[1, 2]], 0, -1)
    z = pick([0, {"k": 5}], 1, "k")
    /say {x} {y} {z}
    put([0, {"k": 5}], 1, "k")
    /say {res}
    put({"x": [1, 2]}, "x", -1)
    /say {res}
"#
        ),
        ["2 2 5", r#"[0,{"k":9}]"#, r#"{"x":[1,9]}"#]
    );
}

#[test]
fn a_key_of_unknown_kind_on_a_parameter_of_unknown_type() {
    assert_eq!(
        said(
            r#"def get(v, k):
    x = v[k]
    /say {x}

def main():
    get([4, 5], 1)
    get("hey", -1)
    get({"a": "b"}, "a")
"#
        ),
        ["5", "y", "b"]
    );
}

// --- Value types ---

#[test]
fn value_from_a_field_a_deep_item_or_pop_can_be_subscripted() {
    let mut sim = Sim::load(
        r#"cfg = {"x": [5, 6, 7]}
rows = [[{"k": 3}]]
q = [[8, 9]]
out = 0
a = 0
b = 0
c = 0

def main():
    global out, a, b, c
    node = cfg.x
    out = node[1]
    item = rows[0][0]
    a = item["k"]
    b = item.k
    r = q.pop(0)
    c = r[1]
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("out"), 6);
    assert_eq!((sim.var("a"), sim.var("b"), sim.var("c")), (3, 3, 9));
}

#[test]
fn list_repeat_at_depth_two_repeats_the_list() {
    let mut sim = Sim::load(
        r#"grid = [[[5, 6, 7]]]
size = 0
got = []

def main():
    global size, got
    got = grid[0][0] * 2
    size = len(got)
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.var("size"), 6);
    assert_eq!(storage(&sim, "got"), json!([5, 6, 7, 5, 6, 7]));
}

#[test]
fn unpacked_text_keeps_its_type() {
    let mut sim = Sim::load(
        r#"names = ["ab", "cd"]
res = "unset"

def main():
    global res
    a, b = names
    res = b + a
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "res"), json!("cdab"));
}

#[test]
fn joining_two_slices_of_a_parameter() {
    assert_eq!(
        said(
            r#"def glue(v):
    r = v[:1] + v[1:]
    /say {r}

def main():
    glue([1, 2, 3])
    glue("abc")
"#
        ),
        ["[1,2,3]", "abc"]
    );
}

#[test]
fn local_copied_from_an_imported_list_is_read_by_attribute() {
    let mut sim = Sim::load_project(
        &[
            ("state.cbl", "items = [{\"value\": 4}]\n"),
            (
                "main.cbl",
                r#"from state import items

def g(i):
    entry = items[i]
    left = entry.value
    right = entry["value"]
    /say {left} {right}

def main():
    g(0)
"#,
            ),
        ],
        &[],
    );
    sim.call("main");
    assert_eq!(sim.log, ["Server: 4 4"]);
}

#[test]
fn comparing_maps_or_lists_gives_a_number() {
    assert_eq!(
        said(
            r#"def f(m):
    flag = {"k": 1} == m
    /say {flag}
    flag = [1] == m
    return flag

def main():
    r = f([1])
    /say {r}
"#
        ),
        ["0", "1"]
    );
}

#[test]
fn and_or_across_container_types_keep_the_selected_operand() {
    let mut sim = Sim::load(
        r#"def main():
    a = [0]
    h = {"a": 0}
    b = a and h
    /say {b}
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(sim.storage["locals"]["main"]["b"], json!({"a":0}));
    // Operands of one type still give one of them.
    assert_eq!(
        said(
            r#"def main():
    a = []
    h = [1, 2]
    b = a or h
    /say {b}
"#
        ),
        ["[1,2]"]
    );
}

#[test]
fn a_condition_as_an_argument_gives_its_number() {
    let mut sim = Sim::load(
        r#"out = []
names = ["a"]
m = {}
got = 0

def keep(v):
    global got
    got = v

def f(s):
    global m
    out.append(s in names)
    out.append(s != "a")
    m = {"k": s == "a"}
    keep(s in names)

def main():
    f("a")
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "out"), json!([1, 0]));
    assert_eq!(storage(&sim, "m"), json!({"k": 1}));
    assert_eq!(sim.var("got"), 1);
}

// --- Text in containers ---

#[test]
fn indexing_text_inside_a_list_or_map() {
    assert_eq!(
        said(
            r#"cfg = {"s": "hi"}
names = ["ab"]
rows = [[{"s": "xyz"}]]

def main():
    i = 1
    a = cfg["s"][1]
    b = names[0][1]
    c = cfg.s[-1]
    d = names[0][i]
    e = rows[0][0].s[1]
    /say {a} {b} {c} {d} {e}
    if cfg.s[i] == "i":
        /say yes
"#
        ),
        ["i b i b y", "yes"]
    );
}

#[test]
fn for_over_text_in_a_field_of_a_list_item() {
    let mut sim = Sim::load(
        r#"rows = [{"s": "hi"}]
res = ""

def main():
    global res
    for ch in rows[0].s:
        res = ch + res
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "res"), json!("ih"));
}

#[test]
fn repeating_text_in_a_field_or_item() {
    let mut sim = Sim::load(
        r#"m = {"s": "ab"}
items = ["cd"]

def main():
    m.s *= 2
    items[0] *= 2
"#,
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "m"), json!({"s": "abab"}));
    assert_eq!(storage(&sim, "items"), json!(["cdcd"]));
}

#[test]
fn indexing_a_string_held_in_a_map_field() {
    assert_eq!(
        said(
            r#"words = {"name": "Bob"}
cache = {}

def fill():
    global cache
    cache.name = "Bob"

def main():
    n = words.name[0]
    /say first {n}
    fill()
    m = cache.name[0]
    /say first {m}
    for ch in cache.name:
        /say char {ch}
"#
        ),
        ["first B", "first B", "char B", "char o", "char b"]
    );
}

#[test]
fn text_or_a_list_in_a_container_filled_at_run_time() {
    assert_eq!(
        said(
            r#"rows = []

def fill():
    rows.append({"s": "hey"})
    rows.append({"s": [7, 8]})

def main():
    fill()
    i = 0
    a = rows[i].s[1]
    b = rows[i + 1].s[-1]
    /say {a} {b}
    for ch in rows[i].s:
        /say char {ch}
    for n in rows[1].s:
        /say item {n}
"#
        ),
        ["e 8", "char h", "char e", "char y", "item 7", "item 8"]
    );
}
