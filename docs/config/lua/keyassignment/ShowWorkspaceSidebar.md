# ``ShowWorkspaceSidebar``

{{since('nightly')}}

Toggles a sidebar pane on the left side of the current tab (a
full-height split taking 20% of the width) that lists the mux
workspaces, with the number of windows in each and a `*` marker on
the active workspace.

Keys and mouse:

* `Enter`, or clicking the selected row - switch to that workspace
* `Up`/`Down` or `k`/`j`, mouse wheel - move the selection
* `n` - prompt for a name and create/switch to that workspace
* `r` - refresh the listing
* `q` / `Escape` - close the sidebar

Invoking the assignment again while the sidebar is open closes it.
Note that the sidebar pane lives in the tab it was opened from, so
switching workspaces leaves it behind in the original workspace.

```lua
config.keys = {
  -- CTRL-SHIFT-w toggles the workspace sidebar
  { key = 'W', mods = 'CTRL', action = wezterm.action.ShowWorkspaceSidebar },
}
```
