def head(v, n):
    return len(v[:n])
def tail(v):
    r = v[1:]
    return len(r)
def main():
    a = head("abcd", 2)
    b = head([1, 2, 3], 5)
    c = tail("abc")
    d = tail([1, 2, 3, 4])
    /say {a} {b} {c} {d}
