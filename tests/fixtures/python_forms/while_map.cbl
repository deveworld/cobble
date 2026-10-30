m = {"n": 3}
def main():
    t = 0
    while m["n"] > 0:
        t += m["n"]
        m["n"] -= 1
    /say {t}
