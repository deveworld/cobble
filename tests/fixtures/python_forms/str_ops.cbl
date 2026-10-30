a = "ab"
b = "cd"
def main():
    c = a + b
    /say {c}
    n = len(c)
    /say {n}
    d = c[1]
    /say {d}
    e = c[-1]
    /say {e}
    f = c[1:3]
    /say {f}
    if a == "ab":
        /say eq
    if a != b:
        /say ne
    g = a * 2
    /say {g}
