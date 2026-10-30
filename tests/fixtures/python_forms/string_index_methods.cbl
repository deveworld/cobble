def main():
    s = "hello"
    t = "x" if "ell" in s else "y"
    u = len(s[1:-1])
    v = s[1:-1] + s[0]
    w = s[-1] + s[0]
    /say {t} {u} {v} {w}
