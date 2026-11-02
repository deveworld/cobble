# Selectors And Entity Templates

## Entity Selector Definitions

`@Name = @selector[...]` defines a selector alias at module level:

```python
@Player = @a[type=player,gamemode=survival]
@Boss = @e[type=zombie,tag=boss]
@Admin = @a[tag=admin]

def give_rewards():
    as @Player:
        /give @s diamond

    as @Boss:
        /effect give @s strength 10 2

def admin_command():
    /tellraw @Admin {"text":"Admin message"}
```

Use aliases in raw commands, selector text components, and execute blocks.
Aliases are shared across the project. They cannot take extra filters:
define a second alias instead of writing `@Boss[distance=..8]`.

## Entity Templates

Entity templates name a reusable summon shape. Define a template with
`define @Name = @selector[...]`, provide the NBT payload after `create`, and end
the declaration with `end`. Create an entity from the template with
`create @Name` inside a function or execute block.

```python
define @ShopKeeper = @e[type=villager,tag=shop_keeper]
create {
    "VillagerData": {
        "profession": "minecraft:librarian",
        "level": 5,
        "type": "minecraft:plains"
    },
    "CustomName": {"text": "Shop Keeper", "color": "gold"},
    "PersistenceRequired": True,
    "NoAI": True
}
end

def spawn_shop():
    as @a[tag=spawn_shop] at @s:
        create @ShopKeeper
```

`create @ShopKeeper` summons the entity at the current position. Every
template needs a `type=`; its positive `tag=` filters become entity tags.
Other filters are not applied to the new entity. The payload uses Cobble
map and list literals.

Template names are shared across the project, like functions and aliases.
