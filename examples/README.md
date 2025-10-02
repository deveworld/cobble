# Cobble Examples

This directory contains example Cobble scripts demonstrating various features and use cases.

## Examples Overview

### 📚 basics.cbl
Basic syntax and language features:
- Variables and functions
- Control flow (if, for, while)
- Simple Minecraft commands

### 👋 hello.cbl
Complete "Hello World" example with:
- Event system usage
- Score tracking
- Particle effects
- Player interactions

### 🎮 events.cbl
Event-driven programming:
- Load and tick event handlers
- Player initialization
- Timed events (every second)
- Day/night cycle control

### 🗡️ boss.cbl
Boss fight system:
- Multi-phase boss battle
- Health bar (bossbar) integration
- Minion spawning
- Victory conditions

### 🏃 parkour.cbl
Parkour/checkpoint system:
- Checkpoint creation and management
- Player respawn handling
- Timer system
- Completion rewards

### 🎒 inventory.cbl
Inventory and item management:
- Custom item creation
- Enchanted items
- Starter kits
- Shop systems
- Chest organization

### 🎯 game_mechanics.cbl
Minigame mechanics:
- Team system
- Scoring and currency
- Random events
- Arena creation
- Match management

## Running Examples

1. Create a new project:
```bash
cobble init my-project
cd my-project
```

2. Copy an example to your src directory:
```bash
cp /path/to/cobble/examples/hello.cbl src/main.cbl
```

3. Build the data pack:
```bash
cobble build
```

4. Copy to Minecraft:
```bash
cp -r output/* ~/.minecraft/saves/YourWorld/datapacks/
```

## Learning Path

Recommended order for learning:

1. **basics.cbl** - Start here to learn syntax
2. **hello.cbl** - Understanding event system
3. **events.cbl** - Advanced event handling
4. **inventory.cbl** - Working with items
5. **game_mechanics.cbl** - Building minigames
6. **parkour.cbl** - Checkpoint systems
7. **boss.cbl** - Complex multi-phase mechanics

## Tips

- Functions starting with `/` are direct Minecraft commands
- Use `stdlib.addEventListener()` for event-driven code
- Variables are scoped to functions
- For loops with `range()` are optimized to Minecraft functions
- Check generated `.mcfunction` files to understand the output

## Contributing

Feel free to submit new examples via pull requests! Examples should:
- Demonstrate specific features or patterns
- Include helpful comments
- Be self-contained and runnable
- Follow Cobble best practices