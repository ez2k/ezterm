# ``MoveTabToNewWindow``

{{since('nightly')}}

Detaches the active tab from its window into a new window in the same
workspace.  Does nothing if the tab is the only tab in its window.

```lua
config.keys = {
  { key = 'N', mods = 'CTRL|SHIFT|ALT', action = wezterm.action.MoveTabToNewWindow },
}
```
