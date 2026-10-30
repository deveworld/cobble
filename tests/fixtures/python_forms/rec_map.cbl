m = {"depth": 0}
def down(n):
    if n == 0:
        return 0
    m["depth"] += 1
    return 1 + down(n - 1)
def main():
    x = down(4)
    y = m["depth"]
    /say {x} {y}
