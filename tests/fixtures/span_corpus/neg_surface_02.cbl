
def tick(player, amount):
    global total
    total = total + amount
    total += 1
    if total > 10:
        /say high
    elif total == 10:
        /say equal
    else:
        pass
    while total < 20:
        total = total + 1
    for i in range(3) by 1:
        /say loop
