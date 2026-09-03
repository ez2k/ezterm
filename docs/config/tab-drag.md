# Dragging tabs

{{since('nightly')}}

Tabs in the tab bar can be dragged with the left mouse button:

* **Reorder**: drag a tab left or right along the tab bar. A marker shows
  where it will be inserted; release to drop.
* **Move to a workspace**: with the
  [workspace sidebar](lua/keyassignment/ShowWorkspaceSidebar.md) open, drop
  the tab on a workspace row to move it into that workspace (into its
  first window, or a new window if the workspace has none).
* **Detach into a new window**: drop the tab outside the window to open a
  new window at the pointer containing just that tab. This does nothing
  for the only tab in a window.

A ghost label follows the pointer while dragging; press `Escape` (or any
other mouse button) to cancel the drag.
