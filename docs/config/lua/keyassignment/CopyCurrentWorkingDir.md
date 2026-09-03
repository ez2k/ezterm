# ``CopyCurrentWorkingDir``

{{since('nightly')}}

Copies the active pane's current working directory to the clipboard
and primary selection.  Local directories are copied as a plain path;
remote ones as a URL.

```lua
config.keys = {
  { key = 'C', mods = 'CTRL|SHIFT|ALT', action = wezterm.action.CopyCurrentWorkingDir },
}
```
