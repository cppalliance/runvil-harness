-- Standard-library replacements for a section VM.
--
-- The Engine runs this chunk while it hardens a fresh VM, after the `_G`
-- guard and before any other chunk captures a function this one replaces.
-- A run with the same seed, start time, and answers must produce the same
-- result, and four stock functions break that: `math.randomseed` with no
-- argument and `table.sort` both read the clock and a memory address, and
-- `tostring` and `string.format` print the address of a table, function,
-- thread, or userdata. Each one is replaced here by a function whose
-- result depends only on its arguments and on the VM's generator. The
-- chunk also holds the `table.concat` wrapper, which is no determinism
-- fix: it lets a value with `__tostring` join like a string.
--
-- The one chunk argument is a privileged capture, never a global:
-- `unaddressed(value)` returns the text Lua prints in front of the address
-- of a value it prints with one, its `__name` or its type name. For every
-- value Lua prints without an address it returns nil, and then the value's
-- `__tostring` when it has one.
local unaddressed = ...

-- Captured so author code that rebinds a global or a library field cannot
-- change what a replacement does.
local error, pcall, select, type = error, pcall, select, type
local getmetatable, setmetatable = getmetatable, setmetatable
local base_tostring = tostring
local tointeger = math.tointeger
local create, pack, unpack = table.create, table.pack, table.unpack
local find, gmatch, match, sub = string.find, string.gmatch, string.match, string.sub
-- These stock functions keep their stock names here. Lua names a function
-- in its argument error by the variable it was called through, so the
-- error reads `bad argument #1 to 'randomseed'`, as it does in stock Lua.
local random, randomseed = math.random, math.randomseed
local format = string.format
local concat = table.concat

-- A replacement must fail as the stock function does: with the stock
-- text, on the line of the author's call. So it runs its work, a stock
-- function included, in a function of this chunk under `pcall`. Lua then
-- puts this chunk's position on every failure Lua itself raises in that
-- work: a stock function's refusal of an argument, an index of a value
-- that takes none, a comparison of a number with a string. No failure of
-- the author's code carries that position: a metamethod raises with its
-- own position, with none, or with a value that is no string.

-- What Lua puts in front of the line number of a failure it raises in
-- this chunk, read from a failure raised here, so it follows the chunk's
-- name wherever the Engine names the chunk.
local _, here = pcall(function() error("", 1) end)
here = match(here, "^(.*:)%d+: $")

-- The outcome of `pcall(f, ...)`, where `f` is a function of this chunk.
-- A failure with this chunk's position loses it and is raised from the
-- line of the author's call. Any other failure is the author's and is
-- raised again as it is, at level 0, so `pcall` in author code catches
-- the same value and the failing line stays the metamethod's.
--
-- Call it from the replacement itself and never as a tail call: level 3
-- is then the author's call. When the author's call is itself a tail
-- call, `return math.randomseed(...)`, its frame is gone and level 3 is the
-- caller of the author's function. Stock Lua keeps that frame, because it
-- calls a C function without replacing the caller; every replacement the
-- Engine writes in Lua differs from stock Lua in this.
local function from_here(ok, ...)
  if ok then return ... end
  local failure = ...
  if type(failure) == "string" and sub(failure, 1, #here) == here then
    local message = match(failure, "^%d+: (.*)", #here + 1)
    if message ~= nil then error(message, 3) end
  end
  error(failure, 0)
end

-- math.randomseed
--
-- Loading the `math` library seeded the generator from the clock and an
-- address. This seed is fixed, so a VM the Engine never seeds draws the
-- same numbers each time; the Engine seeds each section VM after
-- construction, from the run's seed and the section entry's id.
randomseed(0, 0)

-- The stock function, called from this chunk and not as a tail call, so
-- its failure carries this chunk's position. It returns the two parts of
-- the seed it set.
local function seeded(...)
  local n1, n2 = randomseed(...)
  return n1, n2
end

-- With no argument, stock Lua seeds from the clock and an address. Here
-- the new seed is the generator's own next two draws, so the draws after
-- it are a function of the seed the Engine gave the VM and of the draws
-- taken so far. `random(0)` is one whole 64-bit draw. With arguments this
-- is the stock function: the same seeds, results, and argument errors.
function math.randomseed(...)
  if select("#", ...) == 0 then
    local n1 = random(0)
    local n2 = random(0)
    return randomseed(n1, n2)
  end
  local n1, n2 = from_here(pcall(seeded, ...))
  return n1, n2
end

-- tostring
--
-- Stock Lua prints a table, function, thread, or userdata that has no
-- `__tostring` as its `__name` or type name and its address, `table:
-- 0x55d0c0a1e2f0`. The address differs from one run to the next, so only
-- the name is printed. Every other value goes to the stock function, and
-- so does a value with `__tostring`, whose metamethod the stock function
-- calls.

-- The types whose values stock Lua can print with an address.
local ADDRESSED = { table = true, ["function"] = true, userdata = true, thread = true }

-- The stock function, called from this chunk and not as a tail call, so
-- its one failure, a `__tostring` that returns no string, carries this
-- chunk's position.
local function rendered(value)
  return (base_tostring(value))
end

local function tostring(...)
  local value = ...
  if value == nil and select("#", ...) == 0 then
    error("bad argument #1 to 'tostring' (value expected)", 2)
  end
  local kind = type(value)
  if ADDRESSED[kind] then
    local name = unaddressed(value)
    if name ~= nil then return name end
  elseif kind ~= "string" then
    -- A nil, boolean, or number has no metatable in a section VM, so the
    -- stock function cannot fail. A string has one, which an author can
    -- give a `__tostring`.
    return base_tostring(value)
  end
  return (from_here(pcall(rendered, value)))
end
_G.tostring = tostring

-- string.format
--
-- `%s` prints its argument as stock `tostring` does, address included.
-- `%p` prints nothing but an address, of a string too, so it is refused.
--
-- Whether a `%s` argument prints with an address is known only when the
-- stock function reaches it: the `__tostring` of an earlier argument is
-- the author's code, and it can set or clear the metatable of a later
-- one. So an argument that can print with an address is not looked at
-- before the call. A stand-in takes its place, and the stock function
-- asks the stand-in for its text when it reaches that conversion, after
-- every earlier one, as it would ask the argument.
--
-- The pattern takes one conversion the way the stock function reads it:
-- `%`, any run of flag, digit, and `.` characters, then the conversion
-- letter, captured. `%%` is the conversion whose letter is `%`, the only
-- one that takes no argument.
local CONVERSION = "%%[-+#0-9 .]*(.)"

-- Matches a format that may hold a `%s` or a `%p`. A format with neither
-- needs no look at its arguments. `%%s` matches too, and the walk below
-- then finds nothing to do.
local NEEDS_A_LOOK = "%%[-+#0-9 .]*[sp]"

-- The text of the argument a stand-in holds, as the Engine's `tostring`
-- gives it. The stock function checks that what comes back is a string,
-- as it checks what the argument's own `__tostring` returns.
--
-- A `__tostring` that is a function is called from here. Going through
-- the stock `tostring` would cost one more C call for each `string.format`
-- a `__tostring` is nested in, and Lua allows 200 of them.
local STAND_IN = {
  __tostring = function(stand_in)
    local value = stand_in[1]
    local name, rendering = unaddressed(value)
    if name ~= nil then return name end
    if type(rendering) == "function" then return rendering(value) end
    -- A `__tostring` that is no function: the stock function calls it or
    -- refuses it, and the failure carries this chunk's position.
    return (base_tostring(value))
  end,
}

-- The arguments of a call, where a stand-in takes the place of each
-- argument a `%s` can print with an address. Refuses a `%p`, with a
-- failure that carries this chunk's position, as every failure of the
-- stock function does.
--
-- The arguments are packed once, so the work is linear in their count;
-- reading the i-th of them with `select` would cost i steps each time.
local function with_names(fmt, ...)
  -- A number is a valid format and holds no conversion; any other type is
  -- left for the stock function to refuse.
  if type(fmt) ~= "string" or not find(fmt, NEEDS_A_LOOK) then return fmt, ... end
  local arguments = pack(...)
  local index = 0
  for letter in gmatch(fmt, CONVERSION) do
    if letter == "p" then
      error("string.format: conversion '%p' is not available, it prints a memory address")
    end
    if letter ~= "%" then
      index = index + 1
      if letter == "s" then
        -- Past the last argument this reads nil, which stays, and the
        -- stock function then refuses the missing value.
        local value = arguments[index]
        if ADDRESSED[type(value)] then arguments[index] = setmetatable({ value }, STAND_IN) end
      end
    end
  end
  return fmt, unpack(arguments, 1, arguments.n)
end

-- The whole of a call: the look at the arguments and the stock function,
-- which is called from this chunk and not as a tail call. Every failure
-- Lua raises in either carries this chunk's position.
local function formatted(...)
  -- With no argument at all, the stock function must see none.
  if select("#", ...) == 0 then return (format()) end
  return (format(with_names(...)))
end

-- The parentheses keep the call to `from_here` from being a tail call.
function string.format(...)
  return (from_here(pcall(formatted, ...)))
end

-- table.concat
--
-- Stock `table.concat` takes only strings and numbers. A value with a
-- `__tostring` metamethod (a fanout result object, an Engine userdata)
-- joins as `tostring` gives it. A plain table, a boolean, and nil still
-- raise as stock Lua does.
local function renders(v)
  local mt = getmetatable(v)
  return type(mt) == "table" and mt.__tostring ~= nil
end

-- Runs under `pcall`, so every failure Lua raises here, the stock
-- function's refusal of a separator included, is raised from the
-- caller's line.
local function joined(list, sep, i, j)
  i = i or 1
  j = j or #list
  local parts = {}
  for k = i, j do
    local v = list[k]
    local ty = type(v)
    if ty == "string" or ty == "number" then
      parts[#parts + 1] = v
    elseif ty == "userdata" then
      -- Engine userdata with __tostring. mlua metatables are not readable
      -- via getmetatable, so type-gate here.
      parts[#parts + 1] = tostring(v)
    elseif ty == "table" and renders(v) then
      -- Fanout result objects: plain tables under a __tostring metatable.
      parts[#parts + 1] = tostring(v)
    elseif v == nil then
      error("invalid value (nil) at index " .. k .. " in table for 'concat'")
    else
      error("invalid value (" .. ty .. ") at index " .. k .. " in table for 'concat'")
    end
  end
  return (concat(parts, sep))
end

function table.concat(...)
  return (from_here(pcall(joined, ...)))
end

-- table.sort
--
-- Stock `table.sort` is a quicksort that, once a partition comes out
-- unbalanced, picks its next pivot from the clock and an address. Elements
-- that compare equal then end in an order that differs from one run to the
-- next. This is a merge sort instead: it picks nothing, so the result is a
-- function of the list and the comparison, and it is stable, so elements
-- that compare equal keep the order they had. It makes at most
-- n * ceil(log2(n)) comparisons, where stock Lua's worst case is
-- quadratic, and it holds two more arrays of n slots while it runs, each
-- created at its full size so that neither grows.
--
-- `before(a, b)` is the comparison: true when `a` must come before `b`.
-- The elements are read once into an array, merged back and forth between
-- it and a second one in runs of `width`, 2 * `width`, and so on, and
-- written back once. A list behind `__index` and `__newindex` therefore
-- sees n reads and then n writes, and a sort whose comparison fails
-- leaves the list as it was.

-- The elements `list[1]` to `list[n]` as a new array.
local function read(list, n)
  local from = create(n)
  for i = 1, n do from[i] = list[i] end
  return from
end

-- Sorts the array `from` of `n` elements and returns the array that holds
-- the result, which is `from` or the second array.
local function merge(from, n, before)
  local to = create(n)
  local width = 1
  while width < n do
    for low = 1, n, 2 * width do
      -- The left run is [low, middle) and the right run is [middle, high).
      local middle = low + width
      if middle > n + 1 then middle = n + 1 end
      local high = middle + width
      if high > n + 1 then high = n + 1 end
      local left, right = low, middle
      for out = low, high - 1 do
        -- The right element goes first only when it must come before the
        -- left one. On a tie the left one goes first, which is what keeps
        -- equal elements in their order.
        if left < middle and (right >= high or not before(from[right], from[left])) then
          to[out] = from[left]
          left = left + 1
        else
          to[out] = from[right]
          right = right + 1
        end
      end
    end
    from, to = to, from
    width = 2 * width
  end
  return from
end

-- Writes the `n` elements of `sorted` to `list[1]` to `list[n]`.
local function write(list, n, sorted)
  for i = 1, n do list[i] = sorted[i] end
end

-- The order of a sort with no comparison function.
local function less(a, b)
  return a < b
end

-- The length of `list`, which a `__len` metamethod gives when there is
-- one. It runs under `pcall`, so a `__len` that cannot be called fails on
-- the caller's line.
local function length(list)
  return #list
end

-- Argument errors read as the stock function's. The length is converted
-- as the stock function converts it, so a `__len` may return a float or a
-- string with an integer value. A comparison function is checked only
-- when there is something to compare, as in stock Lua.
function table.sort(...)
  local list, comp = ...
  if type(list) ~= "table" then
    local got = select("#", ...) == 0 and "no value" or type(list)
    error("bad argument #1 to 'sort' (table expected, got " .. got .. ")", 2)
  end
  local n = tointeger((from_here(pcall(length, list))))
  if n == nil then
    error("object length is not an integer", 2)
  end
  if n < 2 then return end
  if n >= 0x7fffffff then
    error("bad argument #1 to 'sort' (array too big)", 2)
  end
  if comp ~= nil and type(comp) ~= "function" then
    error("bad argument #2 to 'sort' (function expected, got " .. type(comp) .. ")", 2)
  end
  -- The reads, the writes, and `less` run under `pcall`, so a failure
  -- Lua raises in them is raised from the caller's line. The stock
  -- function, which is C, raises such a failure with no position.
  local sorted = from_here(pcall(read, list, n))
  if comp ~= nil then
    -- No protected call here: a failure the comparison function raises
    -- must leave from its own frame, which is where the Engine reads the
    -- failing line.
    sorted = merge(sorted, n, comp)
  else
    sorted = from_here(pcall(merge, sorted, n, less))
  end
  from_here(pcall(write, list, n, sorted))
end
