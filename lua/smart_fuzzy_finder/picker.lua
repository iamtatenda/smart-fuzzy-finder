local config = require("smart_fuzzy_finder.config")

local M = {}

local function calc_layout(opts)
	local columns = vim.o.columns
	local lines = vim.o.lines - vim.o.cmdheight

	local width = math.floor(columns * opts.width)
	local height = math.floor(lines * opts.height)
	local row = math.floor((lines - height) / 2)
	local col = math.floor((columns - width) / 2)

	local prompt_height = opts.prompt_height
	local body_height = height - prompt_height - 1
	local results_width = math.floor(width * (1 - opts.preview_width))
	local preview_width = width - results_width - 1

	return {
		row = row,
		col = col,
		width = width,
		height = height,
		prompt_height = prompt_height,
		body_height = body_height,
		results_width = results_width,
		preview_width = preview_width,
	}
end

local function open_window(buf, opts, cfg)
	return vim.api.nvim_open_win(buf, true, {
		relative = "editor",
		row = opts.row,
		col = opts.col,
		width = opts.width,
		height = opts.height,
		style = "minimal",
		border = cfg.border,
	})
end

local function load_preview(preview_buf, root, relpath)
	vim.bo[preview_buf].modifiable = true
	vim.api.nvim_buf_set_lines(preview_buf, 0, -1, false, {})

	if not relpath or relpath == "" then
		vim.bo[preview_buf].modifiable = false
		return
	end

	local target = root .. "/" .. relpath
	local lines = {}
	local ok = pcall(function()
		lines = vim.fn.readfile(target, "", 200)
	end)

	if ok and #lines > 0 then
		vim.api.nvim_buf_set_lines(preview_buf, 0, -1, false, lines)
		-- Enable syntax highlighting in the preview pane.
		pcall(function()
			local ft = vim.filetype.match({ buf = preview_buf, filename = relpath })
			if ft then
				vim.bo[preview_buf].filetype = ft
			end
		end)
	else
		vim.api.nvim_buf_set_lines(preview_buf, 0, -1, false, { "Unable to preview: " .. relpath })
	end

	vim.bo[preview_buf].modifiable = false
end

--- Start an async file-find and call on_done(results, err) when complete.
--- Returns a handle table with a single `kill()` function regardless of which
--- backend (vim.system or jobstart) is in use, so callers need only one path.
local function run_find_async(root, query, opts, on_done)
	local cmd = {
		opts.binary,
		"find",
		"--root",
		root,
		"--query",
		query,
		"--limit",
		tostring(opts.limit),
		"--cache-ttl",
		tostring(opts.cache_ttl or 30),
		"--json",
	}

	if opts.include_hidden then
		table.insert(cmd, "--include-hidden")
	end
	if not opts.use_cache then
		table.insert(cmd, "--no-cache")
	end
	if opts.rebuild_cache then
		table.insert(cmd, "--rebuild-cache")
	end

	if vim.system then
		local handle = vim.system(cmd, { text = true }, function(out)
			vim.schedule(function()
				if out.code ~= 0 then
					on_done(nil, out.stderr or "search failed")
					return
				end

				local ok, decoded = pcall(vim.json.decode, out.stdout or "")
				if not ok then
					on_done(nil, "failed to parse finder output")
					return
				end

				on_done(decoded, nil)
			end)
		end)
		-- Normalise: wrap the vim.system object so all callers use .stop().
		return {
			stop = function()
				handle:kill(15)
			end,
		}
	end

	local stdout_chunks = {}
	local stderr_chunks = {}
	local job_id = vim.fn.jobstart(cmd, {
		stdout_buffered = true,
		stderr_buffered = true,
		on_stdout = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						table.insert(stdout_chunks, line)
					end
				end
			end
		end,
		on_stderr = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						table.insert(stderr_chunks, line)
					end
				end
			end
		end,
		on_exit = function(_, code)
			vim.schedule(function()
				if code ~= 0 then
					on_done(nil, table.concat(stderr_chunks, "\n"))
					return
				end

				local ok, decoded = pcall(vim.json.decode, table.concat(stdout_chunks, "\n"))
				if not ok then
					on_done(nil, "failed to parse finder output")
					return
				end

				on_done(decoded, nil)
			end)
		end,
	})

	if job_id <= 0 then
		on_done(nil, "failed to spawn finder process")
		return nil
	end

	return {
		stop = function()
			vim.fn.jobstop(job_id)
		end,
	}
end

function M.find_files()
	local opts = config.get()
	local root = vim.fn.getcwd()

	local layout = calc_layout(opts)
	local frame_buf = vim.api.nvim_create_buf(false, true)
	local prompt_buf = vim.api.nvim_create_buf(false, true)
	local results_buf = vim.api.nvim_create_buf(false, true)
	local preview_buf = vim.api.nvim_create_buf(false, true)

	local frame_win = open_window(frame_buf, {
		row = layout.row,
		col = layout.col,
		width = layout.width,
		height = layout.height,
	}, opts)

	local prompt_win = vim.api.nvim_open_win(prompt_buf, true, {
		relative = "editor",
		row = layout.row + 1,
		col = layout.col + 2,
		width = layout.width - 4,
		height = 1,
		style = "minimal",
		border = "none",
	})

	local results_win = vim.api.nvim_open_win(results_buf, false, {
		relative = "editor",
		row = layout.row + 3,
		col = layout.col + 2,
		width = layout.results_width - 2,
		height = layout.body_height - 2,
		style = "minimal",
		border = "single",
	})

	local preview_win = vim.api.nvim_open_win(preview_buf, false, {
		relative = "editor",
		row = layout.row + 3,
		col = layout.col + layout.results_width + 1,
		width = layout.preview_width - 2,
		height = layout.body_height - 2,
		style = "minimal",
		border = "single",
	})

	local state = {
		items = {},
		index = 1,
		last_request = 0,
		active_job = nil,
	}

	vim.bo[prompt_buf].buftype = "prompt"
	vim.fn.prompt_setprompt(prompt_buf, "Find> ")
	vim.cmd.startinsert()

	vim.bo[results_buf].modifiable = false
	vim.bo[results_buf].bufhidden = "wipe"
	vim.bo[preview_buf].modifiable = false
	vim.bo[preview_buf].bufhidden = "wipe"

	--- Cancel any in-flight search job using the normalised stop() interface.
	local function cancel_active_job()
		if state.active_job then
			pcall(state.active_job.stop)
			state.active_job = nil
		end
	end

	local function close_all()
		cancel_active_job()
		for _, win in ipairs({ prompt_win, results_win, preview_win, frame_win }) do
			if vim.api.nvim_win_is_valid(win) then
				vim.api.nvim_win_close(win, true)
			end
		end
	end

	local function render_results()
		local lines = {}
		for i, item in ipairs(state.items) do
			lines[i] = string.format("%6.3f  %s", item.score or 0, item.path)
		end

		vim.bo[results_buf].modifiable = true
		vim.api.nvim_buf_set_lines(results_buf, 0, -1, false, lines)
		vim.bo[results_buf].modifiable = false

		if #state.items == 0 then
			load_preview(preview_buf, root, nil)
			return
		end

		state.index = math.max(1, math.min(state.index, #state.items))
		vim.api.nvim_win_set_cursor(results_win, { state.index, 0 })
		load_preview(preview_buf, root, state.items[state.index].path)
	end

	local function select_current()
		local item = state.items[state.index]
		if not item then
			return
		end

		close_all()
		vim.cmd.edit(vim.fn.fnameescape(root .. "/" .. item.path))
		-- Record the open asynchronously so it does not block the editor.
		if vim.system then
			vim.system({ opts.binary, "touch", "--path", item.path }, { text = true })
		else
			vim.fn.jobstart({ opts.binary, "touch", "--path", item.path })
		end
	end

	local function move_cursor(delta)
		if #state.items == 0 then
			return
		end
		state.index = math.max(1, math.min(#state.items, state.index + delta))
		vim.api.nvim_win_set_cursor(results_win, { state.index, 0 })
		load_preview(preview_buf, root, state.items[state.index].path)
	end

	local function refresh_results()
		local line = vim.api.nvim_get_current_line()
		local query = line:gsub("^Find>%s*", "")
		state.last_request = state.last_request + 1
		local request_id = state.last_request

		cancel_active_job()

		if query == "" then
			state.items = {}
			state.index = 1
			render_results()
			return
		end

		vim.defer_fn(function()
			if request_id ~= state.last_request then
				return
			end

			state.active_job = run_find_async(root, query, opts, function(found, err)
				if request_id ~= state.last_request then
					return
				end

				state.active_job = nil
				if not found then
					if err and err ~= "" then
						vim.notify("smart-fuzzy-finder: " .. err, vim.log.levels.WARN)
					end
					return
				end

				state.items = found
				state.index = 1
				render_results()
			end)
		end, opts.debounce_ms or 60)
	end

	vim.api.nvim_buf_attach(prompt_buf, false, {
		on_lines = function()
			vim.schedule(refresh_results)
		end,
	})

	local map_opts = { noremap = true, silent = true, buffer = prompt_buf }
	vim.keymap.set("i", "<Esc>", close_all, map_opts)
	vim.keymap.set("i", "<CR>", select_current, map_opts)
	vim.keymap.set("i", "<Down>", function()
		move_cursor(1)
	end, map_opts)
	vim.keymap.set("i", "<Up>", function()
		move_cursor(-1)
	end, map_opts)

	vim.keymap.set("n", "q", close_all, { noremap = true, silent = true, buffer = frame_buf })
end

function M.live_grep()
	M.live_grep_prefilled(nil)
end

function M.grep_cword()
	local word = vim.fn.expand("<cword>")
	if not word or word == "" then
		return
	end

	M.live_grep_prefilled(word)
end

--- Run grep asynchronously and populate the quickfix list on completion.
--- Uses vim.system (non-blocking) when available, falls back to jobstart.
local function run_grep_async(query, opts, root)
	local cmd = {
		opts.binary,
		"grep",
		"--root",
		root,
		"--query",
		query,
		"--limit",
		tostring(opts.limit * 4),
		"--cache-ttl",
		tostring(opts.cache_ttl or 30),
		"--json",
	}

	if not opts.use_cache then
		table.insert(cmd, "--no-cache")
	end
	if opts.rebuild_cache then
		table.insert(cmd, "--rebuild-cache")
	end

	local function handle_result(stdout, err)
		if not stdout then
			vim.notify("smart-fuzzy-finder: " .. (err or "grep failed"), vim.log.levels.ERROR)
			return
		end

		local ok, matches = pcall(vim.json.decode, stdout)
		if not ok then
			vim.notify("smart-fuzzy-finder: failed to parse grep output", vim.log.levels.ERROR)
			return
		end

		local qf = {}
		for _, item in ipairs(matches) do
			qf[#qf + 1] = {
				filename = root .. "/" .. item.path,
				lnum = item.line,
				col = item.column,
				text = item.text,
			}
		end

		vim.fn.setqflist({}, " ", { title = "smart-fuzzy-finder grep: " .. query, items = qf })
		vim.cmd.copen()
	end

	if vim.system then
		vim.system(cmd, { text = true }, function(out)
			vim.schedule(function()
				if out.code ~= 0 then
					handle_result(nil, out.stderr or "grep failed")
				else
					handle_result(out.stdout, nil)
				end
			end)
		end)
		return
	end

	local stdout_chunks = {}
	local stderr_chunks = {}
	local job_id = vim.fn.jobstart(cmd, {
		stdout_buffered = true,
		stderr_buffered = true,
		on_stdout = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						table.insert(stdout_chunks, line)
					end
				end
			end
		end,
		on_stderr = function(_, data)
			if data then
				for _, line in ipairs(data) do
					if line ~= "" then
						table.insert(stderr_chunks, line)
					end
				end
			end
		end,
		on_exit = function(_, code)
			vim.schedule(function()
				if code ~= 0 then
					handle_result(nil, table.concat(stderr_chunks, "\n"))
				else
					handle_result(table.concat(stdout_chunks, "\n"), nil)
				end
			end)
		end,
	})

	if job_id <= 0 then
		vim.notify("smart-fuzzy-finder: failed to spawn grep process", vim.log.levels.ERROR)
	end
end

function M.live_grep_prefilled(default_text)
	local opts = config.get()
	local root = vim.fn.getcwd()

	vim.ui.input({ prompt = "Grep> ", default = default_text or "" }, function(input)
		if not input or input == "" then
			return
		end

		run_grep_async(input, opts, root)
	end)
end

return M
