a = [1, 2, 3]
def main():
    a.append(4)
    n = len(a)
    /say {n}
    x = a.pop()
    /say {x}
    y = a[-1]
    /say {y}
    i = 1
    a[i] *= 5
    z = a[1]
    /say {z}
