m = {"i": 1, "s": "x", "l": [1, 2], "m": {"k": 2}}
def main():
    a = m["i"]
    b = m["s"]
    c = m["l"][1]
    d = m["m"]["k"]
    /say {a} {b} {c} {d}
