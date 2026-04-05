if vim.g.loaded_tate_nvim == 1 or vim.g.loaded_smart_fuzzy_finder_nvim == 1 then
	return
end

vim.g.loaded_tate_nvim = 1
vim.g.loaded_smart_fuzzy_finder_nvim = 1

-- Commands/keymaps are registered when users call setup.
