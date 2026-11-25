-- The stock functions that a standard-library replacement calls, and the
-- move of their failures to the author's line.
--
-- The replacements live in `__impl_stdlib.lua`, which the Engine compiles
-- with no debug info. This chunk keeps its own: Lua puts this chunk's
-- position on every failure it raises in a function of it, which is how
-- `raised_here` tells such a failure from any other.
--
-- Every function here that calls a stock function is called under
-- `pcall` by a replacement, and runs none of the author's code: the
-- replacement has taken the values with a metamethod out of the
-- arguments first.
--
-- The Engine runs this chunk before `__impl_stdlib.lua`, so the stock
-- functions it captures are the stock ones. It returns the table of the
-- functions the replacements call.
--
-- The chunk argument is a privileged capture, never a global.
-- `argument_error(level, function, argument, reason)` returns the text
-- stock Lua refuses an argument with, for the replacement at `level`,
-- where 1 is the function that calls it: `function` is the replacement's
-- name in the library, `argument` the number of the argument, and
-- `reason` what stock Lua says of it. The text names the replacement as
-- the author's call is written.
local argument_error = ...

local error, pcall, type = error, pcall, type
local tointeger = math.tointeger
local unpack = table.unpack
local match, sub = string.match, string.sub
local randomseed = math.randomseed
local format = string.format
local concat = table.concat

-- What Lua puts in front of the line number of a failure it raises in
-- this chunk, read from a failure raised here, so it follows the chunk's
-- name wherever the Engine names the chunk.
local _, here = pcall(function() error("", 1) end)
here = match(here, "^(.*:)%d+: $")

-- Whether `failure`, which a `pcall` of a function of this chunk caught,
-- is one that Lua raised in a function of this chunk: a string with this
-- chunk's position in front. Any other failure, such as the abort of a
-- cancelled run, is not the stock function's own, and whoever caught it
-- raises it again as it is.
local function raised_here(failure)
  return type(failure) == "string" and sub(failure, 1, #here) == here
end

-- Raises `failure`, which a `pcall` of a function of this chunk caught,
-- for the replacement `name`, as in `table.concat`. A failure with this
-- chunk's position loses it and gets the position of the author's call.
-- Any other failure is raised again as it is.
--
-- A stock function names itself in an argument error by the variable of
-- this chunk that it was called through, and counts the arguments as
-- this chunk gave them. Such an error is made again with the name of the
-- author's call, and with `shift` added to its number.
--
-- Call it from a function that the replacement calls, and never as a
-- tail call. Counted as `error` counts, from 1 for this function, the
-- replacement is then at level 3 and the author's call at level 4.
local function raise_from_here(name, failure, shift)
  if raised_here(failure) then
    local message = match(failure, "^%d+: (.*)", #here + 1)
    if message ~= nil then
      local number, reason = match(message, "^bad argument #(%d+) to '[^']*' %((.*)%)$")
      if number ~= nil then
        message = argument_error(3, name, tointeger(number) + shift, reason)
      end
      error(message, 4)
    end
  end
  error(failure, 0)
end

-- The outcome of `pcall(f, ...)`, where `f` is a function of this chunk
-- that calls the stock function of the replacement `name`. A failure is
-- raised from the line of the author's call.
--
-- Call it from the replacement itself and never as a tail call: the
-- failure then gets the position of the author's call. When the author's
-- call is itself a tail call, `return math.randomseed(...)`, its frame is
-- gone and the position is that of the caller of the author's function.
-- Stock Lua keeps that frame, because it calls a C function without
-- replacing the caller; every replacement the Engine writes in Lua
-- differs from stock Lua in this.
local function from_here(name, ok, ...)
  if ok then return ... end
  raise_from_here(name, (...), 0)
end

-- Stock `math.randomseed`, called from this chunk and not as a tail call,
-- so its failure carries this chunk's position. It returns the two parts
-- of the seed it set.
local function seeded(...)
  local n1, n2 = randomseed(...)
  return n1, n2
end

-- Stock `string.format` over the author's own arguments, called from this
-- chunk and not as a tail call. With no argument at all, it sees none.
local function plain_format(...)
  return (format(...))
end

-- Stock `string.format` over `part`, which is a part of a format, and the
-- arguments `arguments[first]` to `arguments[last]`, called from this
-- chunk and not as a tail call. The arguments are unpacked in here, so
-- that a failure to unpack them is under the `pcall` too.
local function stock_format(part, arguments, first, last)
  return (format(part, unpack(arguments, first, last)))
end

-- The outcome of `pcall(stock_format, part, arguments, first, last)`.
-- The stock function counts the arguments of a part from 2, where the
-- author's call has `arguments[first]` at `first`, so the number in a
-- failure that names an argument is brought to the author's count.
-- Call it as `from_here` is called.
local function part_outcome(first, ok, ...)
  if ok then return ... end
  raise_from_here("string.format", (...), first - 2)
end

-- Stock `table.concat`, called from this chunk and not as a tail call.
-- Each call here gives it strings and numbers to join, or no element at
-- all.
local function joined(parts, sep, i, j)
  return (concat(parts, sep, i, j))
end

return {
  raised_here = raised_here,
  from_here = from_here,
  part_outcome = part_outcome,
  seeded = seeded,
  plain_format = plain_format,
  stock_format = stock_format,
  joined = joined,
}
