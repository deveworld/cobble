s = "abcdef"
l = [0, 1, 2, 3, 4, 5]
def main():
    a = s[1:3]
    b = s[:2]
    c = s[4:]
    d = s[-2:]
    e = s[:-4]
    f = s[2:100]
    g = s[5:2]
    h = s[-100:2]
    /say {a} {b} {c} {d} {e} {f} {g} {h}
    i = 1
    j = 4
    k = s[i:j]
    m = l[1:3]
    n = len(m)
    o = m[1]
    p = l[i:j]
    q = len(p)
    r = l[-2:]
    t = r[0]
    u = l[:]
    v = len(u)
    /say {k} {n} {o} {q} {t} {v}
    w = s[1:3] + s[4:]
    /say {w}
