a = [3, 1, 2]
names = ["bo", "al", "cy"]
def main():
    a.append(5)
    a.insert(1, 9)
    a.insert(-1, 7)
    a.insert(100, 8)
    x = a.pop(0)
    y = a.pop()
    a.remove(1)
    n = len(a)
    i = a.index(2)
    c = a.count(9)
    /say {x} {y} {n} {i} {c}
    a.sort()
    p = a[0]
    q = a[-1]
    a.reverse()
    r = a[0]
    /say {p} {q} {r}
    names.remove("al")
    names.append("di")
    names.insert(0, "ed")
    j = names.index("cy")
    first = names[0]
    last = names.pop()
    /say {j} {first} {last}
    a.extend([1, 1])
    k = a.count(1)
    a.clear()
    z = len(a)
    /say {k} {z}
