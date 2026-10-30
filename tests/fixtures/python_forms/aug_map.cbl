m = {"c": 1}
l = [1, 2]
def main():
    m["c"] += 1
    m["c"] *= 3
    l[0] -= 5
    x = m["c"]
    y = l[0]
    /say {x} {y}
