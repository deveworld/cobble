# Loop Examples
# Demonstrates for loops and while loops

def for_loop_example():
    """Show for loop with range"""
    # Count from 0 to 4
    for i in range(5):
        /tellraw @a {"text":"Count: {i}", "color":"yellow"}

def for_loop_with_step():
    """Show for loop with custom step"""
    # Count by 2s
    for i in range(10) by 2:
        /tellraw @a {"text":"Even number: {i}", "color":"green"}

def while_loop_example():
    """Show while loop"""
    i = 0
    while i < 5:
        /tellraw @a {"text":"While iteration", "color":"blue"}
        i = i + 1

def nested_loop():
    """Show nested loops"""
    for x in range(3):
        for y in range(3):
            /tellraw @a {"text":"Nested loop iteration"}
