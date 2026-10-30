def even(n):
    if n == 0:
        return 1 == 1
    if n == 1:
        return 1 == 0
    return even(n - 2)
def main():
    a = even(6)
    b = even(7)
    /say {a} {b}
