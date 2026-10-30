m = {"점수": 3}
def get(k):
    return m[k]
def main():
    a = m["점수"]
    b = get("점수")
    /say {a} {b}
