companion = {}

local state = "idle"
local width = 240
local height = 240
local elapsed = 0

local function block(ctx, x, y, w, h, color)
  ctx.rect(x, y, w, h, color[1], color[2], color[3], color[4] or 255)
end

local function eye(ctx, x, y, closed, color)
  if closed then
    block(ctx, x, y + 6, 20, 5, color)
  else
    block(ctx, x, y, 20, 20, color)
    block(ctx, x + 6, y + 4, 6, 8, { 255, 255, 255, 210 })
  end
end

function companion.load(ctx)
  width = ctx.width
  height = ctx.height
end

function companion.state_changed(event)
  state = event.state
end

function companion.update(dt)
  elapsed = elapsed + dt
end

function companion.draw(ctx)
  ctx.clear(0, 0, 0, 0)

  local bounce = 0
  if state == "happy" then
    bounce = -math.abs(math.sin(ctx.time * 7)) * 16
  elseif state == "error" then
    bounce = math.sin(ctx.time * 22) * 5
  end

  local ox = width * 0.5 - 75 + bounce
  local oy = height * 0.5 - 72 + bounce * 0.25
  local outline = { 26, 34, 50, 255 }
  local body = { 88, 205, 170, 255 }
  local light = { 160, 255, 220, 255 }
  local dark = { 28, 96, 90, 255 }
  local face = { 20, 31, 44, 255 }

  if state == "error" then
    body = { 235, 80, 95, 255 }
    light = { 255, 165, 150, 255 }
  elseif state == "sleeping" then
    body = { 92, 110, 150, 230 }
    light = { 145, 165, 205, 230 }
  elseif state == "listening" then
    local pulse = math.floor(math.abs(math.sin(ctx.time * 7)) * 70)
    ctx.rect(ox - 14, oy - 14, 178, 178, 60, 235, 200, pulse)
  end

  -- Antenna and ears.
  block(ctx, ox + 70, oy - 20, 10, 25, outline)
  block(ctx, ox + 65, oy - 28, 20, 12, light)
  block(ctx, ox - 10, oy + 25, 20, 48, outline)
  block(ctx, ox + 140, oy + 25, 20, 48, outline)
  block(ctx, ox - 4, oy + 32, 14, 30, light)
  block(ctx, ox + 140, oy + 32, 14, 30, light)

  -- Body, feet, and screen face.
  block(ctx, ox, oy, 150, 140, outline)
  block(ctx, ox + 8, oy + 8, 134, 124, body)
  block(ctx, ox + 20, oy + 25, 110, 80, dark)
  block(ctx, ox + 28, oy + 33, 94, 64, face)
  block(ctx, ox + 18, oy + 140, 38, 12, outline)
  block(ctx, ox + 94, oy + 140, 38, 12, outline)

  local blink = state == "sleeping" or (state == "idle" and math.floor(ctx.time * 2.2) % 9 == 0)
  if state == "happy" then
    block(ctx, ox + 43, oy + 55, 18, 5, light)
    block(ctx, ox + 89, oy + 55, 18, 5, light)
    block(ctx, ox + 56, oy + 77, 38, 6, light)
    block(ctx, ox + 62, oy + 83, 26, 5, light)
  else
    eye(ctx, ox + 40, oy + 48, blink, light)
    eye(ctx, ox + 90, oy + 48, blink, light)

    if state == "speaking" then
      local mouth = 8 + math.floor(math.abs(math.sin(ctx.time * 12)) * 20)
      block(ctx, ox + 63, oy + 77, 24, mouth, light)
    elseif state == "error" then
      block(ctx, ox + 62, oy + 82, 10, 6, light)
      block(ctx, ox + 72, oy + 76, 10, 6, light)
      block(ctx, ox + 82, oy + 82, 10, 6, light)
      block(ctx, ox + 125, oy + 22, 7, 20, { 135, 220, 255, 230 })
    else
      block(ctx, ox + 62, oy + 80, 26, 6, light)
    end
  end

  if state == "thinking" then
    local phase = math.floor(ctx.time * 4) % 3
    for i = 0, 2 do
      local size = i == phase and 10 or 6
      block(ctx, ox + 48 + i * 24, oy + 112, size, size, light)
    end
  elseif state == "happy" then
    block(ctx, ox - 18, oy + 5, 8, 8, { 255, 240, 110, 255 })
    block(ctx, ox + 160, oy + 12, 8, 8, { 255, 240, 110, 255 })
    block(ctx, ox + 168, oy + 80, 6, 6, { 255, 240, 110, 230 })
  elseif state == "sleeping" then
    ctx.set_color(180, 205, 255, 230)
    ctx.draw_text("z", ox + 132, oy + 20)
    ctx.draw_text("Z", ox + 148, oy + 4)
  end
end

function companion.hit_test(x, y)
  local cx = width * 0.5
  local cy = height * 0.5
  local dx = x - cx
  local dy = y - cy
  return dx * dx + dy * dy <= 108 * 108
end
