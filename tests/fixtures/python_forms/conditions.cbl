s = "a"
t = "b"
n = 2
m = {"k": "v", "n": 3}
l = ["x", "y", "x"]
def main():
    if not (s == "a"):
        /say c1
    if s == "a" or s == "b":
        /say c2
    if s == "a" and n > 1:
        /say c3
    if s == "z" and n > 1:
        /say c4
    if s == t == "a":
        /say c5
    if m["k"] == "v":
        /say c6
    if m["n"] == 3:
        /say c7
    i = 0
    j = 2
    if l[i] == l[j]:
        /say c8
    if l[i] == "x":
        /say c9
    if l[1] != "x":
        /say c10
    if s == "z":
        /say c11
    elif t == "b":
        /say c12
    else:
        /say c13
    f = s == "a"
    g = not f
    /say {f} {g}
