# ``CloseTabsToTheRight { confirm = true }``

{{since('nightly')}}

Closes every tab to the right of the active one in the current window.
With `confirm = true`, a single confirmation prompt is shown if any of
the tabs would normally ask before closing.

```lua
config.keys = {
  { key = 'R', mods = 'CTRL|SHIFT|ALT', action = wezterm.action.CloseTabsToTheRight { confirm = true } },
}
```
