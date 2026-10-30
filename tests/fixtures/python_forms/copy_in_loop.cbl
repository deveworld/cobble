items = [1, 2, 3]
def show(p):
    a = p
    for x in items:
        /say {a}
        a = a + 1
    b = p
    n = 0
    while n < 2:
        /say {b}
        b = 7
        n += 1
def main():
    show(10)
