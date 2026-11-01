//! Keys, indexes, and value types, round two: an item added by `+=` or `+`
//! is a write of its own, a key whose type only the running pack knows is
//! checked when it runs on a known map or list too, and values keep their
//! type through slices and parameters.

mod common;

use common::mcsim::Sim;
use serde_json::{json, Value};

fn storage(sim: &Sim, name: &str) -> Value {
    sim.storage["vars"][name].clone()
}

fn run(source: &str) -> Sim {
    let mut sim = Sim::load(source, &[]);
    sim.call("main");
    sim
}

/// The `say` lines `main` prints.
fn said(source: &str) -> Vec<String> {
    run(source)
        .log
        .iter()
        .map(|line| line.strip_prefix("Server: ").unwrap_or(line).to_string())
        .collect()
}

// --- Items added by `+=` and `+` (bugs 4, 2) ---

#[test]
fn item_added_by_plus_equals_is_read_by_key_in_another_function() {
    let sim = run(r#"rows = [[1, 2]]
out = 0
lit = 0

def use(k):
    global out, lit
    out = rows[1][k]
    lit = rows[1]["k"]
    rows[1][k] = 9

def main():
    global rows
    rows += [{"k": 3}]
    use("k")
"#);
    assert_eq!(sim.var("out"), 3);
    assert_eq!(sim.var("lit"), 3);
    assert_eq!(storage(&sim, "rows"), json!([[1, 2], {"k": 9}]));
}

#[test]
fn item_added_by_plus_or_by_a_parameter_is_read_by_key() {
    for add in ["rows = rows + [{\"k\": 3}]", "rows = [{\"k\": 3}] + rows"] {
        let at = if add.starts_with("rows = rows") { 1 } else { 0 };
        let source = format!(
            r#"rows = [[1, 2]]
out = 0

def use(k):
    global out
    out = rows[{at}][k]

def main():
    global rows
    {add}
    use("k")
"#
        );
        assert_eq!(run(&source).var("out"), 3, "{add}");
    }
    let sim = run(r#"rows = [[1, 2]]
out = 0

def use(k):
    global out
    out = rows[1][k]

def add(v):
    global rows
    rows += v

def main():
    add([{"k": 3}])
    use("k")
"#);
    assert_eq!(sim.var("out"), 3);
}

#[test]
fn item_added_into_an_item_or_a_field_is_read_by_key() {
    let sim = run(r#"rows = [[{"a": 1}]]
g = [[[1, 2]]]
c = {"a": [[1, 2]]}
m = {"a": [[1, 2]]}
n = [[[1, 2]]]
tree = [[[[1, 2]]]]
out = [0, 0, 0, 0, 0, 0, 0]

def use(i, k):
    out[0] = rows[0][1][i]
    out[1] = g[0][1]["k"]
    out[2] = c.a[1][k]
    out[3] = m["a"][1]["k"]
    out[4] = n[0][1]["k"]
    out[5] = tree[0][0][1][k]
    tree[0][0][1][k] = 9
    out[6] = g[0][1][k]

def main():
    j = 0
    rows[0] += [[5, 6, 3]]
    g[0] += [{"k": 3}]
    c.a += [{"k": 3}]
    m["a"] += [{"k": 3}]
    n[j] += [{"k": 3}]
    tree[0][0] += [{"k": 7}]
    use(2, "k")
"#);
    assert_eq!(storage(&sim, "out"), json!([3, 3, 3, 3, 3, 7, 3]));
    assert_eq!(storage(&sim, "tree"), json!([[[[1, 2], {"k": 9}]]]));
}

#[test]
fn list_added_to_a_list_of_maps_is_read_by_index() {
    let sim = run(r#"rows = [{"a": 1}]
out = 0
lit = 0

def use(i):
    global out, lit
    out = rows[1][i]
    lit = rows[1][2]

def main():
    global rows
    rows += [[5, 6, 3]]
    use(2)
"#);
    assert_eq!(sim.var("out"), 3);
    assert_eq!(sim.var("lit"), 3);
}

#[test]
fn tuple_assignment_is_a_write_of_each_name() {
    let sim = run(r#"rows = [[1, 2]]
ot = [{"k": 3}]
other = [[1, 2]]
out = 0
o2 = 0
x = 0

def use(k):
    global out, o2
    out = rows[0][k]
    o2 = other[0][k]

def main():
    global rows, ot, other, x
    rows, ot = ot, rows
    other, x = [{"k": 3}], 1
    use("k")
"#);
    assert_eq!(sim.var("out"), 3);
    assert_eq!(sim.var("o2"), 3);
}

#[test]
fn text_added_to_a_list_of_lists_reads_its_characters() {
    let sim = run(r#"rows = [[1, 2]]
st = ""

def main():
    global rows, st
    rows += ["xyz"]
    st = rows[1][1]
"#);
    assert_eq!(storage(&sim, "st"), json!("y"));
}

// --- A key of either type on a known map or list (bugs 2, 3, 44) ---

#[test]
fn number_parameter_on_a_known_map_reads_nothing_and_writes_nothing() {
    let sim = run(r#"m = [[[{"1": 7}]]]
flat = {"1": 7}
deep = {"x": {"y": {"1": 7}}}
out = [5, 5, 5, 5, 5]

def f(i):
    z = i
    out[0] = m[0][0][0][i]
    out[1] = m[0][0][0][z]
    out[2] = flat[i]
    out[3] = deep.x.y[i]
    out[4] = m[0][0][0].pop(i)
    m[0][0][0][i] = 99
    m[0][0][0][z] += 1
    flat[i] = 99
    flat[i] += 1
    deep.x.y[i] = 99
    flat.pop(i)
    deep.x.y.pop(z)

def main():
    f(1)
"#);
    assert_eq!(storage(&sim, "out"), json!([0, 0, 0, 0, 0]));
    assert_eq!(storage(&sim, "m"), json!([[[{"1": 7}]]]));
    assert_eq!(storage(&sim, "flat"), json!({"1": 7}));
    assert_eq!(storage(&sim, "deep"), json!({"x": {"y": {"1": 7}}}));
}

#[test]
fn number_parameter_key_agrees_between_in_and_read() {
    let sim = run(r#"m = {"1": 7, "5": 1}
o1 = 0
o2 = 5
o3 = 5

def get(k):
    return m[k]

def f(i):
    global o1, o2, o3
    if i in m:
        o1 = 1
    else:
        o1 = 2
    o2 = m[i]
    o3 = get(5)

def main():
    f(1)
"#);
    assert_eq!(sim.var("o1"), 2);
    assert_eq!(sim.var("o2"), 0);
    assert_eq!(sim.var("o3"), 0);
}

#[test]
fn text_parameter_on_a_known_map_still_picks_its_field() {
    let sim = run(r#"m = {"1": 7}
out = 0

def f(i):
    global out
    out = m[i]
    m[i] = 99
    m[i] += 1

def main():
    f("1")
"#);
    assert_eq!(sim.var("out"), 7);
    assert_eq!(storage(&sim, "m"), json!({"1": 100}));
}

#[test]
fn text_parameter_on_a_known_list_reads_nothing_and_writes_nothing() {
    let sim = run(r#"t = [[[10, 20, 30, 40]]]
u = {"x": {"y": [10, 20, 30]}}
out = [5, 5, 5, 5]

def f(key):
    other = key
    local = [10, 20, 30, 40]
    out[0] = t[0][0][key]
    out[1] = t[0][0][other]
    out[2] = u.x.y[key]
    out[3] = t[0][0].pop(key)
    t[0][0][key] = 99
    t[0][0][other] += 1
    u.x.y[key] = 99
    local[key] = 99
    /say {local}

def main():
    f("ab")
"#);
    assert_eq!(storage(&sim, "out"), json!([0, 0, 0, 0]));
    assert_eq!(storage(&sim, "t"), json!([[[10, 20, 30, 40]]]));
    assert_eq!(storage(&sim, "u"), json!({"x": {"y": [10, 20, 30]}}));
    assert_eq!(sim.log, vec!["Server: [10,20,30,40]"]);
}

#[test]
fn number_parameter_on_a_known_list_still_picks_its_item() {
    let sim = run(r#"t = [[[10, 20, 30, 40]]]
out = 0

def f(key):
    global out
    out = t[0][0][key]
    t[0][0][key] = 99
    t[0][0][key] += 1

def main():
    f(2)
"#);
    assert_eq!(sim.var("out"), 30);
    assert_eq!(storage(&sim, "t"), json!([[[10, 20, 100, 40]]]));
}

#[test]
fn number_key_on_a_map_changes_nothing() {
    let sim = run(r#"m = {"5": 1}
q = 0

def put(k):
    m[k] = 2
    m[k] += 1
    m.pop(k)

def main():
    global q
    put(5)
    if "5" in m:
        q = 1
"#);
    assert_eq!(sim.var("q"), 1);
    assert_eq!(storage(&sim, "m"), json!({"5": 1}));
}
