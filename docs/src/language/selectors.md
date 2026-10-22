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

Cobble replaces each alias at compile time, matching whole names only, in raw
commands and in execute-block targets and conditions. Aliases are shared by
every file in a project, so define each one once.

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
    "Tags": ["shop_keeper"],
    "PersistenceRequired": True,
    "NoAI": True
}
end

def spawn_shop():
    as @a[tag=spawn_shop] at @s:
        create @ShopKeeper
```

`create @ShopKeeper` compiles to a `summon` at the current execute position:

```mcfunction
execute as @a[tag=spawn_shop] at @s run summon minecraft:villager ~ ~ ~ {VillagerData:{...},CustomName:{...},Tags:["shop_keeper"],PersistenceRequired:1b,NoAI:1b}
```

The payload uses Cobble map and list literals and is written as SNBT. The
entity type comes from the selector's `type=` argument, so give every template
one. Other selector arguments are not applied to the new entity; add `Tags` to
the payload, as above, when the alias should match the entities it creates.
`create @Name` must name a template defined in the project, and template names
share the project-wide name set with functions and selector aliases.
