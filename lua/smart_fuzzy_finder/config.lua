local M = {}

M.defaults = {
	binary = "smart-fuzzy-finder",
	limit = 80,
	include_hidden = false,
	use_cache = true,
	cache_ttl = 30,
	rebuild_cache = false,
	border = "rounded",
	prompt_height = 1,
	width = 0.86,
	height = 0.72,
	preview_width = 0.50,
	keymaps = {
		find_files = "<leader>ff",
		live_grep = "<leader>fg",
		grep_cword = "<leader>fw",
	},
}

M.options = vim.deepcopy(M.defaults)

function M.setup(opts)
	M.options = vim.tbl_deep_extend("force", vim.deepcopy(M.defaults), opts or {})
end

function M.get()
	return M.options
end

return M
