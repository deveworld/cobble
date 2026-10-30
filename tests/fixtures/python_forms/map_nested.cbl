d = {"p": {"hp": 3, "mp": [1, 2, 3]}}
def main():
    d["p"]["mp"][1] = 9
    y = d["p"]["mp"][1] + d["p"]["hp"]
    /say {y}
    d["p"]["hp"] += 4
    z = d["p"]["hp"]
    /say {z}
    n = len(d["p"]["mp"])
    /say {n}
