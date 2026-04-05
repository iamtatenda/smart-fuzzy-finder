local config = require("smart_fuzzy_finder.config")
local picker = require("smart_fuzzy_finder.picker")

local M = {}

local function upsert_user_command(name, rhs)
	pcall(vim.api.nvim_del_user_command, name)
	vim.api.nvim_create_user_command(name, rhs, {})
end

function M.find_files()
	picker.find_files()
end

function M.live_grep()
	picker.live_grep()
end

function M.grep_cword()
	picker.grep_cword()
end

function M.setup(opts)
	config.setup(opts)
	local merged = config.get()

	upsert_user_command("SmartFuzzyFind", function()
		M.find_files()
	end)

	upsert_user_command("SmartFuzzyGrep", function()
		M.live_grep()
	end)

	upsert_user_command("SmartFuzzyGrepWord", function()
		M.grep_cword()
	end)

	local km = merged.keymaps or {}
	if km.find_files and km.find_files ~= "" then
		vim.keymap.set("n", km.find_files, M.find_files, { desc = "Smart Fuzzy Find Files" })
	end
	if km.live_grep and km.live_grep ~= "" then
		vim.keymap.set("n", km.live_grep, M.live_grep, { desc = "Smart Fuzzy Live Grep" })
	end
	if km.grep_cword and km.grep_cword ~= "" then
		vim.keymap.set("n", km.grep_cword, M.grep_cword, { desc = "Smart Fuzzy Grep Cword" })
	end
end

return M
