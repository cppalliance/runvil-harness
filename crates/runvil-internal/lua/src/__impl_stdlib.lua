-- Standard-library replacements for a section VM.
--
-- The Engine runs this chunk while it hardens a fresh VM, after the `_G`
-- guard and before any other chunk captures a function this one replaces.
-- A run with the same seed, start time, and answers must produce the same
-- result, and two stock functions break that: `math.randomseed` with no
-- argument and `table.sort` both read the clock and a memory address. Each
-- one is replaced here by a function whose result depends only on its
-- arguments and on the VM's generator.

-- Captured so author code that rebinds a global or a library field cannot
-- change what a replacement does.
local error, pcall, select, type = error, pcall, select, type
local tointeger = math.tointeger
local create = table.create
local match, sub = string.match, string.sub
-- These stock functions keep their stock names here. Lua names a function
-- in its argument error by the variable it was called through, so the
-- error reads `bad argument #1 to 'randomseed'`, as it does in stock Lua.
local random, randomseed = math.random, math.randomseed

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
