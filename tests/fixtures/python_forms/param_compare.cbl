def kind(v):
    if v == "a":
        return 1
    if v == 2:
        return 2
    return 3
def zero(n):
    if n == 0:
        return 1
    return 0
def main():
    a = kind("a")
    b = kind(2)
    c = kind("z")
    d = kind(9)
    e = zero(0)
    f = zero(5)
    /say {a} {b} {c} {d} {e} {f}
