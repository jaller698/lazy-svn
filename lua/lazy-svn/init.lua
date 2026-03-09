local M = {}

--- Returns the absolute path to the lazySVN release binary.
--- The binary is located at `target/release/lazySVN` inside the plugin directory.
---@return string|nil
local function binary_path()
	local info = debug.getinfo(1, "S")
	local source = info and info.source
	if not source or source:sub(1, 1) ~= "@" then
		return nil
	end
	local plugin_dir = vim.fn.fnamemodify(source:sub(2), ":h:h:h")
	return plugin_dir .. "/target/release/lazysvn"
end

--- Open lazySVN in a Snacks terminal window.
--- The binary must have been built beforehand via the lazy.nvim build hook
--- (`cargo build --release`).
function M.open()
	local bin = binary_path()
	if not bin then
		vim.notify("lazySVN: could not determine plugin directory.", vim.log.levels.ERROR)
		return
	end
	if vim.fn.filereadable(bin) == 0 then
		vim.notify(
			"lazySVN: binary not found. Please rebuild the plugin with ':Lazy build lazy-svn'.",
			vim.log.levels.ERROR
		)
		return
	end
	local ok, snacks = pcall(require, "snacks")
	if not ok then
		vim.notify("lazySVN: snacks.nvim is required but not available.", vim.log.levels.ERROR)
		return
	end
	snacks.terminal(bin)
end

--- Setup the plugin.
--- Creates the `:LazySVN` user command.
---@param opts? table
function M.setup(opts)
	opts = opts or {}
	vim.api.nvim_create_user_command("LazySVN", M.open, { desc = "Open lazySVN TUI" })
end

return M
