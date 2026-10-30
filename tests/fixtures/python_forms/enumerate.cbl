l = [5, 6]
s = ["x", "y"]
def main():
    for i, v in enumerate(l):
        /say {i} {v}
    for i, v in enumerate(s, 1):
        /say {i} {v}
    for i, v in enumerate([7, 8, 9]):
        /say {i} {v}
