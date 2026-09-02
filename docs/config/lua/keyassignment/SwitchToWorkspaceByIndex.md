# ``SwitchToWorkspaceByIndex(index)``

{{since('nightly')}}

Switches to the workspace at the given zero-based `index`, where workspaces
are ordered by name -- the same order and numbering shown in the
[workspace sidebar](ShowWorkspaceSidebar.md) (the sidebar displays the
1-based number). Does nothing if there is no workspace at that index.

```lua
local act = wezterm.action
config.keys = {}
-- ALT-1 .. ALT-9 switch to workspaces 1..9
for i = 1, 9 do
  table.insert(config.keys, {
    key = tostring(i),
    mods = 'ALT',
    action = act.SwitchToWorkspaceByIndex(i - 1),
  })
end
```
