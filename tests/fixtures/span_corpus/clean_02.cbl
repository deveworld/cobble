import stdlib
from stdlib import event

counter = 0

def tick(player):
    global counter
    counter = counter + 1

stdlib.addEventListener(event.TICK, tick)
