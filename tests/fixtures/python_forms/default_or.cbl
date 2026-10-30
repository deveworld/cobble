def name_of(v):
    return len(v or "none")
def main():
    a = ""
    b = a or "anon"
    /say {b}
    c = "x"
    d = c or "anon"
    /say {d}
    e = 0
    f = e or 5
    g = 3
    h = g or 5
    i = g and 7
    j = e and 7
    /say {f} {h} {i} {j}
    k = [] or [1, 2]
    n = len(k)
    /say {n}
    o = e or g or 9
    p = g and e and 9
    /say {o} {p}
    q = name_of("") + name_of("ab")
    /say {q}
