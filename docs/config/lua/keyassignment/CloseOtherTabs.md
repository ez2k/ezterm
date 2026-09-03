# ``CloseOtherTabs { confirm = true }``

{{since('nightly')}}

Closes every tab in the current window except the active one.  With
`confirm = true`, a single confirmation prompt is shown if any of the
tabs would normally ask before closing (see
[skip_close_confirmation_for_processes_named](../config/skip_close_confirmation_for_processes_named.md)).

```lua
config.keys = {
  { key = 'O', mods = 'CTRL|SHIFT', action = wezterm.action.CloseOtherTabs { confirm = true } },
}
```
