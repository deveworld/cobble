l = [1, 2, 3]
def f():
    return 7
def main():
    a, b = 1, 2
    a, b = b, a
    c, d, e = 1, a + b, f()
    /say {a} {b} {c} {d} {e}
    l[0], l[2] = l[2], l[0]
    x = l[0]
    y = l[2]
    p, q = [5, 6]
    /say {x} {y} {p} {q}
    t, u = "a", "b"
    t, u = u, t
    /say {t} {u}
