# lazy-svn

A Neovim plugin that opens the lazySVN TUI (built in Rust) inside a [snacks.nvim](https://github.com/folke/snacks.nvim) terminal window.

## Requirements

- [lazy.nvim](https://github.com/folke/lazy.nvim)
- [snacks.nvim](https://github.com/folke/snacks.nvim)
- Rust toolchain (`cargo`) available on `PATH` at install time

## Installation

Add the following to your lazy.nvim plugin list:

```lua
{
  "jaller698/lazy-svn",
  build = "cargo build --release",
  dependencies = {
    "folke/snacks.nvim",
  },
  config = true,
}
```

The `build` step compiles the Rust binary **once on install** (or when you run `:Lazy build lazy-svn`). The binary is **not** recompiled each time you open the TUI.

## Usage

### User command

```
:LazySVN
```

Opens the lazySVN TUI in a snacks terminal window.

### Suggested keymaps

Add a keymap in your config to open lazySVN quickly:

```lua
{
  "jaller698/lazy-svn",
  build = "cargo build --release",
  dependencies = {
    "folke/snacks.nvim",
  },
  keys = {
    { "<leader>sv", "<cmd>LazySVN<cr>", desc = "Open lazySVN" },
  },
  config = true,
}
```

Or set it up manually in your keymaps:

```lua
vim.keymap.set("n", "<leader>sv", "<cmd>LazySVN<cr>", { desc = "Open lazySVN" })
```

## Keybindings inside lazySVN

| Key   | Action                        |
|-------|-------------------------------|
| `j`   | Move down in file list        |
| `k`   | Move up in file list          |
| `Tab` | Switch between panels         |
| `r`   | Refresh SVN status            |
| `q`   | Quit                          |
