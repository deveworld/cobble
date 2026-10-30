l = [1, 2, 3, 4]
s = "abcd"
def main():
    t = 0
    for v in l[1:3]:
        t = t * 10 + v
    /say {t}
    u = ""
    for c in s[2:]:
        u = c + u
    /say {u}
    w = 0
    for v in l + [5]:
        w += v
    /say {w}
    x = 0
    for v in sorted([3, 1, 2]):
        x = x * 10 + v
    /say {x}
