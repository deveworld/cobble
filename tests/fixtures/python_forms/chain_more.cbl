def f(a, b, c):
    r = 0
    if a < b <= c:
        r = 1
    return r
def g(x):
    return 0 < x < 10
def main():
    a = f(1, 2, 2)
    b = f(2, 2, 3)
    c = g(5)
    d = g(50)
    x = 5
    e = 1 < x < 3 < 9
    h = 1 < x < 9 < 10
    while 0 < x < 9:
        x -= 2
    /say {a} {b} {c} {d} {e} {h} {x}
