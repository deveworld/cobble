def f(l):
    t = 0
    for v in l:
        t += v
    return t + len(l)
def main():
    x = f([1, 2, 3])
    /say {x}
