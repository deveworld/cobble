def f(m):
    return m["a"] + m["b"]
def main():
    x = f({"a": 1, "b": 2})
    /say {x}
    cfg = {"a": 5, "b": 6}
    y = f(cfg)
    /say {y}
