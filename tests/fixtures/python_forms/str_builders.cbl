a = "ab"
def twice(s):
    return len(str(s) + s)
def main():
    b = a + "c"
    /say {b}
    c = "x" + a + "y"
    /say {c}
    d = a * 2
    /say {d}
    n = 3
    e = "z" * n
    /say {e}
    f = 2 * "q"
    /say {f}
    g = "n=" + str(5)
    /say {g}
    h = 7
    i = str(h) + "!"
    /say {i}
    j = twice("abc")
    /say {j}
    b += "d"
    /say {b}
    k = ""
    for ch in "abc":
        k = ch + k
    /say {k}
    l = len("ab" + "cd")
    /say {l}
