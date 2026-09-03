//! Dragging panes with the mouse (modifier + left button): drop on a
//! zone of another pane to split beside it or swap with it, on a tab to
//! move it there, on the empty tab bar to give it its own tab, on a
//! workspace sidebar row to move it to that workspace, or outside the
//! window to detach it into a new window.
use crate::quad::TripleLayerQuadAllocator;
use crate::tabbar::TabBarItem;
use crate::termwindow::{TermWindow, UIItemType};
use config::{Dimension, GeometryOrigin, GuiPosition};
use mux::pane::{Pane, PaneId};
use mux::tab::{PositionedPane, SplitDirection, SplitRequest, SplitSize, Tab, TabId};
use mux::window::WindowId;
use mux::Mux;
use std::sync::Arc;
use window::{MouseEvent, RectF, WindowOps};

const DRAG_THRESHOLD_PX: isize = 6;

pub struct PaneDrag {
    pub pane_id: PaneId,
    pub tab_id: TabId,
    pub title: String,
    pub start: MouseEvent,
    pub current: MouseEvent,
    pub active: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PaneDropTarget {
    /// Split the target pane and put the dragged pane on the given side
    Split {
        pane_id: PaneId,
        direction: SplitDirection,
        second: bool,
        rect: RectF,
    },
    /// Swap positions with the target pane
    Swap {
        pane_id: PaneId,
        rect: RectF,
    },
    /// Move into the given tab, splitting beside its active pane
    Tab {
        tab_idx: usize,
    },
    /// Move into a new tab of this window
    NewTab,
    /// Move into a new tab in the given workspace (sidebar row)
    Workspace {
        name: String,
        row: usize,
    },
    /// Detach into a new window at the given screen position
    NewWindow {
        x: isize,
        y: isize,
    },
    None,
}

impl TermWindow {
    pub fn pane_drag_is_active(&self) -> bool {
        self.pane_drag.as_ref().map(|d| d.active).unwrap_or(false)
    }

    /// The pixel rectangle covered by a positioned pane
    fn pane_pixel_rect(&self, pos: &PositionedPane) -> RectF {
        let (padding_left, padding_top) = self.padding_left_top();
        let border = self.get_os_border();
        let top_bar = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
            self.tab_bar_pixel_height().unwrap_or(0.)
        } else {
            0.
        };
        let cw = self.render_metrics.cell_size.width as f32;
        let ch = self.render_metrics.cell_size.height as f32;
        euclid::rect(
            padding_left + border.left.get() as f32 + pos.left as f32 * cw,
            top_bar + padding_top + border.top.get() as f32 + pos.top as f32 * ch,
            pos.width as f32 * cw,
            pos.height as f32 * ch,
        )
    }

    /// The pane under the pointer, if any
    fn pane_at_pixel(&self, x: isize, y: isize) -> Option<(PositionedPane, RectF)> {
        for pos in self.get_panes_to_render() {
            let rect = self.pane_pixel_rect(&pos);
            if x as f32 >= rect.min_x()
                && (x as f32) < rect.max_x()
                && y as f32 >= rect.min_y()
                && (y as f32) < rect.max_y()
            {
                return Some((pos, rect));
            }
        }
        None
    }

    /// Starts a pane drag if the pointer is over a pane. Returns true if
    /// a drag was started.
    pub fn begin_pane_drag(&mut self, event: &MouseEvent) -> bool {
        let Some((pos, _)) = self.pane_at_pixel(event.coords.x, event.coords.y) else {
            return false;
        };
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return false;
        };
        self.pane_drag = Some(PaneDrag {
            pane_id: pos.pane.pane_id(),
            tab_id: tab.tab_id(),
            title: pos.pane.get_title(),
            start: event.clone(),
            current: event.clone(),
            active: false,
        });
        true
    }

    pub fn cancel_pane_drag(&mut self) -> bool {
        if self.pane_drag.take().is_some() {
            if let Some(window) = self.window.as_ref() {
                window.invalidate();
            }
            true
        } else {
            false
        }
    }

    pub fn update_pane_drag(&mut self, event: &MouseEvent, context: &dyn WindowOps) -> bool {
        let Some(drag) = self.pane_drag.as_mut() else {
            return false;
        };
        drag.current = event.clone();
        if !drag.active {
            let dx = (event.coords.x - drag.start.coords.x).abs();
            let dy = (event.coords.y - drag.start.coords.y).abs();
            if dx.max(dy) < DRAG_THRESHOLD_PX {
                return true;
            }
            drag.active = true;
        }
        context.set_cursor(Some(window::CursorIcon::Grabbing));
        context.invalidate();
        true
    }

    pub fn pane_drop_target(&self, event: &MouseEvent) -> PaneDropTarget {
        let Some(drag) = self.pane_drag.as_ref() else {
            return PaneDropTarget::None;
        };
        let x = event.coords.x;
        let y = event.coords.y;

        if x < 0
            || y < 0
            || x as usize >= self.dimensions.pixel_width
            || y as usize >= self.dimensions.pixel_height
        {
            return PaneDropTarget::NewWindow {
                x: event.screen_coords.x,
                y: event.screen_coords.y,
            };
        }

        if self.show_workspace_sidebar {
            let border = self.get_os_border();
            let left = border.left.get() as isize;
            let sidebar_px = self.workspace_sidebar_pixel_width() as isize;
            if x >= left && x < left + sidebar_px {
                let rel = y - self.workspace_sidebar_top() as isize;
                if rel >= 0 {
                    let row = rel as usize / self.render_metrics.cell_size.height as usize;
                    if let Some(name) = self.workspace_sidebar_row_name(row) {
                        return PaneDropTarget::Workspace { name, row };
                    }
                }
                return PaneDropTarget::None;
            }
        }

        // Tab bar items
        for item in self.ui_items.iter().rev() {
            if !item.hit_test(x, y) {
                continue;
            }
            match &item.item_type {
                UIItemType::TabBar(TabBarItem::Tab { tab_idx, .. }) => {
                    let mux = Mux::get();
                    let active = mux
                        .get_window(self.mux_window_id)
                        .map(|w| w.get_active_tab_idx());
                    if active == Some(*tab_idx) {
                        return PaneDropTarget::None;
                    }
                    return PaneDropTarget::Tab { tab_idx: *tab_idx };
                }
                UIItemType::TabBar(TabBarItem::NewTabButton { .. })
                | UIItemType::TabBar(TabBarItem::None) => {
                    return PaneDropTarget::NewTab;
                }
                UIItemType::TabBar(_) => return PaneDropTarget::None,
                _ => {}
            }
        }

        // Another pane in this tab: pick a zone
        let Some((pos, rect)) = self.pane_at_pixel(x, y) else {
            return PaneDropTarget::None;
        };
        if pos.pane.pane_id() == drag.pane_id {
            return PaneDropTarget::None;
        }
        let fx = (x as f32 - rect.min_x()) / rect.width().max(1.);
        let fy = (y as f32 - rect.min_y()) / rect.height().max(1.);
        if (0.25..0.75).contains(&fx) && (0.25..0.75).contains(&fy) {
            return PaneDropTarget::Swap {
                pane_id: pos.pane.pane_id(),
                rect,
            };
        }
        // nearest edge wins
        let d_left = fx;
        let d_right = 1. - fx;
        let d_top = fy;
        let d_bottom = 1. - fy;
        let min = d_left.min(d_right).min(d_top).min(d_bottom);
        let (direction, second, zone) = if min == d_left {
            (
                SplitDirection::Horizontal,
                false,
                euclid::rect(rect.min_x(), rect.min_y(), rect.width() / 2., rect.height()),
            )
        } else if min == d_right {
            (
                SplitDirection::Horizontal,
                true,
                euclid::rect(
                    rect.min_x() + rect.width() / 2.,
                    rect.min_y(),
                    rect.width() / 2.,
                    rect.height(),
                ),
            )
        } else if min == d_top {
            (
                SplitDirection::Vertical,
                false,
                euclid::rect(rect.min_x(), rect.min_y(), rect.width(), rect.height() / 2.),
            )
        } else {
            (
                SplitDirection::Vertical,
                true,
                euclid::rect(
                    rect.min_x(),
                    rect.min_y() + rect.height() / 2.,
                    rect.width(),
                    rect.height() / 2.,
                ),
            )
        };
        PaneDropTarget::Split {
            pane_id: pos.pane.pane_id(),
            direction,
            second,
            rect: zone,
        }
    }

    pub fn finish_pane_drag(&mut self, event: &MouseEvent) {
        let target = self.pane_drop_target(event);
        let Some(drag) = self.pane_drag.take() else {
            return;
        };
        if !drag.active {
            return;
        }
        if let Err(err) = self.apply_pane_drop(&drag, target) {
            log::error!("pane drag: {err:#}");
        }
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    fn pane_index_in_tab(tab: &Arc<Tab>, pane_id: PaneId) -> Option<usize> {
        tab.iter_panes_ignoring_zoom()
            .iter()
            .find(|p| p.pane.pane_id() == pane_id)
            .map(|p| p.index)
    }

    /// Detaches the dragged pane from its tab, removing the tab if it
    /// became empty. Returns the pane.
    fn detach_pane(&mut self, drag: &PaneDrag) -> anyhow::Result<Arc<dyn Pane>> {
        let mux = Mux::get();
        let tab = mux
            .get_tab(drag.tab_id)
            .ok_or_else(|| anyhow::anyhow!("tab {} not found", drag.tab_id))?;
        tab.set_zoomed(false);
        let pane = tab
            .remove_pane(drag.pane_id)
            .ok_or_else(|| anyhow::anyhow!("pane {} not in tab", drag.pane_id))?;
        if tab.iter_panes_ignoring_zoom().is_empty() {
            mux.remove_tab(tab.tab_id());
        }
        Ok(pane)
    }

    /// Creates a new tab holding `pane` in the given window
    fn new_tab_with_pane(
        &mut self,
        pane: Arc<dyn Pane>,
        window_id: WindowId,
        activate: bool,
    ) -> anyhow::Result<()> {
        let mux = Mux::get();
        let size = self.terminal_size;
        let tab = Arc::new(Tab::new(&size));
        pane.resize(size)?;
        tab.assign_pane(&pane);
        mux.add_tab_no_panes(&tab);
        mux.add_tab_to_window(&tab, window_id)?;
        if activate {
            if let Some(mut window) = mux.get_window_mut(window_id) {
                let n = window.count_tabs();
                if n > 0 {
                    window.set_active_tab_idx_without_saving(n - 1);
                }
            }
        }
        Ok(())
    }

    fn apply_pane_drop(&mut self, drag: &PaneDrag, target: PaneDropTarget) -> anyhow::Result<()> {
        let mux = Mux::get();
        match target {
            PaneDropTarget::None => Ok(()),
            PaneDropTarget::Swap { pane_id, .. } => {
                let tab = mux
                    .get_tab(drag.tab_id)
                    .ok_or_else(|| anyhow::anyhow!("tab not found"))?;
                tab.set_zoomed(false);
                let Some(src) = Self::pane_index_in_tab(&tab, drag.pane_id) else {
                    return Ok(());
                };
                let Some(dst) = Self::pane_index_in_tab(&tab, pane_id) else {
                    return Ok(());
                };
                tab.set_active_idx(src);
                tab.swap_active_with_index(dst, true);
                Ok(())
            }
            PaneDropTarget::Split {
                pane_id,
                direction,
                second,
                ..
            } => {
                let tab = mux
                    .get_tab(drag.tab_id)
                    .ok_or_else(|| anyhow::anyhow!("tab not found"))?;
                if tab.iter_panes_ignoring_zoom().len() < 2 {
                    return Ok(());
                }
                tab.set_zoomed(false);
                let pane = tab
                    .remove_pane(drag.pane_id)
                    .ok_or_else(|| anyhow::anyhow!("pane not in tab"))?;
                let Some(dst) = Self::pane_index_in_tab(&tab, pane_id) else {
                    return Ok(());
                };
                let idx = tab.split_and_insert(
                    dst,
                    SplitRequest {
                        direction,
                        target_is_second: second,
                        top_level: false,
                        size: SplitSize::Percent(50),
                    },
                    pane,
                )?;
                tab.set_active_idx(idx);
                Ok(())
            }
            PaneDropTarget::Tab { tab_idx } => {
                let dest = mux
                    .get_window(self.mux_window_id)
                    .and_then(|w| w.get_tab_at_idx(tab_idx).map(Arc::clone))
                    .ok_or_else(|| anyhow::anyhow!("no tab at {tab_idx}"))?;
                if dest.tab_id() == drag.tab_id {
                    return Ok(());
                }
                let pane = self.detach_pane(drag)?;
                dest.set_zoomed(false);
                let active = dest.get_active_idx();
                let idx = dest.split_and_insert(
                    active,
                    SplitRequest {
                        direction: SplitDirection::Horizontal,
                        target_is_second: true,
                        top_level: false,
                        size: SplitSize::Percent(50),
                    },
                    pane,
                )?;
                dest.set_active_idx(idx);
                if let Some(mut window) = mux.get_window_mut(self.mux_window_id) {
                    if let Some(i) = window.get_tab_idx_for_id(dest.tab_id()) {
                        window.set_active_tab_idx_without_saving(i);
                    }
                }
                Ok(())
            }
            PaneDropTarget::NewTab => {
                let src_is_alone = mux
                    .get_tab(drag.tab_id)
                    .map(|t| t.iter_panes_ignoring_zoom().len() < 2)
                    .unwrap_or(true);
                if src_is_alone {
                    return Ok(());
                }
                let pane = self.detach_pane(drag)?;
                self.new_tab_with_pane(pane, self.mux_window_id, true)
            }
            PaneDropTarget::Workspace { name, .. } => {
                let current = mux
                    .get_window(self.mux_window_id)
                    .map(|w| w.get_workspace().to_string())
                    .unwrap_or_default();
                let src_is_alone = mux
                    .get_tab(drag.tab_id)
                    .map(|t| t.iter_panes_ignoring_zoom().len() < 2)
                    .unwrap_or(true);
                if current == name && src_is_alone {
                    return Ok(());
                }
                let pane = self.detach_pane(drag)?;
                match mux.iter_windows_in_workspace(&name).into_iter().next() {
                    Some(window_id) => self.new_tab_with_pane(pane, window_id, true)?,
                    None => {
                        let builder = mux.new_empty_window(Some(name), None);
                        self.new_tab_with_pane(pane, *builder, true)?;
                        drop(builder);
                    }
                }
                mux.prune_dead_windows();
                Ok(())
            }
            PaneDropTarget::NewWindow { x, y } => {
                let src_is_alone = mux
                    .get_tab(drag.tab_id)
                    .map(|t| t.iter_panes_ignoring_zoom().len() < 2)
                    .unwrap_or(true);
                let only_tab = mux
                    .get_window(self.mux_window_id)
                    .map(|w| w.count_tabs() < 2)
                    .unwrap_or(true);
                if src_is_alone && only_tab {
                    return Ok(());
                }
                let workspace = mux
                    .get_window(self.mux_window_id)
                    .map(|w| w.get_workspace().to_string());
                let pane = self.detach_pane(drag)?;
                let builder = mux.new_empty_window(
                    workspace,
                    Some(GuiPosition {
                        x: Dimension::Pixels((x - 40).max(0) as f32),
                        y: Dimension::Pixels((y - 10).max(0) as f32),
                        origin: GeometryOrigin::ScreenCoordinateSystem,
                    }),
                );
                self.new_tab_with_pane(pane, *builder, true)?;
                drop(builder);
                mux.prune_dead_windows();
                Ok(())
            }
        }
    }

    /// Draws the translucent drop-zone highlight (in the layered pass)
    pub fn paint_pane_drag_zone(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let Some(drag) = self.pane_drag.as_ref() else {
            return Ok(());
        };
        if !drag.active {
            return Ok(());
        }
        let current = drag.current.clone();
        let target = self.pane_drop_target(&current);
        let accent = self.palette().cursor_bg.to_linear();
        match target {
            PaneDropTarget::Split { rect, .. } => {
                self.filled_rectangle(layers, 2, rect, accent.mul_alpha(0.35))?;
            }
            PaneDropTarget::Swap { rect, .. } => {
                self.filled_rectangle(layers, 2, rect, accent.mul_alpha(0.2))?;
            }
            _ => {}
        }
        Ok(())
    }

    /// Draws the ghost label and any chrome highlight (after the layered pass)
    pub fn paint_pane_drag(&mut self) -> anyhow::Result<()> {
        let Some(drag) = self.pane_drag.as_ref() else {
            return Ok(());
        };
        if !drag.active {
            return Ok(());
        }
        let title = drag.title.clone();
        let current = drag.current.clone();
        let target = self.pane_drop_target(&current);
        let label = match &target {
            PaneDropTarget::Split {
                direction, second, ..
            } => {
                let side = match (direction, second) {
                    (SplitDirection::Horizontal, false) => "left of",
                    (SplitDirection::Horizontal, true) => "right of",
                    (SplitDirection::Vertical, false) => "above",
                    (SplitDirection::Vertical, true) => "below",
                };
                format!("{title}  → {side} pane")
            }
            PaneDropTarget::Swap { .. } => format!("{title}  ⇄ swap"),
            PaneDropTarget::Tab { tab_idx } => format!("{title}  → tab {}", tab_idx + 1),
            PaneDropTarget::NewTab => format!("{title}  → new tab"),
            PaneDropTarget::Workspace { name, .. } => format!("{title}  → {name}"),
            PaneDropTarget::NewWindow { .. } => format!("{title}  → new window"),
            PaneDropTarget::None => title.clone(),
        };
        self.paint_drag_ghost(label, current.coords.x as f32, current.coords.y as f32)?;
        if let PaneDropTarget::Workspace { row, .. } = target {
            self.paint_sidebar_row_highlight(row)?;
        }
        Ok(())
    }
}
