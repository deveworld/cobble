def any_big(l):
    for v in l:
        if v > 2:
            return v > 3
    return 0 > 1
def main():
    a = any_big([1, 2, 3])
    b = any_big([1, 4])
    c = any_big([1])
    /say {a} {b} {c}
