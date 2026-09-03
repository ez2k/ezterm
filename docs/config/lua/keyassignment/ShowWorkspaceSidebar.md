# ``ShowWorkspaceSidebar``

{{since('nightly')}}

Toggles a workspace sidebar on the left edge of the window. Like the tab
bar, the sidebar is part of the window chrome rather than a pane: it spans
the full height beside the panes and stays in place across tab and
workspace switches.

Each row shows a workspace name and its window count; the active workspace
is highlighted and marked with `*`, and each row is numbered to match
[SwitchToWorkspaceByIndex](SwitchToWorkspaceByIndex.md). Clicking a row
switches to that workspace; double-clicking prompts to rename it.
Right-clicking a row opens a menu with *Switch*, *Rename*, *New window in
workspace* and *Close workspace*; right-clicking the header or a blank
row offers *New workspace* and *Hide sidebar*.

Closing a workspace always asks for confirmation first. The dialog lists
how many windows, tabs and panes will be killed and which programs are
running in them, and warns when it is the last workspace (closing it
quits ezterm). The width is controlled by
[workspace_sidebar_width](../config/workspace_sidebar_width.md).

```lua
config.keys = {
  -- CTRL-SHIFT-w toggles the workspace sidebar
  { key = 'W', mods = 'CTRL', action = wezterm.action.ShowWorkspaceSidebar },
}
```
