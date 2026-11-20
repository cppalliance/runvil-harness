-- The iterator of the Engine's `pairs`.
--
-- The Engine's `pairs` puts a table's keys in the walk order once, in
-- Rust, and hands them here as a plain list. The iterator is a Lua
-- function over that list, so all that a walk keeps lives in Lua's heap:
-- Lua counts it against the VM's memory limit and frees it when it
-- collects the iterator. Rust holds nothing of a walk between two steps.
--
-- It did once. `mlua` gives every Lua value a Rust value holds a slot on
-- one stack of about a million, and panics when it cannot get one. An
-- iterator in Rust held a slot for its table and for each string key, and
-- kept them until Lua collected it. Lua collects by how much its own heap
-- grew, and such an iterator added little to it, so several hundred
-- thousand of them were alive at once and a block that walked a small
-- table in a loop ran out of slots.

-- Captured before hardening removes the global. The table of a walk may
-- have an `__index` metamethod, which a walk does not call.
local rawget = rawget

-- The iterator of one walk over `data`: the keys in `order`, 1 to
-- `count`, each with its value. A key whose value is nil by the time the
-- walk reaches it is left out. `order` is a list the Engine made and has
-- no metatable.
return function(data, order, count)
  local position = 0
  return function()
    while position < count do
      position = position + 1
      local key = order[position]
      local value = rawget(data, key)
      if value ~= nil then return key, value end
    end
    -- The walk is over. Lua collects the iterator only later, so it lets
    -- go of the table and the keys now.
    data, order, count = nil, nil, 0
    return nil, nil
  end
end
