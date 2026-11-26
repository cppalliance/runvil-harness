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
-- The Engine compiles this chunk with no debug info, so a function of
-- it has no line, as the C function it replaces has none. The author's
-- code counts on that: `error(text, 2)` in a metamethod puts in front of
-- `text` the position of the function that called the metamethod, which
-- is nothing for a C function, and `error(text, 3)` the position of the
-- author's call. So a replacement must be one frame with no position
-- between the author's call and the author's code. Each replacement
-- therefore calls a metamethod or a comparison function from its own
-- body, never from a function it calls, and a function of this chunk
-- that is no replacement runs none of the author's code.
--
-- The chunk arguments are privileged captures, never globals. The first
-- five read a metamethod as Lua finds it, which Lua code cannot do for a
-- userdata or for a table whose metatable `__metatable` covers.
--
-- `unaddressed(value)` takes a value Lua can print with an address, a
-- table, function, userdata, or thread, and returns two values, of which
-- at most one is set. The second is the `__tostring` of a value that has
-- one. The first is the text Lua prints in front of the address of a
-- value with none, where that is not the name of its type: its `__name`.
-- Neither is set for a value with no `__tostring` and no `__name`, which
-- prints as the name `type` gives.
--
-- `unmeasured(list)` returns the text Lua refuses `#list` with when the
-- `__len` of `list` is one it cannot call, for a table or a value that
-- has a `__len`, and nil otherwise.
--
-- `unordered(a, b)` returns the text Lua refuses `a < b` with when the
-- `__lt` of the two is one it cannot call, for two values that are not
-- both numbers and not both strings, and nil otherwise.
--
-- `mistyped(value, expected, ...)` returns the name stock Lua gives the
-- type of `value` when a stock function refuses it as an argument of the
-- type `expected`, and nil when it takes it: a value of that type, or a
-- value whose metatable holds each metamethod named after it, which is
-- how a stock function of `table` takes a value that is no table as its
-- list. With no metamethod named, only a value of the type is taken.
--
-- `bare(list)` is true for a table with no metatable. Lua takes the
-- length of such a table and reads and writes its items with none of
-- the author's code, and refuses none of the three.
--
-- `stock` is the table `__impl_stdlib_stock.lua` returns: the stock
-- functions, each in a function that a replacement calls under `pcall`,
-- `from_here` and `part_outcome`, which raise the failure of such a
-- call from the author's line, and `raised_here`, which tells whether
-- such a failure is the stock function's own.
--
-- `argument_error(level, function, argument, reason)` returns the text
-- stock Lua refuses an argument with, for the replacement at `level`,
-- where 1 is the function that calls it: `function` is the replacement's
-- name in the library, `argument` the number of the argument, and
-- `reason` what stock Lua says of it. The text names the replacement as
-- the author's call is written: by the variable or the field of the
-- call, and by `function` under a caller that is no Lua code, such as
-- `pcall`. A function of this chunk that runs a metamethod is such a
-- caller too, as the C function it stands in for is.
--
-- `strings` is the one metatable that all strings share, which no code
-- of a section VM can put another in the place of, and nil in a VM with
-- no `string` library. The chunk reads the `__tostring` of a string from
-- it with `rawget`, as Lua finds a metamethod, which costs no call of
-- the Engine for the commonest argument of `%s`.
local unaddressed, unmeasured, unordered, mistyped, bare, stock, argument_error, strings = ...

-- Captured so author code that rebinds a global or a library field cannot
-- change what a replacement does. The Engine takes `rawget` out of the
-- globals once this chunk has loaded.
local error, pcall, select, type = error, pcall, select, type
local rawget, setmetatable = rawget, setmetatable
local base_tostring = tostring
local tointeger = math.tointeger
local create, move, pack = table.create, table.move, table.pack
local find, gmatch, sub = string.find, string.gmatch, string.sub
local random, randomseed = math.random, math.randomseed
local concat = table.concat
local from_here, part_outcome = stock.from_here, stock.part_outcome
local raised_here = stock.raised_here
local seeded, joined = stock.seeded, stock.joined
local plain_format, stock_format = stock.plain_format, stock.stock_format

-- A replacement must fail as the stock function does: with the stock
-- text, on the line of the author's call. It must also run the author's
-- code, a metamethod or a comparison function, as the stock function
-- does: under no protected call of its own. A failure of that code then
-- reaches the handler of the author's `xpcall`, and the Engine's own
-- handler, at the place of the raise, with the frames of that code in
-- place and its variables still open.
--
-- So the work of a replacement is of two kinds.
--
-- Work that can run the author's code runs in the open, in the body of
-- the replacement. Lua raises its own refusal of such work, a read of a
-- value that takes none or two values with no order, in the
-- replacement's frame. With no debug info in this chunk the text is the
-- stock function's with `?:?:` in front, which the Engine takes off
-- wherever it reads a failure. One refusal has other words in a Lua
-- function than in a C function: of a `__len` or a `__lt` that cannot
-- be called Lua says which metamethod it was, as in `attempt to call a
-- number value (metamethod 'len')`. The chunk therefore asks the Engine
-- first whether Lua can call that metamethod, and raises the refusal
-- itself, in the stock function's words.
--
-- Work that runs none of the author's code, a stock function over
-- values whose metamethods it does not use, runs in a function of
-- `stock` under `pcall`. Lua puts that chunk's position on every failure
-- it raises there, and `from_here` moves the failure to the author's
-- line. Call `from_here` and `part_outcome` from the replacement itself
-- and never as a tail call: they count the replacement as one frame.

-- math.randomseed
--
-- Loading the `math` library seeded the generator from the clock and an
-- address. This seed is fixed, so a VM the Engine never seeds draws the
-- same numbers each time; the Engine seeds each section VM after
-- construction, from the run's seed and the section entry's id.
randomseed(0, 0)

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
  local n1, n2 = from_here("math.randomseed", pcall(seeded, ...))
  return n1, n2
end

-- tostring
--
-- Stock Lua prints a table, function, thread, or userdata that has no
-- `__tostring` as its `__name` or type name and its address, `table:
-- 0x55d0c0a1e2f0`. The address differs from one run to the next, so only
-- the name is printed. A value with `__tostring` is the text its
-- metamethod gives, and a nil, boolean, or number goes to the stock
-- function.

-- The types whose values stock Lua can print with an address.
local ADDRESSED = { table = true, ["function"] = true, userdata = true, thread = true }

-- The metatable of strings, and an empty table where strings have none,
-- which then holds no `__tostring`.
local STRINGS = strings or {}

-- What a `__tostring` metamethod returned, as the stock function takes
-- it: a string, or a number as its text. Nil for any other value, which
-- the stock function refuses.
local function text_of(result)
  local kind = type(result)
  if kind == "string" then return result end
  if kind == "number" then return base_tostring(result) end
  return nil
end

-- The metamethod is called here, in the body, and the result is no tail
-- call, so this function's frame is the one between the author's call
-- and the metamethod.
local function tostring(...)
  local value = ...
  if value == nil and select("#", ...) == 0 then
    error(argument_error(1, "tostring", 1, "value expected"), 2)
  end
  local kind = type(value)
  local name, rendering
  if kind == "string" then
    -- A string is its own text, unless the author gave the string
    -- metatable a `__tostring`.
    rendering = rawget(STRINGS, "__tostring")
    if rendering == nil then return value end
  elseif ADDRESSED[kind] then
    name, rendering = unaddressed(value)
    if rendering == nil then return name or kind end
  else
    -- A nil, boolean, or number has no metatable in a section VM, so the
    -- stock function runs no metamethod and cannot fail.
    return base_tostring(value)
  end
  return text_of(rendering(value)) or error("'__tostring' must return a string", 2)
end
_G.tostring = tostring

-- string.format
--
-- `%s` prints its argument as stock `tostring` does, address included,
-- and it calls the argument's `__tostring`, which is the author's code.
-- `%p` prints nothing but an address, of a string too, so it is refused.
--
-- The replacement makes the text of such an argument itself, by a call
-- of the `__tostring` from its own body. The stock function gets a
-- stand-in that holds the text, and runs under `pcall` with none of the
-- author's code left to run.
--
-- The stock function takes the conversions in order and stops at the
-- first one it refuses. A `__tostring` therefore runs only when every
-- conversion in front of it is accepted, and it can set or clear the
-- metatable of a later argument. The replacement keeps that order. It
-- walks the conversions, and before it runs a `__tostring` it gives the
-- stock function the part of the format in front of that conversion.
-- The results of the parts, joined, are the result of the whole format:
-- the stock function reads a format from left to right and keeps
-- nothing from one conversion to the next.
--
-- The pattern takes one conversion the way the stock function reads it:
-- `%`, any run of flag, digit, and `.` characters, then the conversion
-- letter. It captures where the conversion starts, the letter, and where
-- the conversion ends. `%%` is the one conversion that takes no
-- argument.
local CONVERSION = "()%%[-+#0-9 .]*(.)()"

-- Matches a format that may hold a `%s` or a `%p`. A format with neither
-- needs no look at its arguments. `%%s` matches too, and the walk then
-- finds nothing to do.
local NEEDS_A_LOOK = "%%[-+#0-9 .]*[sp]"

-- The stock function refuses a conversion of this many bytes, `%` and
-- letter included, or more, before it looks at the argument. Such a `%s`
-- is left to it, and no `__tostring` runs.
local TOO_LONG = 23

-- The metatable of a stand-in, which holds the text of a `%s` argument.
-- The stock function asks the stand-in for its text, as it would ask the
-- argument, and asks nothing of the string metatable about that text.
local HELD = { __tostring = function(held) return held[1] end }

function string.format(...)
  local fmt = ...
  -- A number is a valid format and holds no conversion; any other type is
  -- left for the stock function to refuse.
  if type(fmt) ~= "string" or not find(fmt, NEEDS_A_LOOK) then
    return (from_here("string.format", pcall(plain_format, ...)))
  end
  if find(fmt, "p", 1, true) then
    for _, letter in gmatch(fmt, CONVERSION) do
      if letter == "p" then
        error("string.format: conversion '%p' is not available, it prints a memory address", 2)
      end
    end
  end
  -- The arguments are packed once, the format among them, so each has
  -- the number the stock function gives it and the work is linear in
  -- their count.
  local arguments = pack(...)
  -- The results of the parts given to the stock function so far. The
  -- part not yet given starts at byte `from` of the format, and its
  -- first argument is `arguments[first]`.
  local parts, count = {}, 0
  local from, first = 1, 2
  local number = 1
  for at, letter, after in gmatch(fmt, CONVERSION) do
    if letter ~= "%" or after - at > 2 then
      number = number + 1
      -- Past the last argument this reads nil, which stays, and the
      -- stock function then refuses the missing value.
      local value = arguments[number]
      local kind = type(value)
      if letter == "s" and after - at < TOO_LONG and (kind == "string" or ADDRESSED[kind]) then
        local name, rendering
        if kind == "string" then
          rendering = rawget(STRINGS, "__tostring")
        else
          name, rendering = unaddressed(value)
          if rendering == nil then name = name or kind end
        end
        if name ~= nil then
          arguments[number] = setmetatable({ name }, HELD)
        elseif rendering ~= nil then
          if at > from then
            count = count + 1
            parts[count] = part_outcome(first,
              pcall(stock_format, sub(fmt, from, at - 1), arguments, first, number - 1))
          end
          local text = text_of(rendering(value)) or error("'__tostring' must return a string", 2)
          arguments[number] = setmetatable({ text }, HELD)
          from, first = at, number
        end
      end
    end
  end
  local last = part_outcome(first,
    pcall(stock_format, from == 1 and fmt or sub(fmt, from), arguments, first, arguments.n))
  if count == 0 then return last end
  parts[count + 1] = last
  return concat(parts)
end

-- table.concat
--
-- Stock `table.concat` takes only strings and numbers. A value with a
-- `__tostring` metamethod (a fanout result object, an Engine userdata)
-- joins as `tostring` gives it, and a userdata with none joins as its
-- name. A plain table, a boolean, and nil still raise as stock Lua does.
--
-- The rest is the stock function's order of work: the list, its length,
-- the separator, the two bounds, then one element after the other.
--
-- The length and each element are read in the open as plain Lua, which
-- runs a `__len` and an `__index` of the list. A call that waits
-- therefore works inside either. The `__tostring` of an element is
-- called the same way.
--
-- A list with no metatable goes to the stock function first, which runs
-- none of the author's code over it. The stock function takes it when
-- every element is a string or a number. When it refuses the list,
-- nothing has happened that the author's code can see, and the work
-- starts again here, which takes a value with `__tostring` or fails as
-- the stock function does, on the author's line. A failure that is not
-- the stock function's refusal ends the call.

-- A list with no element, for a call of a stock function that must get
-- no further than its arguments.
local EMPTY = {}

function table.concat(...)
  local list, sep, i, j = ...
  local got = mistyped(list, "table", "__index", "__len")
  if got ~= nil then
    if select("#", ...) == 0 then got = "no value" end
    error(argument_error(1, "table.concat", 1, "table expected, got " .. got), 2)
  end
  if bare(list) then
    local ok, result = pcall(joined, list, sep, i, j)
    if ok then return result end
    -- Only the stock function's own refusal starts the work again. Any
    -- other failure, such as the abort of a cancelled run, is raised as
    -- it is.
    if not raised_here(result) then error(result, 0) end
  end
  local refusal = unmeasured(list)
  if refusal ~= nil then error(refusal, 0) end
  local n = tointeger(#list)
  if n == nil then
    error("object length is not an integer", 2)
  end
  -- The stock function refuses a separator that is no string or number
  -- and a bound that is no integer. Over the empty list and that one
  -- argument it gets as far as the refusal, which is then raised from
  -- the author's line.
  local kind = type(sep)
  if sep ~= nil and kind ~= "string" and kind ~= "number" then
    from_here("table.concat", pcall(joined, EMPTY, sep))
  end
  if i == nil then
    i = 1
  else
    i = tointeger(i) or from_here("table.concat", pcall(joined, EMPTY, nil, i))
  end
  if j == nil then
    j = n
  else
    j = tointeger(j) or from_here("table.concat", pcall(joined, EMPTY, nil, nil, j))
  end
  local parts, count = {}, 0
  for k = i, j do
    local v = list[k]
    local ty = type(v)
    count = count + 1
    if ty == "string" or ty == "number" then
      parts[count] = v
    elseif ty == "table" or ty == "userdata" then
      local name, rendering = unaddressed(v)
      if rendering ~= nil then
        parts[count] = text_of(rendering(v)) or error("'__tostring' must return a string", 2)
      elseif ty == "userdata" then
        parts[count] = name or ty
      else
        error("invalid value (table) at index " .. k .. " in table for 'concat'", 2)
      end
    else
      error("invalid value (" .. ty .. ") at index " .. k .. " in table for 'concat'", 2)
    end
  end
  return (from_here("table.concat", pcall(joined, parts, sep)))
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
-- The elements are read once into an array, merged back and forth between
-- it and a second one in runs of `width`, 2 * `width`, and so on, and
-- written back once. A list behind `__index` and `__newindex` therefore
-- sees n reads and then n writes, and a sort whose comparison fails
-- leaves the list as it was.
--
-- The whole sort is the body of one function. The length, the reads, the
-- comparisons, and the writes can each run the author's code, and each
-- must run it from the replacement's own frame.

-- Whether the `n` elements of the array `from` are all numbers or all
-- strings. Lua compares two of either with no metamethod and refuses
-- none.
local function is_plain(from, n)
  local kind = type(from[1])
  if kind ~= "number" and kind ~= "string" then return false end
  for k = 2, n do
    if type(from[k]) ~= kind then return false end
  end
  return true
end

-- The list is a table, or a value whose metatable holds `__index`,
-- `__newindex`, and `__len`, as the stock function takes one. The length
-- is converted as the stock function converts it, so a `__len` may
-- return a float or a string with an integer value. A comparison
-- function is checked only when there is something to compare, as in
-- stock Lua.
--
-- A list with no metatable is read and written through stock
-- `table.move`, which runs none of the author's code over it. Any other
-- list is read and written one item at a time as plain Lua, which runs
-- its `__index` and `__newindex`. The comparison function can give the
-- list a metatable or take it away, so the list is looked at again
-- before the writes.
--
-- With no comparison function the order is `a < b`. Two numbers and two
-- strings compare with no metamethod. For any other pair `a < b` runs
-- the `__lt` of the author's values, and the Engine says first whether
-- Lua can call it.
function table.sort(...)
  local list, comp = ...
  local got = mistyped(list, "table", "__index", "__newindex", "__len")
  if got ~= nil then
    if select("#", ...) == 0 then got = "no value" end
    error(argument_error(1, "table.sort", 1, "table expected, got " .. got), 2)
  end
  local refusal = unmeasured(list)
  if refusal ~= nil then error(refusal, 0) end
  local n = tointeger(#list)
  if n == nil then
    error("object length is not an integer", 2)
  end
  if n < 2 then return end
  if n >= 0x7fffffff then
    error(argument_error(1, "table.sort", 1, "array too big"), 2)
  end
  if comp ~= nil then
    got = mistyped(comp, "function")
    if got ~= nil then
      error(argument_error(1, "table.sort", 2, "function expected, got " .. got), 2)
    end
  end
  local from = create(n)
  if bare(list) then
    move(list, 1, n, 1, from)
  else
    for k = 1, n do
      from[k] = list[k]
    end
  end
  local plain = comp == nil and is_plain(from, n)
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
        local right_first = false
        if left >= middle then
          right_first = true
        elseif right < high then
          local a, b = from[right], from[left]
          if comp ~= nil then
            right_first = comp(a, b)
          elseif plain then
            right_first = a < b
          else
            local kind = type(a)
            if kind ~= type(b) or (kind ~= "number" and kind ~= "string") then
              refusal = unordered(a, b)
              if refusal ~= nil then error(refusal, 0) end
            end
            right_first = a < b
          end
        end
        if right_first then
          to[out] = from[right]
          right = right + 1
        else
          to[out] = from[left]
          left = left + 1
        end
      end
    end
    from, to = to, from
    width = 2 * width
  end
  if bare(list) then
    move(from, 1, n, 1, list)
  else
    for k = 1, n do
      list[k] = from[k]
    end
  end
end
