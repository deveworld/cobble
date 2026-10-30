cfg = {"a": 1, "b": 2}
def main():
    cfg["a"] = 5
    cfg["b"] = cfg["b"] + 1
    x = cfg["a"] + cfg["b"]
    /say {x}
