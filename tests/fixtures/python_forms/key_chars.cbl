m = {"a b": 1, "c-d": 2, "e.f": 3, "1": 4, "2x": 5, "g:h": 6}
def get(k):
    return m[k]
def main():
    a = m["a b"]
    b = m["c-d"]
    c = m["e.f"]
    d = m["1"]
    e = m["2x"]
    f = m["g:h"]
    /say {a} {b} {c} {d} {e} {f}
    g = get("a b")
    h = get("c-d")
    i = get("e.f")
    j = get("1")
    l = get("2x")
    o = get("g:h")
    /say {g} {h} {i} {j} {l} {o}
    m["e.f"] = 30
    x = m["e.f"]
    /say {x}
