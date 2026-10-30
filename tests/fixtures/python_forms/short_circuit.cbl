l = []
def main():
    if l and l.pop() > 0:
        /say popped
    r = len(l)
    /say {r}
    m = [4]
    if m and m.pop() > 0:
        /say yes
    q = len(m)
    /say {q}
    x = 0
    if x == 0 or m.pop() > 0:
        /say skipped
    y = 1 if x == 0 else m.pop()
    /say {y}
