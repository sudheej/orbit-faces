companion = {}

local state = "idle"
local pulse = 0

function companion.load()
  state = "idle"
end

function companion.state_changed(next_state)
  state = next_state
end

function companion.update(dt)
  pulse = pulse + dt
end

function companion.draw(ctx)
  local cx = 110
  local cy = 110
  local base_radius = 72
  local wobble = math.sin(ctx.time * 3.0) * 4

  local outer = { 70, 160, 255, 220 }
  local inner = { 140, 220, 255, 245 }
  local accent = { 255, 255, 255, 230 }

  if state == "listening" then
    outer = { 40, 210, 150, 230 }
    inner = { 150, 255, 210, 250 }
    wobble = math.sin(ctx.time * 7.0) * 8
  elseif state == "thinking" then
    outer = { 185, 110, 255, 230 }
    inner = { 225, 190, 255, 250 }
    wobble = math.sin(ctx.time * 2.0) * 10
  elseif state == "speaking" then
    outer = { 255, 180, 70, 235 }
    inner = { 255, 230, 145, 255 }
    wobble = math.sin(ctx.time * 10.0) * 6
  end

  ctx.clear(0, 0, 0, 0)
  ctx.circle(cx, cy, base_radius + 14 + wobble, outer[1], outer[2], outer[3], 90)
  ctx.circle(cx, cy, base_radius + wobble, outer[1], outer[2], outer[3], outer[4])
  ctx.circle(cx - 15, cy - 18, 42, inner[1], inner[2], inner[3], inner[4])
  ctx.circle(cx + 28, cy + 24, 18, 255, 255, 255, 90)

  if state == "thinking" then
    local dot = 8 + math.sin(ctx.time * 5.0) * 3
    ctx.circle(cx - 24, cy + 44, dot, accent[1], accent[2], accent[3], accent[4])
    ctx.circle(cx, cy + 48, dot + 2, accent[1], accent[2], accent[3], accent[4])
    ctx.circle(cx + 28, cy + 44, dot, accent[1], accent[2], accent[3], accent[4])
  elseif state == "speaking" then
    local h = 10 + math.abs(math.sin(ctx.time * 12.0)) * 22
    ctx.rect(cx - 34, cy + 42 - h * 0.5, 14, h, 255, 255, 255, 210)
    ctx.rect(cx - 7, cy + 42 - h, 14, h * 2, 255, 255, 255, 230)
    ctx.rect(cx + 20, cy + 42 - h * 0.5, 14, h, 255, 255, 255, 210)
  elseif state == "listening" then
    ctx.circle(cx, cy + 42, 17 + math.sin(ctx.time * 8.0) * 4, 255, 255, 255, 180)
  else
    ctx.circle(cx, cy + 46, 15, 255, 255, 255, 160)
  end
end
