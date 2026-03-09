---@type LazySpec
return {
  "jaller698/lazy-svn",
  build = "cargo build --release",
  dependencies = {
    "folke/snacks.nvim",
  },
  config = true,
}
