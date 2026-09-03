//! A small right-click style context menu, rendered as a modal using the
//! box model, positioned at the mouse cursor.  Items either perform a
//! `KeyAssignment` or run an arbitrary callback against the window.
use crate::termwindow::box_model::*;
use crate::termwindow::modal::Modal;
use crate::termwindow::render::corners::{
    BOTTOM_LEFT_ROUNDED_CORNER, BOTTOM_RIGHT_ROUNDED_CORNER, TOP_LEFT_ROUNDED_CORNER,
    TOP_RIGHT_ROUNDED_CORNER,
};
use crate::termwindow::{DimensionContext, TermWindow, UIItemType};
use crate::utilsprites::RenderMetrics;
use config::keyassignment::KeyAssignment;
use config::Dimension;
use mux::Mux;
use std::cell::{Ref, RefCell};
use std::rc::Rc;
use wezterm_term::{KeyCode, KeyModifiers, MouseEvent};
use window::color::LinearRgba;
use window::WindowOps;

pub type MenuCallback = Rc<dyn Fn(&mut TermWindow)>;

pub enum MenuAction {
    Assignment(KeyAssignment),
    Callback(MenuCallback),
    Separator,
}

pub struct MenuItem {
    pub label: String,
    pub action: MenuAction,
}

impl MenuItem {
    pub fn assignment(label: impl Into<String>, assignment: KeyAssignment) -> Self {
        Self {
            label: label.into(),
            action: MenuAction::Assignment(assignment),
        }
    }

    pub fn callback<F: Fn(&mut TermWindow) + 'static>(label: impl Into<String>, f: F) -> Self {
        Self {
            label: label.into(),
            action: MenuAction::Callback(Rc::new(f)),
        }
    }

    pub fn separator() -> Self {
        Self {
            label: String::new(),
            action: MenuAction::Separator,
        }
    }

    fn is_separator(&self) -> bool {
        matches!(self.action, MenuAction::Separator)
    }
}

pub struct ContextMenu {
    items: Vec<MenuItem>,
    /// pixel position of the top-left corner (usually the mouse cursor)
    x: f32,
    y: f32,
    selected: RefCell<Option<usize>>,
    element: RefCell<Option<Vec<ComputedElement>>>,
}

impl ContextMenu {
    pub fn new(items: Vec<MenuItem>, x: f32, y: f32) -> Self {
        Self {
            items,
            x,
            y,
            selected: RefCell::new(None),
            element: RefCell::new(None),
        }
    }

    /// Runs the item at `idx`, closing the menu first
    pub fn activate(&self, idx: usize, term_window: &mut TermWindow) {
        let Some(item) = self.items.get(idx) else {
            return;
        };
        term_window.cancel_modal();
        match &item.action {
            MenuAction::Separator => {}
            MenuAction::Assignment(assignment) => {
                if let Some(pane) = term_window.get_active_pane_or_overlay() {
                    if let Err(err) = term_window.perform_key_assignment(&pane, assignment) {
                        log::error!("context menu: error performing {assignment:?}: {err:#}");
                    }
                }
            }
            MenuAction::Callback(f) => {
                let f = Rc::clone(f);
                f(term_window);
            }
        }
    }

    fn move_selection(&self, delta: isize) {
        let n = self.items.len();
        if n == 0 {
            return;
        }
        let mut sel = self.selected.borrow_mut();
        let mut idx = match *sel {
            Some(i) => i as isize,
            None if delta > 0 => -1,
            None => n as isize,
        };
        for _ in 0..n {
            idx = (idx + delta).rem_euclid(n as isize);
            if !self.items[idx as usize].is_separator() {
                *sel = Some(idx as usize);
                return;
            }
        }
    }

    fn compute(&self, term_window: &mut TermWindow) -> anyhow::Result<Vec<ComputedElement>> {
        let font = term_window
            .fonts
            .command_palette_font()
            .expect("to resolve command palette font");
        let metrics = RenderMetrics::with_font_metrics(&font.metrics())
            .scale_line_height(term_window.config.command_palette_line_height);

        let bg: InheritableColor = term_window
            .config
            .command_palette_bg_color
            .to_linear()
            .into();
        let fg: InheritableColor = term_window
            .config
            .command_palette_fg_color
            .to_linear()
            .into();
        let selected = *self.selected.borrow();

        let mut rows = vec![];
        let max_label = self
            .items
            .iter()
            .map(|i| termwiz::cell::unicode_column_width(&i.label, None))
            .max()
            .unwrap_or(8)
            .max(8);

        for (idx, item) in self.items.iter().enumerate() {
            if item.is_separator() {
                rows.push(
                    Element::new(&font, ElementContent::Text("─".repeat(max_label + 2)))
                        .colors(ElementColors {
                            border: BorderColor::default(),
                            bg: LinearRgba::TRANSPARENT.into(),
                            text: fg.clone(),
                        })
                        .display(DisplayType::Block),
                );
                continue;
            }
            let is_selected = selected == Some(idx);
            let (row_bg, row_fg) = if is_selected {
                (fg.clone(), bg.clone())
            } else {
                (LinearRgba::TRANSPARENT.into(), fg.clone())
            };
            rows.push(
                Element::new(&font, ElementContent::Text(item.label.clone()))
                    .item_type(UIItemType::ContextMenuItem(idx))
                    .colors(ElementColors {
                        border: BorderColor::default(),
                        bg: row_bg,
                        text: row_fg,
                    })
                    .hover_colors(Some(ElementColors {
                        border: BorderColor::default(),
                        bg: fg.clone(),
                        text: bg.clone(),
                    }))
                    .padding(BoxDimension {
                        left: Dimension::Cells(0.5),
                        right: Dimension::Cells(0.5),
                        top: Dimension::Cells(0.),
                        bottom: Dimension::Cells(0.),
                    })
                    .min_width(Some(Dimension::Percent(1.)))
                    .display(DisplayType::Block),
            );
        }

        let element = Element::new(&font, ElementContent::Children(rows))
            .colors(ElementColors {
                border: BorderColor::new(term_window.config.command_palette_fg_color.to_linear()),
                bg: bg.clone(),
                text: fg.clone(),
            })
            .padding(BoxDimension {
                left: Dimension::Cells(0.25),
                right: Dimension::Cells(0.25),
                top: Dimension::Cells(0.25),
                bottom: Dimension::Cells(0.25),
            })
            .border(BoxDimension::new(Dimension::Pixels(1.)))
            .border_corners(Some(Corners {
                top_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_LEFT_ROUNDED_CORNER,
                },
                top_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: TOP_RIGHT_ROUNDED_CORNER,
                },
                bottom_left: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_LEFT_ROUNDED_CORNER,
                },
                bottom_right: SizedPoly {
                    width: Dimension::Cells(0.25),
                    height: Dimension::Cells(0.25),
                    poly: BOTTOM_RIGHT_ROUNDED_CORNER,
                },
            }))
            .zindex(100);

        let dimensions = term_window.dimensions;
        let win_w = dimensions.pixel_width as f32;
        let win_h = dimensions.pixel_height as f32;

        let layout = |term_window: &mut TermWindow, x: f32, y: f32| {
            term_window.compute_element(
                &LayoutContext {
                    height: DimensionContext {
                        dpi: dimensions.dpi as f32,
                        pixel_max: win_h,
                        pixel_cell: metrics.cell_size.height as f32,
                    },
                    width: DimensionContext {
                        dpi: dimensions.dpi as f32,
                        pixel_max: win_w,
                        pixel_cell: metrics.cell_size.width as f32,
                    },
                    bounds: euclid::rect(x, y, win_w - x, win_h - y),
                    metrics: &metrics,
                    gl_state: term_window.render_state.as_ref().unwrap(),
                    zindex: 100,
                },
                &element,
            )
        };

        // Lay it out at the requested spot, then shift it back into the
        // window if it would overflow the right or bottom edge.
        let computed = layout(term_window, self.x, self.y)?;
        let w = computed.bounds.width();
        let h = computed.bounds.height();
        let x = if self.x + w > win_w {
            (win_w - w).max(0.)
        } else {
            self.x
        };
        let y = if self.y + h > win_h {
            (win_h - h).max(0.)
        } else {
            self.y
        };
        let computed = if x != self.x || y != self.y {
            layout(term_window, x, y)?
        } else {
            computed
        };

        Ok(vec![computed])
    }
}

impl Modal for ContextMenu {
    fn perform_assignment(
        &self,
        _assignment: &KeyAssignment,
        _term_window: &mut TermWindow,
    ) -> bool {
        false
    }

    fn mouse_event(&self, _event: MouseEvent, _term_window: &mut TermWindow) -> anyhow::Result<()> {
        Ok(())
    }

    fn key_down(
        &self,
        key: KeyCode,
        mods: KeyModifiers,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<bool> {
        match (key, mods) {
            (KeyCode::Escape, KeyModifiers::NONE) | (KeyCode::Char('g'), KeyModifiers::CTRL) => {
                term_window.cancel_modal();
                return Ok(true);
            }
            (KeyCode::UpArrow, KeyModifiers::NONE)
            | (KeyCode::Char('k'), KeyModifiers::NONE)
            | (KeyCode::Char('p'), KeyModifiers::CTRL) => {
                self.move_selection(-1);
            }
            (KeyCode::DownArrow, KeyModifiers::NONE)
            | (KeyCode::Char('j'), KeyModifiers::NONE)
            | (KeyCode::Char('n'), KeyModifiers::CTRL) => {
                self.move_selection(1);
            }
            (KeyCode::Enter, KeyModifiers::NONE) => {
                let sel = *self.selected.borrow();
                if let Some(idx) = sel {
                    self.activate(idx, term_window);
                }
                return Ok(true);
            }
            _ => {
                // Swallow everything else while the menu is open
                return Ok(true);
            }
        }
        self.element.borrow_mut().take();
        term_window.invalidate_modal();
        Ok(true)
    }

    fn computed_element(
        &self,
        term_window: &mut TermWindow,
    ) -> anyhow::Result<Ref<'_, [ComputedElement]>> {
        if self.element.borrow().is_none() {
            let element = self.compute(term_window)?;
            self.element.borrow_mut().replace(element);
        }
        Ok(Ref::map(self.element.borrow(), |v| {
            v.as_ref().unwrap().as_slice()
        }))
    }

    fn reconfigure(&self, _term_window: &mut TermWindow) {
        self.element.borrow_mut().take();
    }
}

/// Which part of the window a context menu was requested for
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContextMenuKind {
    /// the terminal area of the active pane
    Terminal,
    /// a tab in the tab bar
    Tab(usize),
}

impl ContextMenuKind {
    fn lua_name(&self) -> &'static str {
        match self {
            Self::Terminal => "terminal",
            Self::Tab(_) => "tab",
        }
    }
}

impl TermWindow {
    pub fn context_menu_is_open(&self) -> bool {
        self.get_modal()
            .map(|m| m.downcast_ref::<ContextMenu>().is_some())
            .unwrap_or(false)
    }

    /// Activates item `idx` of the open context menu, if any
    pub fn context_menu_activate(&mut self, idx: usize) {
        if let Some(modal) = self.get_modal() {
            if let Some(menu) = modal.downcast_ref::<ContextMenu>() {
                menu.activate(idx, self);
            }
        }
    }

    /// Opens a menu with the given items at the given pixel position
    pub fn show_menu_at(&mut self, items: Vec<MenuItem>, x: f32, y: f32) {
        if items.is_empty() {
            return;
        }
        self.set_modal(Rc::new(ContextMenu::new(items, x, y)));
    }

    /// The pixel position of the mouse, or the top-left of the pane
    /// area if the mouse isn't over the window
    fn context_menu_origin(&self) -> (f32, f32) {
        match &self.current_mouse_event {
            Some(event) => (event.coords.x as f32, event.coords.y as f32),
            None => {
                let (padding_left, padding_top) = self.padding_left_top();
                let border = self.get_os_border();
                let top = if self.show_tab_bar && !self.config.tab_bar_at_bottom {
                    self.tab_bar_pixel_height().unwrap_or(0.)
                } else {
                    0.
                };
                (
                    padding_left + border.left.get() as f32,
                    top + padding_top + border.top.get() as f32,
                )
            }
        }
    }

    /// Requests a context menu of the given kind at the mouse position.
    /// If the config defines a `context-menu` event handler that returns
    /// `false`, the default menu is suppressed.
    pub fn show_context_menu(&mut self, kind: ContextMenuKind) {
        let (x, y) = self.context_menu_origin();
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };

        async fn dispatch(
            lua: Option<Rc<mlua::Lua>>,
            window: crate::termwindow::GuiWin,
            pane: mux_lua::MuxPane,
            kind: ContextMenuKind,
            x: f32,
            y: f32,
        ) -> anyhow::Result<()> {
            let show_default = match lua {
                Some(lua) => {
                    let tab_idx = match kind {
                        ContextMenuKind::Tab(idx) => Some(idx),
                        ContextMenuKind::Terminal => None,
                    };
                    let args = lua.pack_multi((window.clone(), pane, kind.lua_name(), tab_idx))?;
                    config::lua::emit_event(&lua, ("context-menu".to_string(), args))
                        .await
                        .map_err(|e| {
                            log::error!("while processing context-menu event: {:#}", e);
                            e
                        })?
                }
                None => true,
            };
            if show_default {
                window
                    .window
                    .notify(crate::termwindow::TermWindowNotif::Apply(Box::new(
                        move |tw| tw.open_default_context_menu(kind, x, y),
                    )));
            }
            Ok(())
        }

        let window = crate::termwindow::GuiWin::new(self);
        let pane = mux_lua::MuxPane(pane.pane_id());
        promise::spawn::spawn(config::with_lua_config_on_main_thread(move |lua| {
            dispatch(lua, window, pane, kind, x, y)
        }))
        .detach();
    }

    fn open_default_context_menu(&mut self, kind: ContextMenuKind, x: f32, y: f32) {
        let items = match kind {
            ContextMenuKind::Terminal => self.terminal_menu_items(),
            ContextMenuKind::Tab(idx) => self.tab_menu_items(idx),
        };
        self.show_menu_at(items, x, y);
    }

    fn terminal_menu_items(&self) -> Vec<MenuItem> {
        use config::keyassignment::{
            ClipboardCopyDestination, ClipboardPasteSource, SpawnCommand, SpawnTabDomain,
        };
        let mut items = vec![];
        if self.current_highlight.is_some() {
            items.push(MenuItem::assignment(
                "Open link",
                KeyAssignment::OpenLinkAtMouseCursor,
            ));
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::assignment(
            "Copy",
            KeyAssignment::CopyTo(ClipboardCopyDestination::ClipboardAndPrimarySelection),
        ));
        items.push(MenuItem::assignment(
            "Paste",
            KeyAssignment::PasteFrom(ClipboardPasteSource::Clipboard),
        ));
        items.push(MenuItem::assignment(
            "Select all (copy mode)",
            KeyAssignment::ActivateCopyMode,
        ));
        items.push(MenuItem::separator());
        items.push(MenuItem::assignment(
            "Split right",
            KeyAssignment::SplitHorizontal(SpawnCommand {
                domain: SpawnTabDomain::CurrentPaneDomain,
                ..Default::default()
            }),
        ));
        items.push(MenuItem::assignment(
            "Split down",
            KeyAssignment::SplitVertical(SpawnCommand {
                domain: SpawnTabDomain::CurrentPaneDomain,
                ..Default::default()
            }),
        ));
        items.push(MenuItem::assignment(
            "Toggle pane zoom",
            KeyAssignment::TogglePaneZoomState,
        ));
        items.push(MenuItem::assignment(
            "Close pane",
            KeyAssignment::CloseCurrentPane { confirm: true },
        ));
        items.push(MenuItem::separator());
        items.push(MenuItem::assignment(
            "New tab",
            KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        ));
        items.push(MenuItem::assignment(
            "Copy working directory",
            KeyAssignment::CopyCurrentWorkingDir,
        ));
        items.push(MenuItem::separator());
        items.push(MenuItem::assignment(
            "File manager",
            KeyAssignment::ShowFileManager,
        ));
        items.push(MenuItem::assignment(
            "Command palette",
            KeyAssignment::ActivateCommandPalette,
        ));
        items
    }

    fn tab_menu_items(&self, tab_idx: usize) -> Vec<MenuItem> {
        use config::keyassignment::SpawnTabDomain;
        let mux = Mux::get();
        let (count, is_active) = match mux.get_window(self.mux_window_id) {
            Some(w) => (w.count_tabs(), w.get_active_tab_idx() == tab_idx),
            None => return vec![],
        };
        let mut items = vec![];
        if !is_active {
            items.push(MenuItem::assignment(
                "Activate tab",
                KeyAssignment::ActivateTab(tab_idx as isize),
            ));
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::assignment(
            "New tab",
            KeyAssignment::SpawnTab(SpawnTabDomain::CurrentPaneDomain),
        ));
        items.push(MenuItem::callback("Duplicate tab", move |tw| {
            tw.with_tab_activated(tab_idx, |tw| tw.duplicate_tab());
        }));
        items.push(MenuItem::callback("Rename tab", move |tw| {
            tw.with_tab_activated(tab_idx, |tw| tw.rename_tab());
        }));
        items.push(MenuItem::separator());
        if tab_idx > 0 {
            items.push(MenuItem::callback("Move tab left", move |tw| {
                tw.with_tab_activated(tab_idx, |tw| {
                    tw.move_tab_relative(-1).ok();
                });
            }));
        }
        if tab_idx + 1 < count {
            items.push(MenuItem::callback("Move tab right", move |tw| {
                tw.with_tab_activated(tab_idx, |tw| {
                    tw.move_tab_relative(1).ok();
                });
            }));
        }
        if tab_idx > 0 {
            items.push(MenuItem::callback("Merge into left tab", move |tw| {
                if let Some(id) = tw.tab_id_at(tab_idx) {
                    tw.merge_tab_into(id, tab_idx - 1);
                }
            }));
        }
        if tab_idx + 1 < count {
            items.push(MenuItem::callback("Merge into right tab", move |tw| {
                if let Some(id) = tw.tab_id_at(tab_idx) {
                    tw.merge_tab_into(id, tab_idx + 1);
                }
            }));
        }
        if count > 1 {
            items.push(MenuItem::callback("Move tab to new window", move |tw| {
                tw.with_tab_activated(tab_idx, |tw| tw.move_tab_to_new_window());
            }));
        }
        items.push(MenuItem::separator());
        items.push(MenuItem::callback("Close tab", move |tw| {
            tw.close_specific_tab(tab_idx, true);
        }));
        if count > 1 {
            items.push(MenuItem::callback("Close other tabs", move |tw| {
                tw.with_tab_activated(tab_idx, |tw| tw.close_other_tabs(true));
            }));
        }
        if tab_idx + 1 < count {
            items.push(MenuItem::callback("Close tabs to the right", move |tw| {
                tw.with_tab_activated(tab_idx, |tw| tw.close_tabs_to_the_right(true));
            }));
        }
        items
    }

    fn tab_id_at(&self, tab_idx: usize) -> Option<mux::tab::TabId> {
        Mux::get()
            .get_window(self.mux_window_id)
            .and_then(|w| w.get_tab_at_idx(tab_idx).map(|t| t.tab_id()))
    }

    /// Activates `tab_idx` and then runs `f`; the tab-scoped actions all
    /// operate on the active tab.
    fn with_tab_activated<F: FnOnce(&mut TermWindow)>(&mut self, tab_idx: usize, f: F) {
        if self.activate_tab(tab_idx as isize).is_ok() {
            f(self);
        }
    }

    fn active_tab_info(&self) -> Option<(usize, Vec<mux::tab::TabId>)> {
        let mux = Mux::get();
        let window = mux.get_window(self.mux_window_id)?;
        let active = window.get_active_tab_idx();
        let ids = window.iter_tabs().map(|t| t.tab_id()).collect();
        Some((active, ids))
    }

    /// Closes the given tabs, prompting once if any of them would
    /// normally ask for confirmation.
    fn close_tabs(&mut self, tab_ids: Vec<mux::tab::TabId>, confirm: bool) {
        if tab_ids.is_empty() {
            return;
        }
        let mux = Mux::get();
        let needs_prompt = confirm
            && tab_ids.iter().any(|id| {
                mux.get_tab(*id)
                    .map(|t| !t.can_close_without_prompting(mux::pane::CloseReason::Tab))
                    .unwrap_or(false)
            });
        if !needs_prompt {
            for id in tab_ids {
                mux.remove_tab(id);
            }
            return;
        }
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let (overlay, future) =
            crate::overlay::start_overlay(self, &tab, move |active_tab_id, term| {
                crate::overlay::confirm_close_tabs(tab_ids, term, window, active_tab_id)
            });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    pub fn close_other_tabs(&mut self, confirm: bool) {
        let Some((active, ids)) = self.active_tab_info() else {
            return;
        };
        let others = ids
            .into_iter()
            .enumerate()
            .filter_map(|(i, id)| if i != active { Some(id) } else { None })
            .collect();
        self.close_tabs(others, confirm);
    }

    pub fn close_tabs_to_the_right(&mut self, confirm: bool) {
        let Some((active, ids)) = self.active_tab_info() else {
            return;
        };
        let right = ids.into_iter().skip(active + 1).collect();
        self.close_tabs(right, confirm);
    }

    /// Spawns a new tab in the same domain and working directory as the
    /// active pane
    pub fn duplicate_tab(&mut self) {
        use config::keyassignment::{SpawnCommand, SpawnTabDomain};
        let Some(pane) = self.get_active_pane_or_overlay() else {
            return;
        };
        let cwd = pane
            .get_current_working_dir(mux::pane::CachePolicy::AllowStale)
            .and_then(|url| {
                if url.scheme() == "file" {
                    url.to_file_path().ok()
                } else {
                    None
                }
            });
        self.spawn_command(
            &SpawnCommand {
                domain: SpawnTabDomain::CurrentPaneDomain,
                cwd,
                ..Default::default()
            },
            crate::spawn::SpawnWhere::NewTab,
        );
    }

    /// Detaches the active tab into a brand new window in the same
    /// workspace
    pub fn move_tab_to_new_window(&mut self) {
        self.move_tab_to_new_window_at(None);
    }

    /// As `move_tab_to_new_window`, placing the new window at `position`
    pub fn move_tab_to_new_window_at(&mut self, position: Option<config::GuiPosition>) {
        let mux = Mux::get();
        let (tab, workspace) = {
            let mut window = match mux.get_window_mut(self.mux_window_id) {
                Some(w) => w,
                None => return,
            };
            if window.count_tabs() < 2 {
                // Already alone in its window; nothing to do
                return;
            }
            let idx = window.get_active_tab_idx();
            let workspace = window.get_workspace().to_string();
            (window.remove_tab_idx(idx), workspace)
        };
        let new_window = mux.new_empty_window(Some(workspace), position);
        if let Err(err) = mux.add_tab_to_window(&tab, *new_window) {
            log::error!("move_tab_to_new_window: {err:#}");
        }
        // Dropping the builder notifies the GUI to create the window
        drop(new_window);
        if let Some(window) = self.window.as_ref() {
            window.invalidate();
        }
    }

    /// Prompts for a new title for the active tab
    pub fn rename_tab(&mut self) {
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let old_name = tab.get_title();
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |tab_id, term| {
            crate::overlay::rename_workspace::rename_tab_prompt(term, tab_id, old_name, window)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    /// Copies the active pane's working directory to the clipboard
    pub fn copy_current_working_dir(&mut self, pane: &std::sync::Arc<dyn mux::pane::Pane>) {
        use config::keyassignment::ClipboardCopyDestination;
        let Some(url) = pane.get_current_working_dir(mux::pane::CachePolicy::AllowStale) else {
            return;
        };
        let text = if url.scheme() == "file" {
            url.to_file_path()
                .ok()
                .map(|p| p.to_string_lossy().to_string())
                .unwrap_or_else(|| url.to_string())
        } else {
            url.to_string()
        };
        self.copy_to_clipboard(ClipboardCopyDestination::ClipboardAndPrimarySelection, text);
    }
}
