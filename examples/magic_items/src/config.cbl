"""Item settings. Change these and rebuild the pack."""

# Blink Rod: dash up to this many blocks the way you face.
const BLINK_RANGE = 8
const BLINK_COOLDOWN_SECONDS = 3

# Healing Staff: heal everyone (you included) within this radius.
const HEAL_RADIUS = 6
const HEAL_COOLDOWN_SECONDS = 15

# Frost Wand: a ray that slows and damages the mobs it passes.
const FROST_RANGE = 12
const FROST_DAMAGE = 4
const FROST_COOLDOWN_SECONDS = 5

const BLINK_COOLDOWN_TICKS = BLINK_COOLDOWN_SECONDS * 20
const HEAL_COOLDOWN_TICKS = HEAL_COOLDOWN_SECONDS * 20
const FROST_COOLDOWN_TICKS = FROST_COOLDOWN_SECONDS * 20
