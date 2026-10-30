x = 5
def num(c):
    return 1 if c else 2
def show(v):
    return v + 1
def main():
    a = 10 if x > 3 else 20
    b = 10 if x > 9 else 20
    c = "big" if x > 3 else "small"
    d = show(1 if x > 3 else 2)
    e = (1 if x > 9 else 2) * 3
    f = 1 if x > 9 else 2 if x > 8 else 3
    g = num(1) + num(0)
    /say {a} {b} {c} {d} {e} {f} {g}
    l = [1 if x > 3 else 2, 5]
    h = l[0]
    m = {"k": "a" if x > 9 else "b"}
    i = m["k"]
    /say {h} {i}
