-- Standard-library replacements for a section VM.
--
-- The Engine runs this chunk while it hardens a fresh VM, after the `_G`
-- guard and before any other chunk captures a function this one replaces.
-- A run with the same seed, start time, and answers must produce the same
-- result, and stock `math.randomseed` with no argument breaks that: it
-- reads the clock and a memory address. It is replaced here by a function
-- whose result depends only on its arguments and on the VM's generator.

-- Captured so author code that rebinds a global or a library field cannot
-- change what a replacement does.
local error, pcall, select, type = error, pcall, select, type
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
