# ``ShowContextMenu``

{{since('nightly')}}

Shows a context menu at the current mouse position for the active pane.
By default a plain right click (with no modifiers, and when the running
application is not capturing the mouse) triggers this action; right
clicking a tab in the tab bar shows a tab-specific menu instead.

The terminal menu offers copy/paste, opening a hovered link, splitting,
zooming and closing the pane, spawning a tab, copying the working
directory, and opening the file manager or command palette.  The tab
menu offers activating, duplicating, renaming, moving and closing tabs
(including "close other tabs" and "close tabs to the right").

Navigate the menu with the mouse, or with `Up`/`Down` (or `j`/`k`) and
`Enter`; `Escape` or a click outside the menu dismisses it.

The [context-menu](../window-events/context-menu.md) event fires before
the default menu is shown; returning `false` from it suppresses the menu.

To restore the upstream right-click behaviour (no menu), disable the
default mouse binding:

```lua
config.mouse_bindings = {
  {
    event = { Down = { streak = 1, button = 'Right' } },
    mods = 'NONE',
    action = wezterm.action.DisableDefaultAssignment,
  },
}
```
