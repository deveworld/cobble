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

// --- Slices of a parameter, and `+` and `*` on values of unknown type
// (bugs 40, 14, 15, 9) ---

#[test]
fn slices_of_a_parameter_join_as_lists_or_text() {
    let lines = said(
        r#"def f(v):
    r = v[:1] + v[1:2] + v[2:]
    a = v[:1]
    b = v[1:]
    s = a + b
    t = v[:1] + (v[1:2] + v[2:])
    u = v[:1] + v[1:] + v[:1]
    /say {r} {s} {t} {u}

def main():
    f([1, 2, 3])
    f("abc")
"#,
    );
    assert_eq!(
        lines,
        vec!["[1,2,3] [1,2,3] [1,2,3] [1,2,3,1]", "abc abc abc abca"]
    );
}

#[test]
fn an_empty_slice_joins_as_an_empty_list() {
    let lines = said(
        r#"def f(v, i, p):
    r = v[:0] + v[1:]
    s = v[:i] + v[i + 1:]
    t = p.k[:0] + p.k
    /say {r} {s} {t}

def g(v):
    r = v[:1] + v[1:]
    n = len(r)
    /say {r} {n}

def main():
    f([1, 2, 3], 0, {"k": [1]})
    f([1, 2, 3], 1, {"k": [1]})
    g([])
"#,
    );
    assert_eq!(lines, vec!["[2,3] [2,3] [1]", "[2,3] [1,3] [1]", "[] 0"]);
}

#[test]
fn permutations_built_from_slices_of_a_parameter() {
    let sim = run(r#"out = []

def perms(rest, acc):
    if len(rest) == 0:
        out.append(acc)
        return 0
    i = 0
    while i < len(rest):
        perms(rest[:i] + rest[i + 1:], acc + [rest[i]])
        i += 1
    return 0

def main():
    perms([1, 2, 3], [])
"#);
    assert_eq!(
        storage(&sim, "out"),
        json!([
            [1, 2, 3],
            [1, 3, 2],
            [2, 1, 3],
            [2, 3, 1],
            [3, 1, 2],
            [3, 2, 1]
        ])
    );
}

#[test]
fn repeating_a_value_of_unknown_type_repeats_lists_and_text() {
    let lines = said(
        r#"def f(v, n):
    r = v[:1] * 2
    s = v[:] * n
    a = v[:1]
    t = a * 2
    p = v * 2
    q = v[0] * 2
    /say {r} {s} {t} {p} {q}

def main():
    f([1, 2], 2)
    f("ab", 2)
    f(["cd"], 1)
    f([4], 1)
"#,
    );
    assert_eq!(
        lines,
        vec![
            "[1,1] [1,2,1,2] [1,1] [1,2,1,2] 2",
            "aa abab aa abab aa",
            "[\"cd\",\"cd\"] [\"cd\"] [\"cd\",\"cd\"] [\"cd\",\"cd\"] cdcd",
            "[4,4] [4] [4,4] [4,4] 8",
        ]
    );
}

#[test]
fn items_of_lists_read_through_a_loop_or_a_copy_join_and_repeat() {
    let lines = said(
        r#"rows = [[1], [2]]
mixed = [[5, 6, 7], [3]]
names = ["ab", "cd"]

def main():
    for row in rows:
        got = row * 2
        both = row + row
        n = len(row * 2)
        /say {got} {both} {n}
    m = mixed[0] * 2
    /say {m}
    mixed[0] = 3
    p = names
    joined = p[1] + p[0]
    a, b = p
    swapped = b + a
    /say {joined} {swapped}
"#,
    );
    assert_eq!(
        lines,
        vec![
            "[1,1] [1,1] 2",
            "[2,2] [2,2] 2",
            "[5,6,7,5,6,7]",
            "cdab cdab"
        ]
    );
}

#[test]
fn items_of_a_parameter_join_as_text() {
    let lines = said(
        r#"def f(v):
    a, b = v
    r = b + a
    s = v[1] + v[0]
    /say {r} {s}

def main():
    f(["ab", "cd"])
"#,
    );
    assert_eq!(lines, vec!["cdab cdab"]);
}

#[test]
fn augmented_repeat_on_a_parameter_keeps_text_and_lists() {
    let lines = said(
        r#"def f(x, l, p):
    x *= 2
    l[0] *= 2
    p.s *= 2
    /say {x} {l} {p}

def main():
    f("ab", ["cd"], {"s": "ab"})
    f(3, [4], {"s": 5})
"#,
    );
    assert_eq!(
        lines,
        vec!["abab [\"cdcd\"] {\"s\":\"abab\"}", "6 [8] {\"s\":10}"]
    );
}

#[test]
fn two_parameters_still_add_and_multiply_as_numbers() {
    let sim = run(r#"out = [0, 0, 0]

def f(a, b):
    out[0] = a + b
    out[1] = a * b
    out[2] = a * 3 + b

def main():
    f(4, 5)
"#);
    assert_eq!(storage(&sim, "out"), json!([9, 20, 17]));
}

// --- Unpacked values and fields named like variables (bugs 12, 45) ---

#[test]
fn unpacked_values_can_be_subscripted() {
    let sim = run(r#"rows = [[1, 2], [3, 4]]
maps = [{"k": 4}, {"k": 5}]
cfg = {"p": [[1, 2], [3, 4]]}
out = [0, 0, 0, 0, 0]

def main():
    a, b = rows
    out[0] = b[1]
    x = b
    out[1] = x[1]
    c, d = [1, 2], [3, 4]
    out[2] = d[1]
    e, f = maps
    out[3] = f.k
    g, h = cfg.p
    out[4] = h[1]
"#);
    assert_eq!(storage(&sim, "out"), json!([4, 4, 4, 5, 4]));
}

#[test]
fn a_field_named_like_a_variable_is_still_a_field() {
    let sim = run(r#"y = {"k": 1}
count = 3
items = {"a": 1}
x = {"y": [1, 2]}
b = {"z": [5, 6, 7], "m": {"k": 7}, "count": [8, 9], "items": [4, 5], "x": {"z": [6]}}
cc = {"a": {"b": {"c": [1, 2]}}}
out = [0, 0, 0, 0, 0, 0, 0, 0]

def main():
    z = 1
    m = "k"
    out[0] = x.y[1]
    out[1] = b.z[0]
    out[2] = b.m[m]
    out[3] = b.count[0]
    out[4] = b.items[1]
    out[5] = b.x.z[0]
    out[6] = cc.a.b.c[1]
    out[7] = cc.a["b"].c[1]
"#);
    assert_eq!(storage(&sim, "out"), json!([2, 5, 7, 8, 5, 6, 2, 2]));
}

#[test]
fn a_copy_of_an_imported_list_item_reads_its_fields() {
    let mut sim = Sim::load_project(
        &[
            ("state.cbl", "items = [{\"value\": 7}, {\"value\": 9}]\n"),
            (
                "main.cbl",
                r#"import state

out = [0, 0, 0]

def main():
    e = items[0]
    out[0] = e.value
    out[1] = items[0].value
    left = items[1].value
    out[2] = left
"#,
            ),
        ],
        &[],
    );
    sim.call("main");
    assert_eq!(storage(&sim, "out"), json!([7, 7, 9]));
}

#[test]
fn an_undefined_name_as_a_list_index_is_an_error() {
    for line in [
        "x = nums[nokey]",
        "nums[nokey] = 5",
        "y = nums[nokey + 1]",
        "nums[nokey] += 1",
    ] {
        let source = format!("nums = [10, 20]\n\ndef main():\n    {line}\n");
        let error = Sim::try_load(&source, &[]).err().unwrap_or_default();
        assert!(
            error.contains("main.cbl:4:") && error.contains("Undefined variable `nokey`"),
            "{line}: {error}"
        );
    }
}

// --- Computed keys (bug 13) ---

#[test]
fn a_method_on_an_item_with_a_computed_key_runs() {
    let sim = run(r#"b = {"lx": [9], "z": [1, 2], "l": [5]}
t = [[{"lx": [1]}]]
out = [0, 0, 0, 0]

def f(k, c):
    b[k + "x"].append(3)
    b[str(k)].extend([4])
    b[k or "z"].append(6)
    b[k if c else "z"].append(7)
    b["z"].clear()
    t[0][0][k + "x"].append(2)
    out[0] = b[str(k)].pop()
    out[1] = b[k or "z"].pop()
    out[2] = b[k if c else "z"].pop()
    x = b[str(k)][1:]
    out[3] = len(x)

def main():
    b["l" + "x"].append(8)
    f("l", 1)
"#);
    assert_eq!(
        storage(&sim, "b"),
        json!({"l": [5], "lx": [9, 8, 3], "z": []})
    );
    assert_eq!(storage(&sim, "t"), json!([[{"lx": [1, 2]}]]));
    assert_eq!(storage(&sim, "out"), json!([7, 6, 4, 0]));
}

// --- Second check: augmented keys, pops, comparisons ---

#[test]
fn a_pop_in_an_augmented_key_runs_once() {
    let sim = run(r#"q = ["b", "a"]
d = {"a": 1, "b": 2}
n = [1, 2]
qq = [1, 0]
t = [[[{"x": 1, "y": 2}]]]
q3 = ["y", "x"]

def main():
    d[q.pop(0)] += 5
    n[qq.pop(0)] += 5
    t[0][0][0][q3.pop(0)] += 5
"#);
    assert_eq!(storage(&sim, "d"), json!({"a": 1, "b": 7}));
    assert_eq!(storage(&sim, "q"), json!(["a"]));
    assert_eq!(storage(&sim, "n"), json!([1, 7]));
    assert_eq!(storage(&sim, "t"), json!([[[{"x": 1, "y": 7}]]]));
}

#[test]
fn popped_text_joins() {
    let sim = run(r#"q = ["b", "a", "c"]
s = ""

def main():
    global s
    s = q.pop(0) + q.pop(0) + q.pop(0) + "z"
"#);
    assert_eq!(storage(&sim, "s"), json!("bacz"));
}

#[test]
fn a_number_is_never_equal_to_text_a_list_or_a_map() {
    let sim = run(r#"out = []

def f(m):
    r = 0
    if m == 1:
        r += 1
    if 1 == m:
        r += 2
    if m != 1:
        r += 4
    x = m == 1
    r += x * 8
    return r

def g(m):
    return 0 == m

def main():
    out.append(f({"k": 1}))
    out.append(f([1]))
    out.append(f("a"))
    out.append(f(1))
    out.append(f(0))
    out.append(g([]))
    out.append(g(""))
    out.append(g({}))
    out.append(g(0))
"#);
    assert_eq!(storage(&sim, "out"), json!([4, 4, 4, 11, 4, 0, 0, 0, 1]));
}

#[test]
fn augmented_assignments_python_rejects_are_errors() {
    let cases = [
        ("d = {\"a\": 1}", "d[[1]] += 1", "uses a list as a key"),
        ("nums = [1, 2]", "nums[nums] += 1", "uses a list as a key"),
        ("s = \"ab\"", "s[0] += 1", "can't be changed in place"),
        ("s = \"ab\"", "s[\"a\"] += 1", "can't be changed in place"),
        ("nums = [1, 2]", "nums.a += 1", "has no field `a`"),
        ("d = {\"a\": 1}", "d[\"a\"][\"z\"] += 1", "holds a number"),
        (
            "cfg = {\"s\": \"hi\"}",
            "cfg.s[1] = \"x\"",
            "can't be changed in place",
        ),
    ];
    for (setup, line, message) in cases {
        let source = format!("{setup}\n\ndef main():\n    {line}\n");
        let error = Sim::try_load(&source, &[]).err().unwrap_or_default();
        assert!(
            error.contains("main.cbl:4:") && error.contains(message),
            "{line}: {error}"
        );
    }
}

#[test]
fn key_errors_name_their_line() {
    let cases = [
        (
            "box = {\"a\": 1}\nn = 1\n\ndef main():\n    box[n + 1] += 1\n",
            "main.cbl:5:",
        ),
        (
            "box = {\"a\": 1}\nn = 1\n\ndef main():\n    if n:\n        pass\n    else:\n        box[n + 1] += 1\n",
            "main.cbl:8:",
        ),
        (
            "box = {\"a\": 1}\nn = 1\n\ndef main():\n    box[n * 2] = 1\n",
            "main.cbl:5:",
        ),
        (
            "pairs = [[\"a\", 1]]\nm = {}\n\ndef main():\n    for k, v in pairs:\n        m[k] = v\n",
            "main.cbl:5:",
        ),
        (
            "a = [1]\nh = {\"k\": 1}\n\ndef main():\n    b = (a and h) == h\n",
            "main.cbl:5:",
        ),
    ];
    for (source, location) in cases {
        let error = Sim::try_load(source, &[]).err().unwrap_or_default();
        assert!(error.contains(location), "{source}: {error}");
    }
}

#[test]
fn a_choice_between_a_list_and_a_map_names_no_internal_variable() {
    let error = Sim::try_load(
        "a = [1]\nh = {\"k\": 1}\nc = 1\n\ndef g(x):\n    pass\n\ndef main():\n    g(a if c else h)\n",
        &[],
    )
    .err()
    .unwrap_or_default();
    assert!(
        error.contains("main.cbl:9:") && error.contains("gives a list or a map"),
        "{error}"
    );
    assert!(!error.contains("_cobble_hold"), "{error}");
}

#[test]
fn a_condition_assigned_to_an_item_or_a_field_stores_its_flag() {
    let sim = run(r#"out = [0, 0, 0, 0, 0]
d = {"k": 0, "j": 0}

def f(s, a):
    out[0] = s in ["a"]
    d["k"] = s in ["b"]
    d.j = a == 2
    out[1] = a not in [2]
    out[2] = not a
    out[3] = a > 1
    out[4] = s in d

def main():
    f("a", 2)
"#);
    assert_eq!(storage(&sim, "out"), json!([1, 0, 0, 1, 0]));
    assert_eq!(storage(&sim, "d"), json!({"k": 0, "j": 1}));
}

#[test]
fn a_module_comparison_of_lists_names_the_rewrite() {
    let error = Sim::try_load("x = [1] == [1]\n\ndef main():\n    pass\n", &[])
        .err()
        .unwrap_or_default();
    assert!(error.contains("Assign it inside a function"), "{error}");
}

#[test]
fn a_missing_read_at_module_level_still_prints() {
    let lines = said(
        "cfg = {}\nleft = cfg.value\n\ndef main():\n    /say before\n    /say got {left}\n    /say after\n",
    );
    assert_eq!(lines, vec!["before", "got 0", "after"]);
}
