def f(a, b):
    return a * 10 + b
def g(s):
    return len(s)
def main():
    x = 5
    a = f(1 if x > 3 else 2, 2 if x > 9 else 3)
    l = [1, 2]
    b = f(l.pop(), l.pop())
    c = g("ab" + "c")
    d = g([1] + [2])
    e = g("abcd"[1:3])
    h = f(min(1, 2), max(3, 4))
    /say {a} {b} {c} {d} {e} {h}
