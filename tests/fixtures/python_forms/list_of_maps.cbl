ps = [{"n": "a", "s": 1}, {"n": "b", "s": 2}]
def main():
    total = 0
    for p in ps:
        total += p["s"]
        n = p["n"]
        /say {n}
    /say {total}
    ps[1]["s"] = 7
    i = 1
    x = ps[i]["s"]
    /say {x}
    name = ps[i]["n"]
    /say {name}
