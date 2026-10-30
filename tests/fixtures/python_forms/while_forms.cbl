def main():
    l = [1, 2, 3, 0, 5]
    n = 0
    while l.pop(0) > 0:
        n += 1
    /say {n}
    m = len(l)
    /say {m}
    s = "aaab"
    t = 0
    while s[t:t + 1] == "a":
        t += 1
    /say {t}
    x = 0
    while x < 3 if t > 0 else x < 1:
        x += 1
    /say {x}
