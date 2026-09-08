# Survival SDK v1

The runtime owns gravity, fall detection, hazards, health, and respawn. A game
manifest declares those rules; the Luau helper only keeps lifecycle state for a
game-owned HUD or objective.

```luau
-- @include "@cubacadabra/survival-v1.luau"

local survival = CubaSurvival.create({})

function Game.on_player_event(api, event)
    survival:handle(api, event)
    local status = survival:status()
end
```

The helper begins in `waiting` state and becomes authoritative when it receives
the runtime `spawn` event. `status()` returns a named table with `state`,
`health`, `maxHealth`, `deaths`, and `source`. `damage` events expose `source`,
`amount`, `health`, and `maxHealth`; death, heal, and respawn events carry the
same authoritative health fields. Shelter timers, inventory, weather,
crafting, and victory conditions remain specific to the game.

World manifests may also declare `safeZones` with an optional
`healPerSecond`. A safe zone suppresses damage hazards while the player is
inside and emits `heal` lifecycle events. This is useful for campfires,
shelters, med-bays, and other reusable survival anchors.

After each lifecycle event it publishes a live `__player_state` snapshot with
`kind = "survival"`, health, max health, deaths, and alive state. Other players
receive this through `Game.on_network_message` as `event.type ==
"player_state"`. The snapshot is ephemeral connection state; use a separate
retained channel only for shared round state, and a future profile system for
durable progression.
