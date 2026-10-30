x = 5
def a():
    return x > 3
def b():
    return x == 5 and x < 2
def c():
    return not (x > 3)
def d():
    return x > 3 or x < 0
def e(v):
    return v
def main():
    p = a()
    q = b()
    r = c()
    s = d()
    t = 0
    if a() and d():
        t = 1
    u = e(a() + 1)
    /say {p} {q} {r} {s} {t} {u}
