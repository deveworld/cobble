m = {"a": 1}
def has(k):
    if k in m:
        return 1
    return 0
def main():
    x = has("a")
    y = has("b")
    /say {x} {y}
