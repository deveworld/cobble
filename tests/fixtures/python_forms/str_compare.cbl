a = "ab"
b = "cd"
def same(x, y):
    return x == y
def main():
    if a == "ab":
        /say eq
    if a != b:
        /say ne
    if "ab" == a:
        /say rev
    c = a
    if c == a:
        /say copy
    d = a[0]
    if d == "a":
        /say char
    x = same("q", "q")
    y = same("q", "r")
    z = same(1, 1)
    w = same(1, 2)
    /say {x} {y} {z} {w}
    v = a == "ab"
    u = a != "ab"
    /say {v} {u}
