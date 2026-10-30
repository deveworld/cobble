def f(n):
    a, b = n, n + 1
    if n > 0:
        f(n - 1)
    return a * 10 + b
def g(n):
    x = "a" if n > 1 else "b"
    y = 0
    if n > 0:
        y = g(n - 1)
    if x == "a":
        y += 10
    else:
        y += 1
    return y
def main():
    p = f(2)
    q = g(3)
    /say {p} {q}
