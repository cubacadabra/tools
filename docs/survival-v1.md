# Survival SDK v1

The runtime owns gravity, fall detection, hazards, health, and respawn. A game
manifest declares those rules; the Luau helper only keeps lifecycle state for a
game-owned HUD or objective.

```luau
-- @include "@cubacadabra/survival-v1.luau"

local survival = CubaSurvival.create({ maxHealth = 100, startHealth = 100 })

function Game.on_player_event(api, event)
    survival:handle(api, event)
    local state, health, maxHealth, deaths, source = survival:status()
end
```

`damage` events expose `source`, `amount`, `health`, and `maxHealth`. `death`
events expose `cause` and `deaths`; `respawn` events restore the configured
health values. Shelter timers, healing, inventory, weather, crafting, and
victory conditions remain specific to the game.
