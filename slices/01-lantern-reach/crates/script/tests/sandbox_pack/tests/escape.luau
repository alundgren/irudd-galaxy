-- Sandbox escape attempts. Each test passes if the sandbox holds.
local function fails(f: () -> ()): boolean
	local ok = pcall(f)
	return not ok
end

return {
	no_clock = function()
		assert(os == nil, "no os library")
		assert((tick :: any) == nil and (time :: any) == nil and (wait :: any) == nil, "no timer globals")
	end,
	no_unseeded_random = function()
		assert(math.random == nil and math.randomseed == nil)
	end,
	no_code_loading = function()
		assert((loadstring :: any) == nil and (getfenv :: any) == nil and (setfenv :: any) == nil)
		assert((io :: any) == nil and (debug :: any) == nil and (collectgarbage :: any) == nil)
	end,
	globals_are_frozen = function()
		assert(fails(function()
			(_G :: any).leaked = 1
		end), "_G is read-only")
		assert(fails(function()
			local env = (getmetatable :: any)
			leaked_global = env
		end), "assigning a global fails")
		assert(fails(function()
			(string :: any).upper = nil
		end), "libraries are read-only")
	end,
	require_stays_in_the_pack = function()
		assert(fails(function()
			require("@std/fs")
		end))
		assert(fails(function()
			require("../../../outside")
		end))
		assert(require("../scripts/helper").answer == 42)
	end,
	memory_is_limited = function()
		-- (`string.rep` of 200 MB would run over the step budget first: it charges per
		-- byte. Doubling by concatenation reaches 32 MB in a few steps.)
		assert(fails(function()
			local s = string.rep("x", 1024 * 1024)
			for _ = 1, 5 do
				s = s .. s
			end
		end), "a 32 MB string is over the pack's limit")
	end,
	budget_survives_pcall = function()
		-- Even caught, the budget error recurs at every step, so this test must fail.
		-- (Checked from Rust: this one is expected to be cut off.)
		while true do
			pcall(function()
				while true do
				end
			end)
		end
	end,
}
