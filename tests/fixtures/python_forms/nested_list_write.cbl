g = [[0, 0], [0, 0]]
def main():
    for r in range(2):
        for c in range(2):
            g[r][c] = r * 2 + c
    t = 0
    for row in g:
        for v in row:
            t = t * 10 + v
    /say {t}
