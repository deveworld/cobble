"""Dialogs: the server menu and the rules page.

The menu is added to the pause screen through the vanilla
`minecraft:pause_screen_additions` dialog tag, so players can open it from
Esc without typing a command. Buttons run the same trigger commands players
can type themselves, so they need no operator permission.
"""

from stdlib import datapack

datapack.dialog("menu", {
    "type": "minecraft:multi_action",
    "title": {"text": "Server Menu", "color": "gold"},
    "external_title": {"text": "Server Menu"},
    "body": {
        "type": "minecraft:plain_message",
        "contents": {"text": "Everything here also works as /trigger <name>."}
    },
    "columns": 2,
    "actions": [
        {
            "label": {"text": "Go to spawn"},
            "tooltip": {"text": "/trigger spawn"},
            "action": {"type": "minecraft:run_command", "command": "trigger spawn"}
        },
        {
            "label": {"text": "My stats"},
            "tooltip": {"text": "/trigger stats"},
            "action": {"type": "minecraft:run_command", "command": "trigger stats"}
        },
        {
            "label": {"text": "Server rules"},
            "action": {"type": "minecraft:show_dialog", "dialog": "lobby:rules"}
        }
    ],
    "exit_action": {"label": {"text": "Close"}}
})

datapack.dialog("rules", {
    "type": "minecraft:notice",
    "title": {"text": "Server Rules", "color": "gold"},
    "body": [
        {"type": "minecraft:plain_message", "contents": {"text": "1. Be kind to other players."}},
        {"type": "minecraft:plain_message", "contents": {"text": "2. No griefing or stealing."}},
        {"type": "minecraft:plain_message", "contents": {"text": "3. Keep farms lag-friendly."}}
    ],
    "action": {
        "label": {"text": "Back"},
        "action": {"type": "minecraft:show_dialog", "dialog": "lobby:menu"}
    }
})

datapack.dialog_tag("minecraft:pause_screen_additions", ["lobby:menu"])
