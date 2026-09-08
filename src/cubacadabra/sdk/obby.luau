-- Cubacadabra obby SDK v1.
--
-- The runtime owns physics, ladder volumes, checkpoints, fall detection, and
-- respawn. This helper keeps the small amount of game-owned course state and
-- gives packages one place to react to the player lifecycle.

local CubaObby = {}
CubaObby.__index = CubaObby

function CubaObby.create(config)
    if type(config) ~= "table" then
        error("CubaObby.create requires a config table")
    end
    return setmetatable({
        checkpoint = "start",
        deaths = 0,
        state = "ready",
        onCheckpoint = config.onCheckpoint,
        onDeath = config.onDeath,
        onRespawn = config.onRespawn,
    }, CubaObby)
end

function CubaObby:handle(api, event)
    if type(event) ~= "table" or type(event.kind) ~= "string" then
        return false
    end
    if event.kind == "checkpoint" then
        self.checkpoint = event.id or self.checkpoint
        self.state = "running"
        if type(self.onCheckpoint) == "function" then
            self.onCheckpoint(api, event, self)
        end
        return true
    elseif event.kind == "death" then
        self.deaths = event.deaths or (self.deaths + 1)
        self.state = "dead"
        if type(self.onDeath) == "function" then
            self.onDeath(api, event, self)
        end
        return true
    elseif event.kind == "respawn" then
        self.checkpoint = event.checkpoint or self.checkpoint
        self.deaths = event.deaths or self.deaths
        self.state = "running"
        if type(self.onRespawn) == "function" then
            self.onRespawn(api, event, self)
        end
        return true
    end
    return false
end

function CubaObby:status()
    return self.state, self.checkpoint, self.deaths
end
