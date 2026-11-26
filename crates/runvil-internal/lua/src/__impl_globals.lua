-- The `_G` guard for a section VM, and the `setmetatable` and
-- `getmetatable` replacements that keep it in place.
--
-- The Engine runs this chunk first at VM construction, before hardening
-- strips the raw functions it captures and before any other chunk
-- captures `setmetatable` or `getmetatable`. The chunk arguments are
-- privileged captures, never globals: `globals` is the VM's globals table,
-- `state` is the Engine's slot table (`args` once injection installs the
-- read-only table of the chain's arguments,
-- `prose` once a block installs its render,
-- `messages` once injection installs the section's list, and `author` for
-- the metatable author code set on `_G`), `refuse_args()`,
-- `refuse_prose()`, and `refuse_messages()` raise the Engine's assignment
-- refusals, and `argument_error(level, function, argument, reason)`
-- returns the text stock Lua refuses an argument with, with the name that
-- the author's call of the function at `level` is written with. The
-- chunk returns the guard and the two replacements for the Engine to
-- install.
--
-- The Engine compiles this chunk with no debug info. Lua names the
-- variable that holds a value it cannot index, and with no debug info a
-- handler of the author's that it cannot index fails in Lua's words
-- alone, `attempt to index a number value`, with no variable of this
-- chunk after them.
local globals, state, refuse_args, refuse_prose, refuse_messages, argument_error = ...

local base_setmetatable, base_getmetatable = setmetatable, getmetatable
local rawequal, rawget, rawset = rawequal, rawget, rawset
local error, next, pcall, select, type = error, next, pcall, select, type
local tointeger = math.tointeger
local match = string.match

-- The guard is `_G`'s only metatable, and nothing hands it out: the base
-- `setmetatable` refuses to replace it, and the replacements below answer
-- for the author's metatable instead.
local guard = { __metatable = "_G is guarded" }

-- The guard's own fields. Every other field of the author's metatable
-- except `__mode` is copied onto the guard when the author sets it, so
-- `_G` keeps the author's other metamethods but never becomes weak.
local own = { __index = true, __newindex = true, __metatable = true }

-- `args`, `prose`, and `messages` never reach the author: `args` always
-- serves the table injection installed, and `messages` always serves the
-- section's one list. Every other key goes to the author's handler, read
-- raw from the author's metatable on each access the way Lua reads a
-- metamethod, and tail-called so the handler's error levels and yields
-- behave as if it were the metamethod.
function guard.__index(t, key)
  if key == "args" then
    return state.args
  end
  if key == "prose" then
    local render = state.prose
    if render == nil then return nil end
    return render()
  end
  if key == "messages" then
    return state.messages
  end
  local author = state.author
  local handler = author and rawget(author, "__index")
  if handler == nil then return nil end
  if type(handler) == "function" then return handler(t, key) end
  return handler[key]
end

function guard.__newindex(t, key, value)
  if key == "args" then
    return refuse_args()
  end
  if key == "prose" then
    return refuse_prose()
  end
  if key == "messages" then
    return refuse_messages()
  end
  local author = state.author
  local handler = author and rawget(author, "__newindex")
  if handler == nil then
    rawset(t, key, value)
  elseif type(handler) == "function" then
    return handler(t, key, value)
  else
    handler[key] = value
  end
end

local function forward(author)
  for key in next, guard do
    if not own[key] then guard[key] = nil end
  end
  if author == nil then return end
  for key, value in next, author do
    if not own[key] and key ~= "__mode" then guard[key] = value end
  end
end

-- A base function called through `pcall` cannot see the name it was
-- called by, so its argument error names it '?'. The error is made again
-- for the replacement `name`, with the name the author's call of it is
-- written with. Call it from the replacement itself.
local function named(message, name)
  local number, reason = match(message, "^bad argument #(%d+) to '%?' %((.*)%)$")
  if number == nil then return message end
  return argument_error(2, name, tointeger(number), reason)
end

-- Re-raising at level 2 puts the caller's position on the message, where
-- the base function's own error would have put it. Lua marks a value for
-- finalization only when its metatable already has `__gc` at this call, so
-- refusing it here keeps finalizers out for good.
local function replace_metatable(...)
  local target, metatable = ...
  if type(metatable) == "table" and rawget(metatable, "__gc") ~= nil then
    error("setmetatable: finalizers (__gc) are not available in the sandbox", 2)
  end
  if not rawequal(target, globals) then
    local ok, result = pcall(base_setmetatable, ...)
    if not ok then error(named(result, "setmetatable"), 2) end
    return result
  end
  if select("#", ...) < 2 or (metatable ~= nil and type(metatable) ~= "table") then
    local _, message = pcall(base_setmetatable, {}, select(2, ...))
    error(named(message, "setmetatable"), 2)
  end
  local author = state.author
  if author ~= nil and rawget(author, "__metatable") ~= nil then
    error("cannot change a protected metatable", 2)
  end
  if metatable ~= nil and rawget(metatable, "__mode") ~= nil then
    error("setmetatable: weak tables (__mode) are not available for _G", 2)
  end
  forward(metatable)
  state.author = metatable
  return target
end

local function read_metatable(...)
  if rawequal((...), globals) then
    local author = state.author
    if author == nil then return nil end
    local protected = rawget(author, "__metatable")
    if protected ~= nil then return protected end
    return author
  end
  local ok, result = pcall(base_getmetatable, ...)
  if not ok then error(named(result, "getmetatable"), 2) end
  return result
end

return guard, replace_metatable, read_metatable
