# ``RenameTab``

{{since('nightly')}}

Prompts for a new title for the active tab; the title is applied via
`tab:set_title()`.  Escape cancels the prompt.

```lua
config.keys = {
  { key = 'F2', mods = 'NONE', action = wezterm.action.RenameTab },
}
```
