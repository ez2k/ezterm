//! Dragging tabs with the mouse: reorder within the tab bar, drop on a
//! workspace sidebar row to move the tab to that workspace, or drop
//! outside the window to detach the tab into a new window.
use crate::tabbar::TabBarItem;
use crate::termwindow::box_model::*;
use crate::termwindow::{DimensionContext, TermWindow, UIItemType};
use config::{Dimension, GeometryOrigin, GuiPosition};
use mux::tab::TabId;
use mux::Mux;
use window::color::LinearRgba;
use window::{MouseEvent, WindowOps};

/// How far the pointer must travel before a press on a tab becomes a drag
const DRAG_THRESHOLD_PX: isize = 6;

pub struct TabDrag {
    pub tab_idx: usize,
    pub tab_id: TabId,
    pub title: String,
    pub start: MouseEvent,
    pub current: MouseEvent,
    /// true once the pointer has moved past the drag threshold
    pub active: bool,
}

/// Where a dragged tab would land if released now
#[derive(Debug, Clone, PartialEq)]
pub enum TabDropTarget {
    /// Insert at this index in the tab bar (index in the current tab
    /// order, before removal of the dragged tab); the f32 is the pixel
    /// x of the insertion point for the indicator.
    TabBar {
        insert_idx: usize,
        x: f32,
    },
    /// Merge the dragged tab's panes into this tab (Shift held)
    Merge {
        tab_idx: usize,
        x: f32,
        width: f32,
    },
    /// Move to this workspace (sidebar row)
    Workspace {
        name: String,
        row: usize,
    },
    /// Detach into a new window at these screen coordinates
    NewWindow {
        x: isize,
        y: isize,
    },
    None,
}

impl TermWindow {
    pub fn tab_drag_is_active(&self) -> bool {
        self.tab_drag.as_ref().map(|d| d.active).unwrap_or(false)
    }

    /// Records a press on a tab as a potential drag
    pub fn begin_tab_drag(&mut self, tab_idx: usize, event: &MouseEvent) {
        let mux = Mux::get();
        let Some(window) = mux.get_window(self.mux_window_id) else {
            return;
        };
        let Some(tab) = window.get_tab_at_idx(tab_idx) else {
            return;
        };
        self.tab_drag = Some(TabDrag {
            tab_idx,
            tab_id: tab.tab_id(),
            title: tab.get_title(),
            start: event.clone(),
            current: event.clone(),
            active: false,
        });
    }

    pub fn cancel_tab_drag(&mut self) -> bool {
        if self.tab_drag.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
            true
        } else {
            false
        }
    }

    /// Updates the drag with a new pointer position. Returns true if a
    /// drag is in progress and the event was consumed.
    pub fn update_tab_drag(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(drag) = self.tab_drag.as_mut() else {
            return false;
        };
        drag.current = event.clone();
        if !drag.active {
            let dx = (event.coords.x - drag.start.coords.x).abs();
            let dy = (event.coords.y - drag.start.coords.y).abs();
            if dx.max(dy) < DRAG_THRESHOLD_PX {
                return false;
            }
            drag.active = true;
        }
        context.set_cursor(Some(window::CursorIcon::Grabbing));
        context.invalidate();
        true
    }

    /// Computes where the dragged tab would land for the given pointer
    pub fn tab_drop_target(&self, event: &MouseEvent) -> TabDropTarget {
        let Some(drag) = self.tab_drag.as_ref() else {
            return TabDropTarget::None;
        };
        let x = event.coords.x;
        let y = event.coords.y;

        // Outside the window: detach
        if x < 0
            || y < 0
            || x as usize >= self.dimensions.pixel_width
            || y as usize >= self.dimensions.pixel_height
        {
            return TabDropTarget::NewWindow {
                x: event.screen_coords.x,
                y: event.screen_coords.y,
            };
        }

        // Over the workspace sidebar: move to that workspace
        if self.show_workspace_sidebar {
            let border = self.get_os_border();
            let left = border.left.get() as isize;
            let sidebar_px = self.workspace_sidebar_pixel_width() as isize;
            if x >= left && x < left + sidebar_px {
                let rel = y - self.workspace_sidebar_top() as isize;
                if rel >= 0 {
                    let row = rel as usize / self.render_metrics.cell_size.height as usize;
                    if let Some(name) = self.workspace_sidebar_row_name(row) {
                        return TabDropTarget::Workspace { name, row };
                    }
                }
                return TabDropTarget::None;
            }
        }

        // Over the tab bar: find the insertion point among the tab items
        let mut tabs: Vec<(usize, f32, f32, f32)> = self
            .ui_items
            .iter()
            .filter_map(|item| match &item.item_type {
                UIItemType::TabBar(TabBarItem::Tab { tab_idx, .. }) => Some((
                    *tab_idx,
                    item.x as f32,
                    (item.x + item.width) as f32,
                    item.y as f32,
                )),
                _ => None,
            })
            .collect();
        if tabs.is_empty() {
            return TabDropTarget::None;
        }
        tabs.sort_by(|a, b| a.1.partial_cmp(&b.1).unwrap());

        let bar_y = tabs[0].3;
        let bar_h = self.tab_bar_pixel_height().unwrap_or(0.);
        let in_bar = y as f32 >= bar_y - bar_h && (y as f32) < bar_y + bar_h * 2.;
        if !in_bar {
            return TabDropTarget::None;
        }

        let xf = x as f32;

        // Shift + drop on another tab merges into it
        if event.modifiers.contains(window::Modifiers::SHIFT) {
            for (idx, left, right, _) in &tabs {
                if *idx != drag.tab_idx && xf >= *left && xf < *right {
                    return TabDropTarget::Merge {
                        tab_idx: *idx,
                        x: *left,
                        width: right - left,
                    };
                }
            }
        }

        let mut target = None;
        for (idx, left, right, _) in &tabs {
            if xf < (left + right) / 2. {
                target = Some((*idx, *left));
                break;
            }
        }
        let (insert_idx, ix) = target.unwrap_or_else(|| {
            let last = tabs.last().unwrap();
            (last.0 + 1, last.2)
        });
        let _ = drag;
        TabDropTarget::TabBar { insert_idx, x: ix }
    }

    /// Completes the drag on release
    pub fn finish_tab_drag(&mut self, event: &MouseEvent) {
        let target = self.tab_drop_target(event);
        let Some(drag) = self.tab_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        match target {
            TabDropTarget::TabBar { insert_idx, .. } => {
                // insert_idx is in pre-removal numbering
                let dest = if insert_idx > drag.tab_idx {
                    insert_idx - 1
                } else {
                    insert_idx
                };
                if dest != drag.tab_idx {
                    if self.activate_tab(drag.tab_idx as isize).is_ok() {
                        if let Err(err) = self.move_tab(dest) {
                            log::error!("tab drag: move_tab: {err:#}");
                        }
                    }
                }
            }
            TabDropTarget::Merge { tab_idx, .. } => {
                self.merge_tab_into(drag.tab_id, tab_idx);
            }
            TabDropTarget::Workspace { name, .. } => {
                self.move_tab_to_workspace(drag.tab_id, &name);
            }
            TabDropTarget::NewWindow { x, y } => {
                if self.activate_tab(drag.tab_idx as isize).is_ok() {
                    self.move_tab_to_new_window_at(Some(GuiPosition {
                        x: Dimension::Pixels((x - 40).max(0) as f32),
                        y: Dimension::Pixels((y - 10).max(0) as f32),
                        origin: GeometryOrigin::ScreenCoordinateSystem,
                    }));
                }
            }
            TabDropTarget::None => {}
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Moves every pane of `src_tab_id` into the tab at `dest_idx` of
    /// this window (each split beside that tab's active pane) and
    /// removes the now-empty source tab.
    pub fn merge_tab_into(&mut self, src_tab_id: TabId, dest_idx: usize) {
        let mux = Mux::get();
        let Some(src) = mux.get_tab(src_tab_id) else {
            return;
        };
        let Some(dest) = mux
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_tab_at_idx(dest_idx).map(std::sync::Arc::clone))
        else {
            return;
        };
        if dest.tab_id() == src_tab_id {
            return;
        }
        src.set_zoomed(false);
        dest.set_zoomed(false);
        let panes: Vec<_> = src
            .iter_panes_ignoring_zoom()
            .into_iter()
            .map(|p| p.pane)
            .collect();
        for pane in panes {
            let Some(pane) = src.remove_pane(pane.pane_id()) else {
                continue;
            };
            let active = dest.get_active_idx();
            match dest.split_and_insert(
                active,
                mux::tab::SplitRequest {
                    direction: mux::tab::SplitDirection::Horizontal,
                    target_is_second: true,
                    top_level: false,
                    size: mux::tab::SplitSize::Percent(50),
                },
                pane,
            ) {
                Ok(idx) => dest.set_active_idx(idx),
                Err(err) => log::error!("merge_tab_into: {err:#}"),
            }
        }
        mux.remove_tab(src_tab_id);
        let window = mux.get_window_mut(self.mux_window_id);
        if let Some(mut window) = window {
            if let Some(i) = window.get_tab_idx_for_id(dest.tab_id()) {
                window.set_active_tab_idx_without_saving(i);
            }
        }
    }

    /// Moves the given tab out of this window into a window of the
    /// named workspace, creating one if the workspace has none.
    pub fn move_tab_to_workspace(&mut self, tab_id: TabId, workspace: &str) {
        let mux = Mux::get();
        let tab = {
            let Some(mut window) = mux.get_window_mut(self.mux_window_id) else {
                return;
            };
            if window.get_workspace() == workspace {
                return;
            }
            let Some(idx) = window.get_tab_idx_for_id(tab_id) else {
                return;
            };
            window.remove_tab_idx(idx)
        };
        let dest = mux.iter_windows_in_workspace(workspace).into_iter().next();
        match dest {
            Some(window_id) => {
                if let Err(err) = mux.add_tab_to_window(&tab, window_id) {
                    log::error!("move_tab_to_workspace: {err:#}");
                }
                if let Some(mut window) = mux.get_window_mut(window_id) {
                    let n = window.count_tabs();
                    if n > 0 {
                        window.set_active_tab_idx_without_saving(n - 1);
                    }
                }
            }
            None => {
                let builder = mux.new_empty_window(Some(workspace.to_string()), None);
                if let Err(err) = mux.add_tab_to_window(&tab, *builder) {
                    log::error!("move_tab_to_workspace: {err:#}");
                }
                drop(builder);
            }
        }
        // If that was our last tab, let the mux retire this window
        mux.prune_dead_windows();
    }

    /// The workspace shown on the given sidebar row, if any
    pub fn workspace_sidebar_row_name(&self, row: usize) -> Option<String> {
        const HEADER_ROWS: usize = 1;
        if row < HEADER_ROWS {
            return None;
        }
        self.workspace_sidebar_entries()
            .get(row - HEADER_ROWS)
            .map(|e| e.name.clone())
    }

    /// Draws a floating label near the given pixel position (used as the
    /// ghost while dragging tabs and panes)
    pub fn paint_drag_ghost(&mut self, label: String, x: f32, y: f32) -> anyhow::Result<()> {
        let font = self.fonts.title_font()?;
        let palette = self.palette().clone();
        let fg = palette.foreground.to_linear();
        let accent = palette.cursor_bg.to_linear();
        let bg = palette.background.to_linear();
        let element = Element::new(&font, ElementContent::Text(label))
            .colors(ElementColors {
                border: BorderColor::new(accent),
                bg: bg.mul_alpha(0.85).into(),
                text: fg.into(),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.5),
                right: Dimension::Cells(0.5),
                top: Dimension::Cells(0.1),
                bottom: Dimension::Cells(0.1),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .zindex(2);
        self.paint_floating_element(&font, element, x + 12., y + 12.)
    }

    /// Lays out and renders an element at an absolute pixel position
    pub fn paint_floating_element(
        &mut self,
        font: &std::rc::Rc<wezterm_font::LoadedFont>,
        element: Element,
        x: f32,
        y: f32,
    ) -> anyhow::Result<()> {
        let metrics = crate::utilsprites::RenderMetrics::with_font_metrics(&font.metrics());
        let dims = self.dimensions;
        let computed = self.compute_element(
            &LayoutContext {
                height: DimensionContext {
                    dpi: dims.dpi as f32,
                    pixel_max: dims.pixel_height as f32,
                    pixel_cell: metrics.cell_size.height as f32,
                },
                width: DimensionContext {
                    dpi: dims.dpi as f32,
                    pixel_max: dims.pixel_width as f32,
                    pixel_cell: metrics.cell_size.width as f32,
                },
                bounds: euclid::rect(
                    x,
                    y,
                    (dims.pixel_width as f32 - x).max(1.),
                    (dims.pixel_height as f32 - y).max(1.),
                ),
                metrics: &metrics,
                gl_state: self.render_state.as_ref().unwrap(),
                zindex: 100,
            },
            &element,
        )?;
        let gl_state = self.render_state.as_ref().unwrap();
        self.render_element(&computed, gl_state, None)
    }

    /// Draws the drag ghost and the drop indicator, if a drag is active
    pub fn paint_tab_drag(&mut self) -> anyhow::Result<()> {
        let Some(drag) = self.tab_drag.as_ref() else {
            return Ok(());
        };
        if !drag.active {
            return Ok(());
        }
        let title = drag.title.clone();
        let current = drag.current.clone();
        let target = self.tab_drop_target(&current);

        let font = self.fonts.title_font()?;
        let palette = self.palette().clone();
        let accent = palette.cursor_bg.to_linear();

        let label = match &target {
            TabDropTarget::NewWindow { .. } => format!("{title}  → new window"),
            TabDropTarget::Workspace { name, .. } => format!("{title}  → {name}"),
            TabDropTarget::Merge { tab_idx, .. } => {
                format!("{title}  → merge into tab {}", tab_idx + 1)
            }
            _ => title.clone(),
        };
        self.paint_drag_ghost(label, current.coords.x as f32, current.coords.y as f32)?;

        // Insertion marker in the tab bar
        if let TabDropTarget::TabBar { x, .. } = &target {
            let bar_top = self
                .ui_items
                .iter()
                .find_map(|item| match &item.item_type {
                    UIItemType::TabBar(TabBarItem::Tab { .. }) => Some(item.y as f32),
                    _ => None,
                })
                .unwrap_or(0.);
            let marker = Element::new(&font, ElementContent::Text(" ".to_string()))
                .colors(ElementColors {
                    border: BorderColor::default(),
                    bg: accent.into(),
                    text: LinearRgba::TRANSPARENT.into(),
                })
                .min_width(Some(Dimension::Pixels(3.)))
                .max_width(Some(Dimension::Pixels(3.)))
                .zindex(1);
            self.paint_floating_element(&font, marker, (x - 1.5).max(0.), bar_top)?;
        }

        // Highlight the tab being merged into
        if let TabDropTarget::Merge { x, width, .. } = &target {
            let bar_top = self
                .ui_items
                .iter()
                .find_map(|item| match &item.item_type {
                    UIItemType::TabBar(TabBarItem::Tab { .. }) => Some(item.y as f32),
                    _ => None,
                })
                .unwrap_or(0.);
            let hl = Element::new(&font, ElementContent::Text(" ".to_string()))
                .colors(ElementColors {
                    border: BorderColor::new(accent),
                    bg: accent.mul_alpha(0.25).into(),
                    text: LinearRgba::TRANSPARENT.into(),
                })
                .border(BoxDimension::new(Dimension::Pixels(1.)))
                .min_width(Some(Dimension::Pixels(*width)))
                .zindex(1);
            self.paint_floating_element(&font, hl, *x, bar_top)?;
        }

        // Highlight the sidebar row being targeted
        if let TabDropTarget::Workspace { row, .. } = &target {
            self.paint_sidebar_row_highlight(*row)?;
        }
        Ok(())
    }

    /// Highlights a workspace sidebar row as a drop target
    pub fn paint_sidebar_row_highlight(&mut self, row: usize) -> anyhow::Result<()> {
        let font = self.fonts.title_font()?;
        let palette = self.palette().clone();
        let accent = palette.cursor_bg.to_linear();
        let border = self.get_os_border();
        let cell_h = self.render_metrics.cell_size.height as f32;
        let top = self.workspace_sidebar_top() + row as f32 * cell_h;
        let element = Element::new(&font, ElementContent::Text(" ".to_string()))
            .colors(ElementColors {
                border: BorderColor::new(accent),
                bg: accent.mul_alpha(0.25).into(),
                text: LinearRgba::TRANSPARENT.into(),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .min_width(Some(Dimension::Pixels(
                self.workspace_sidebar_pixel_width() as f32,
            )))
            .zindex(1);
        self.paint_floating_element(&font, element, border.left.get() as f32, top)
    }
}
