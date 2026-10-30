def main():
    out = []
    for w in ["a", "bb", "c"]:
        n = len(w)
        t = "two" if n == 2 else "other"
        if w == "c":
            out.append(n * 10)
        else:
            out.append(n)
        /say {t}
    x = out[2]
    y = sum(out)
    /say {x} {y}
    for i in range(3):
        out.append(i)
    z = len(out)
    /say {z}
