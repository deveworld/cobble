def main():
    xs = [1, 2, 3]
    xs = [xs[2], xs[0], 9]
    a = xs[0]
    b = xs[1]
    m = {"a": 1}
    m = {"a": m["a"] + 1, "b": m["a"]}
    c = m["a"]
    d = m["b"]
    /say {a} {b} {c} {d}
