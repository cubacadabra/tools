-- Cubacadabra survival SDK v1.
--
-- The runtime owns health, hazards, fall detection, and respawn. This helper
-- keeps the small amount of game-owned lifecycle state that most survival
-- games need without prescribing a particular objective or HUD.

local CubaSurvival = {}
CubaSurvival.__index = CubaSurvival

function CubaSurvival.create(config)
    if type(config) ~= "table" then
        error("CubaSurvival.create requires a config table")
    end
    local max_health = config.maxHealth or 100
    return setmetatable({
        health = config.startHealth or max_health,
        max_health = max_health,
        deaths = 0,
        state = "ready",
        source = nil,
        onDamage = config.onDamage,
        onDeath = config.onDeath,
        onRespawn = config.onRespawn,
    }, CubaSurvival)
end

function CubaSurvival:handle(api, event)
    if type(event) ~= "table" or type(event.kind) ~= "string" then
        return false
    end
    if event.kind == "damage" then
        self.health = event.health or self.health
        self.max_health = event.maxHealth or self.max_health
        self.source = event.source or self.source
        self.state = "exposed"
        if type(self.onDamage) == "function" then
            self.onDamage(api, event, self)
        end
        return true
    elseif event.kind == "death" then
        self.health = event.health or 0
        self.max_health = event.maxHealth or self.max_health
        self.deaths = event.deaths or (self.deaths + 1)
        self.state = "dead"
        if type(self.onDeath) == "function" then
            self.onDeath(api, event, self)
        end
        return true
    elseif event.kind == "respawn" then
        self.health = event.health or self.max_health
        self.max_health = event.maxHealth or self.max_health
        self.deaths = event.deaths or self.deaths
        self.source = nil
        self.state = "running"
        if type(self.onRespawn) == "function" then
            self.onRespawn(api, event, self)
        end
        return true
    end
    return false
end

function CubaSurvival:status()
    return self.state, self.health, self.max_health, self.deaths, self.source
end
