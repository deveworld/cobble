score = 1

def main(player):
    message = "ready"
    for i in range(3):
        /tellraw {player} {"text":"Score {score} {message} {i}"}
