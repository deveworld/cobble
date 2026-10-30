gs = "Mc"
gl = [0, 0]
total = 2
def change_text():
    global gs
    gs = "xyz"
    return 0
def change_list():
    gl[0] = 1
    return 1
def change_total():
    global total
    total = 18
    return 0
def change_item():
    gl[1] = 7
    return 1
def main():
    a = gs[change_text()]
    b = change_list() in gl
    c = gl.count(change_list())
    d = total * (change_total() or 1)
    e = [total, change_total()]
    f = e[0]
    gl[1] = 5
    g = gl[change_item()]
    /say {a} {b} {c} {d} {f} {g}
