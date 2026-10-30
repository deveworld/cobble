def main():
    a, b = 0, 1
    for i in range(10):
        a, b = b, a + b
    /say {a} {b}
    n = 7
    x, y = 0, 1
    while n > 0:
        x, y = y, x + y
        n -= 1
    /say {x} {y}
