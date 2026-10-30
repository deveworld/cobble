m = {"a.b": 1}
def main():
    k = "a.b"
    x = m[k]
    m[k] = 2
    y = m[k]
    /say {x} {y}
