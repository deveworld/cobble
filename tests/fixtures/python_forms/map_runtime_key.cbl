cfg = {"a": 1, "b": 2}
def get(k):
    return cfg[k]
def put(k, v):
    cfg[k] = v
def main():
    x = get("b")
    /say {x}
    put("a", 40)
    put("c", 50)
    y = get("a") + get("c")
    /say {y}
    n = len(cfg)
    /say {n}
