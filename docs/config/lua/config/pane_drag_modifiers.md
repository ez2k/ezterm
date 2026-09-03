# `pane_drag_modifiers = "CTRL|ALT"`

{{since('nightly')}}

The modifier keys that, held while pressing the left mouse button in a
pane, start [dragging that pane](../../pane-drag.md) instead of sending
the click to the terminal or the mouse bindings.

Set it to `"NONE"` to disable pane dragging by mouse.

```lua
config.pane_drag_modifiers = 'SUPER'
```
