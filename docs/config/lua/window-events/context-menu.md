# `context-menu`

{{since('nightly')}}

The `context-menu` event is emitted when a context menu is about to be
shown, either from [ShowContextMenu](../keyassignment/ShowContextMenu.md)
(by default: a right click in the terminal) or from a right click on a
tab in the tab bar.

The parameters are:

* `window` - the [window](../window/index.md) object
* `pane` - the active [pane](../pane/index.md) object
* `kind` - `"terminal"` or `"tab"`
* `tab_idx` - the 0-based index of the tab that was clicked, or `nil`

Returning `false` suppresses the built-in menu, allowing you to show
something else instead:

```lua
local wezterm = require 'wezterm'

wezterm.on('context-menu', function(window, pane, kind, tab_idx)
  if kind == 'terminal' then
    window:perform_action(wezterm.action.ShowLauncher, pane)
    return false
  end
end)
```
