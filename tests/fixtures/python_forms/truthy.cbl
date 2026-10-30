l = []
s = ""
t = "x"
m = {}
n = 0
def has(v):
    if v:
        return 1
    return 0
def main():
    if l:
        /say lt
    if not l:
        /say lf
    if s:
        /say st
    if not s:
        /say sf
    if t:
        /say tt
    if m:
        /say mt
    if not m:
        /say mf
    l.append(1)
    m["k"] = 1
    if l and m:
        /say both
    a = has("x")
    b = has("")
    c = has([1])
    d = has([])
    e = has(0)
    f = has(4)
    /say {a} {b} {c} {d} {e} {f}
    while l:
        l.pop()
    x = len(l)
    /say {x}
