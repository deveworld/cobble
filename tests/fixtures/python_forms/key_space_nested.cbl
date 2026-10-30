shop = {"Iron Sword": {"cost": 5, "tags": ["a", "b"]}}
def cost(name):
    return shop[name]["cost"]
def main():
    a = shop["Iron Sword"]["cost"]
    b = cost("Iron Sword")
    shop["Iron Sword"]["cost"] = 9
    c = cost("Iron Sword")
    t = shop["Iron Sword"]["tags"][1]
    /say {a} {b} {c} {t}
