# ``DuplicateTab``

{{since('nightly')}}

Spawns a new tab in the same domain as the active pane, starting in the
same working directory (when the pane reports one via OSC 7).

```lua
config.keys = {
  { key = 'D', mods = 'CTRL|SHIFT', action = wezterm.action.DuplicateTab },
}
```
