grid = [[1, 2], [3]]
inv = {"bag": ["a"], "n": 0}
def main():
    grid[1].append(4)
    grid[0].reverse()
    x = grid[0][0]
    y = len(grid[1])
    inv["bag"].append("b")
    z = inv["bag"].pop()
    w = len(inv["bag"])
    i = 1
    grid[i].append(9)
    v = grid[i].pop()
    u = grid[i].pop(0)
    /say {x} {y} {z} {w} {v} {u}
