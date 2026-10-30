def f():
    for i in range(3):
        for j in range(3):
            if i * j == 2:
                return i * 10 + j
    return -1
def main():
    x = f()
    /say {x}
