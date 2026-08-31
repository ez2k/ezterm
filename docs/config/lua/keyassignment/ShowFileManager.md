# ``ShowFileManager``

{{since('nightly')}}

Opens a simple file manager in a sidebar pane on the right side of the
current tab (a full-height split taking 30% of the width). Closing the
file manager (`q`/`Escape`) closes the sidebar pane.

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
