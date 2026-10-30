l = [1, 2, 3]
s = ["a", "b"]
t = "hello"
m = {"k": 1, "a b": 2}
def has(x, y):
    return x in y
def main():
    a = 2 in l
    b = 9 in l
    c = "a" in s
    d = "z" not in s
    e = "ell" in t
    f = "xyz" in t
    g = "k" in m
    h = "q" in m
    i = "a b" in m
    /say {a} {b} {c} {d} {e} {f} {g} {h} {i}
    p = has(1, [1, 2])
    q = has("b", ["a"])
    r = has("el", "hello")
    u = has("z", "hello")
    v = has("k", {"k": 1})
    w = has("j", {"k": 1})
    /say {p} {q} {r} {u} {v} {w}
    k = "k"
    if k in m and 1 in l:
        /say both
    n = 3
    if n in [1, 2, 3]:
        /say lit
    if n not in l or n in [9]:
        /say no
    y = 0
    if 5 in l:
        y = 1
    elif "b" in s:
        y = 2
    /say {y}
