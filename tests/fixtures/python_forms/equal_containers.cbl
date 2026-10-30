a = [1, 2]
b = [1, 2]
c = [1, 3]
m = {"x": 1, "y": [1]}
n = {"y": [1], "x": 1}
def main():
    if a == b:
        /say leq
    if a != c:
        /say lne
    if a == [1, 2]:
        /say lit
    if m == n:
        /say meq
    if m["y"] == [1]:
        /say sub
    d = [1, 2]
    d.append(3)
    if d != a:
        /say changed
