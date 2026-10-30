def even(n):
    if n == 0:
        return 1
    return odd(n - 1)
def odd(n):
    if n == 0:
        return 0
    return even(n - 1)
def main():
    a = even(4)
    b = even(5)
    /say {a} {b}
