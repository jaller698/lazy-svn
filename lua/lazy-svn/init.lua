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

--- Open two files in a Neovim diff view.
--- This is called from the Rust binary via `nvim --remote-expr`.
---@param left string
---@param right string
---@param title string
---@return integer
function M.open_diff(left, right, title)
	if not left or not right then
		vim.notify("lazySVN: missing diff file paths.", vim.log.levels.ERROR)
		return 0
	end

	vim.schedule(function()
		vim.cmd("tabnew " .. vim.fn.fnameescape(left))
		vim.cmd("vert diffsplit " .. vim.fn.fnameescape(right))

		local right_win = vim.api.nvim_get_current_win()
		vim.wo[right_win].number = true
		vim.wo[right_win].relativenumber = false
		vim.bo.readonly = true
		vim.bo.modifiable = false
		vim.bo.bufhidden = "wipe"

		vim.cmd("wincmd h")
		local left_win = vim.api.nvim_get_current_win()
		vim.wo[left_win].number = true
		vim.wo[left_win].relativenumber = false
		vim.bo.readonly = true
		vim.bo.modifiable = false
		vim.bo.bufhidden = "wipe"

		if title and title ~= "" then
			vim.api.nvim_echo({ { "lazySVN diff: " .. title, "Title" } }, false, {})
		end
	end)

	return 1
end

--- Setup the plugin.
--- Creates the `:LazySVN` user command.
---@param opts? table
function M.setup(opts)
	opts = opts or {}
	vim.api.nvim_create_user_command("LazySVN", M.open, { desc = "Open lazySVN TUI" })
	_G.LazySvnOpenDiff = M.open_diff
end

return M
