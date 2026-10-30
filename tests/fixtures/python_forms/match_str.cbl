def f(s):
    match s:
        case "a":
            return 1
        case "b":
            return 2
        case _:
            return 3
def main():
    a = f("a")
    b = f("b")
    c = f("z")
    /say {a} {b} {c}
