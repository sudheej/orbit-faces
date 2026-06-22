companion = {}

local state = "idle"
local width = 280
local height = 260
local audio_level = 0

local function rect(ctx, x, y, w, h, color)
  ctx.rect(x, y, w, h, color[1], color[2], color[3], color[4] or 255)
end

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

  local ox = width * 0.5 - 100
  local oy = height * 0.5 - 82
  local green = { 70, 245, 155, 255 }
  local dim = { 30, 125, 90, 230 }
  local frame = { 25, 35, 43, 255 }
  local panel = { 5, 17, 18, 248 }

  if state == "error" then
    green = { 255, 75, 85, 255 }
    dim = { 145, 35, 48, 235 }
  elseif state == "sleeping" then
    green = { 65, 105, 95, 180 }
    dim = { 30, 55, 52, 180 }
    panel = { 4, 10, 12, 210 }
  elseif state == "happy" then
    green = { 100, 255, 120, 255 }
  end

  -- Cube shadow, side panel, and terminal front.
  rect(ctx, ox + 18, oy + 18, 200, 160, { 0, 0, 0, 80 })
  rect(ctx, ox + 184, oy + 18, 28, 144, { 18, 50, 46, 245 })
  rect(ctx, ox + 196, oy + 8, 16, 144, { 28, 78, 68, 235 })
  rect(ctx, ox, oy, 190, 160, frame)
  rect(ctx, ox + 8, oy + 8, 174, 144, panel)
  rect(ctx, ox + 8, oy + 8, 174, 16, dim)

  ctx.set_color(green[1], green[2], green[3], green[4])
  ctx.draw_text("ORBITAL://FACE", ox + 14, oy + 12)

  -- Scanlines.
  for y = 32, 136, 8 do
    rect(ctx, ox + 12, oy + y, 166, 1, { dim[1], dim[2], dim[3], 65 })
  end

  if state == "idle" then
    ctx.set_color(green[1], green[2], green[3], 230)
    ctx.draw_text("> READY", ox + 18, oy + 50)
    rect(ctx, ox + 18, oy + 72, 64, 4, dim)
    rect(ctx, ox + 18, oy + 88, 112, 4, dim)
    rect(ctx, ox + 18, oy + 104, 88, 4, dim)
  elseif state == "listening" then
    ctx.set_color(green[1], green[2], green[3], 255)
    ctx.draw_text("> INPUT_", ox + 18, oy + 50)
    if math.floor(ctx.time * 3) % 2 == 0 then
      rect(ctx, ox + 90, oy + 48, 8, 12, green)
    end
    rect(ctx, ox + 18, oy + 82, 140, 2, dim)
  elseif state == "thinking" then
    ctx.set_color(green[1], green[2], green[3], 255)
    ctx.draw_text("COMPUTING", ox + 18, oy + 48)
    local phase = math.floor(ctx.time * 5) % 4
    for i = 0, 3 do
      local color = i == phase and green or dim
      rect(ctx, ox + 26 + i * 30, oy + 78, 14, 14, color)
    end
    ctx.draw_text("0x" .. tostring(math.floor(ctx.time * 17) % 99), ox + 18, oy + 112)
  elseif state == "speaking" then
    ctx.set_color(green[1], green[2], green[3], 255)
    ctx.draw_text("OUTPUT", ox + 18, oy + 42)
    local level = math.max(audio_level, 0.35)
    for i = 0, 7 do
      local h = 10 + math.abs(math.sin(ctx.time * 10 + i * 0.8)) * 48 * level
      rect(ctx, ox + 20 + i * 18, oy + 112 - h, 10, h, green)
    end
  elseif state == "happy" then
    ctx.set_color(green[1], green[2], green[3], 255)
    ctx.draw_text("[ OK ]", ox + 55, oy + 54)
    rect(ctx, ox + 52, oy + 88, 10, 10, green)
    rect(ctx, ox + 62, oy + 98, 10, 10, green)
    rect(ctx, ox + 72, oy + 88, 44, 10, green)
  elseif state == "error" then
    local jitter = math.floor(math.sin(ctx.time * 24) * 5)
    ctx.set_color(green[1], green[2], green[3], 255)
    ctx.draw_text("! ERROR", ox + 18 + jitter, oy + 46)
    ctx.draw_text("FAULT 0xDEAD", ox + 18 - jitter, oy + 72)
    for i = 0, 4 do
      rect(ctx, ox + 18 + ((i * 37) % 130), oy + 100 + i * 5, 28, 3, green)
    end
  elseif state == "sleeping" then
    ctx.set_color(green[1], green[2], green[3], green[4])
    ctx.draw_text("SUSPENDED", ox + 38, oy + 64)
    ctx.draw_text("zZ", ox + 126, oy + 92)
  end

  rect(ctx, ox + 30, oy + 166, 140, 8, frame)
  rect(ctx, ox + 65, oy + 174, 70, 8, frame)
end

function companion.hit_test(x, y)
  local cx = width * 0.5
  local cy = height * 0.5
  local dx = x - cx
  local dy = y - cy
  return dx * dx + dy * dy <= 120 * 120
end
