d = {"a": [1, 2], "b": {"c": [3]}}
def main():
    d["a"].append(3)
    d["b"]["c"].insert(0, 9)
    x = d["a"][-1]
    y = d["b"]["c"][0]
    d["a"].reverse()
    z = d["a"][0]
    n = len(d["a"])
    e = "a" in d
    f = "z" in d
    g = "c" in d["b"]
    h = 2 in d["a"]
    /say {x} {y} {z} {n} {e} {f} {g} {h}
