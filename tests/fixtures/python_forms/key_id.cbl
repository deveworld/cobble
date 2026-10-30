prices = {"minecraft:diamond": 100, "minecraft:iron_ingot": 10}
def price(id):
    return prices[id]
def main():
    a = prices["minecraft:diamond"]
    b = price("minecraft:iron_ingot")
    prices["minecraft:gold_ingot"] = 50
    c = price("minecraft:gold_ingot")
    prices["minecraft:diamond"] += 1
    d = prices["minecraft:diamond"]
    n = len(prices)
    /say {a} {b} {c} {d} {n}
