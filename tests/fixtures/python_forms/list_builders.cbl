a = [1, 2]
def main():
    b = a + [3]
    c = [0] + a
    d = [0] * 3
    n = 2
    e = a * n
    f = 2 * [7]
    x = len(b)
    y = c[0]
    z = len(d)
    w = len(e)
    v = e[2]
    u = f[1]
    /say {x} {y} {z} {w} {v} {u}
    b += [4]
    q = len(b)
    /say {q}
