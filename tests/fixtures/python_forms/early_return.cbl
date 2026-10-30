def find(l, t):
    i = 0
    for v in l:
        if v == t:
            return i
        i += 1
    return -1
def main():
    a = find([5, 6, 7], 6)
    b = find([5, 6, 7], 9)
    /say {a} {b}
