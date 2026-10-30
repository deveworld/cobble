def f(s):
    match s:
        case "a":
            return 1
        case "b":
            return 2
        case _:
            return 3
def g(s):
    r = 0
    match s:
        case "x":
            r = 1
        case "y":
            r = 2
    return r
name = "b"
def main():
    a = f("a")
    b = f(name)
    c = f("z")
    d = g("y")
    e = g("q")
    /say {a} {b} {c} {d} {e}
