def rev(s):
    if len(s) <= 1:
        return 0
    return 1 + rev(s[1:])
def count(l, v):
    if not l:
        return 0
    head = l[0]
    rest = l[1:]
    n = count(rest, v)
    if head == v:
        n += 1
    return n
def fib(n):
    return n if n < 2 else fib(n - 1) + fib(n - 2)
def main():
    a = rev("abcd")
    b = count([1, 2, 1, 3, 1], 1)
    c = fib(10)
    /say {a} {b} {c}
