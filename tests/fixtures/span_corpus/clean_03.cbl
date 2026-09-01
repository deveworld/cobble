def main():
    total = 0
    if total > 10:
        /say high
    else:
        pass
    while total < 3:
        total = total + 1
    for i in range(3):
        /say loop
    match total:
        case 0:
            /say zero
        case _:
            /say other
