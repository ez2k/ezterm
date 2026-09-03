# Dragging panes

{{since('nightly')}}

Hold [pane_drag_modifiers](lua/config/pane_drag_modifiers.md) (default
`CTRL|ALT`) and drag a pane with the left mouse button. A ghost label
follows the pointer and the drop zone is highlighted:

* **Edges of another pane** (outer quarter on each side): split that pane
  and place the dragged pane on that side.
* **Centre of another pane**: swap the two panes.
* **A tab in the tab bar**: move the pane into that tab, beside its
  active pane.
* **The empty part of the tab bar or the new-tab button**: move the pane
  into a new tab.
* **A workspace sidebar row**: move the pane into a new tab in that
  workspace.
* **Outside the window**: detach the pane into a new window at the
  pointer.

Moving the only pane of a tab into a new tab is a no-op. Press `Escape`
or any other mouse button to cancel.
