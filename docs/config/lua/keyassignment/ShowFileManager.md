# ``ShowFileManager``

{{since('nightly')}}

Toggles a simple file manager in a sidebar pane on the right side of the
current tab (a full-height split taking 30% of the width). Invoking it
again while the sidebar is open closes it; `q`/`Escape` inside the file
manager also closes the sidebar pane.

For local panes it browses the local filesystem starting from the pane's
current working directory.

For panes attached to an [ssh domain](../SshDomain.md),
it browses the remote filesystem over the existing ssh session using SFTP,
and additionally supports:

* `d` - download the selected remote file to your local downloads directory
* `u` - upload a local file (you will be prompted for its path) into the
  current remote directory

Common keys:

* `Enter` / `Right` - enter the selected directory
* `Backspace` / `Left` - go to the parent directory
* `Up`/`Down` or `k`/`j` - move the selection
* `r` - refresh the listing
* `q` / `Escape` - close the file manager

```lua
config.keys = {
  -- CTRL-SHIFT-e activates the file manager
  { key = 'E', mods = 'CTRL', action = wezterm.action.ShowFileManager },
}
```

## Action menu

Press `m`, or Shift/Ctrl + right-click a row, to open a small action
menu for the selected entry: open or view it, download it (remote),
upload a file here (remote), rename or delete it, go to the parent
directory, back, forward, refresh, or close the file manager. `F2`
renames and `Delete` deletes the selected entry directly; deleting asks
for confirmation, and only empty directories can be deleted. Navigate with the arrow keys or the
mouse; `Escape` or a click outside dismisses it. A plain right click still
goes back and a middle click still goes forward.
