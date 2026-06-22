companion = {}

local state = "idle"
local width = 140
local height = 140
local audio_level = 0

function companion.load(ctx)
  width = ctx.width
  height = ctx.height
end

function companion.state_changed(event)
  state = event.state
  audio_level = event.audio_level or 0
end

function companion.update(dt)
end

function companion.draw(ctx)
  ctx.clear(0, 0, 0, 0)

  local cx = width * 0.5
  local cy = height * 0.5
  local color = { 95, 190, 255, 245 }
  local radius = 10

  if state == "listening" then
    local ring = 25 + math.abs(math.sin(ctx.time * 5)) * 24
    ctx.circle(cx, cy, ring, 80, 230, 185, 45)
    ctx.circle(cx, cy, ring - 4, 0, 0, 0, 0)
    color = { 80, 230, 185, 255 }
    radius = 12
  elseif state == "thinking" then
    color = { 185, 120, 255, 255 }
    for i = 0, 5 do
      local angle = ctx.time * 4 + i * math.pi / 3
      local alpha = 60 + i * 30
      ctx.circle(cx + math.cos(angle) * 30, cy + math.sin(angle) * 30, 4,
        color[1], color[2], color[3], alpha)
    end
  elseif state == "speaking" then
    color = { 255, 180, 75, 255 }
    radius = 11 + math.max(audio_level, 0.35) * math.abs(math.sin(ctx.time * 11)) * 25
  elseif state == "happy" then
    color = { 255, 115, 185, 255 }
    radius = 14 + math.abs(math.sin(ctx.time * 6)) * 6
    for i = 0, 3 do
      local angle = i * math.pi * 0.5 + ctx.time
      ctx.circle(cx + math.cos(angle) * 34, cy + math.sin(angle) * 34, 3,
        255, 230, 120, 230)
    end
  elseif state == "error" then
    color = { 245, 65, 80, 255 }
    cx = cx + math.sin(ctx.time * 25) * 6
    ctx.set_color(255, 220, 220, 255)
    ctx.draw_text("!", cx - 3, cy - 32)
  elseif state == "sleeping" then
    color = { 85, 105, 150, 170 }
    radius = 7 + math.sin(ctx.time * 1.5)
  end

  ctx.circle(cx, cy, radius + 8, color[1], color[2], color[3], 40)
  ctx.circle(cx, cy, radius, color[1], color[2], color[3], color[4])
end

function companion.hit_test(x, y)
  local dx = x - width * 0.5
  local dy = y - height * 0.5
  return dx * dx + dy * dy <= 58 * 58
end
