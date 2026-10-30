m = {"a": [1, 2], "b": [3]}
def main():
    m["c"] = [4, 5, 6]
    n = len(m)
    /say {n}
    k = "c"
    v = m[k][1]
    /say {v}
    l = len(m[k])
    /say {l}
