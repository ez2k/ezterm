//! A left-hand workspace sidebar rendered as part of the window chrome,
//! like the tab bar: it spans the full height beside the panes, and stays
//! in place across tab and workspace switches.
use crate::quad::TripleLayerQuadAllocator;
use crate::tabbar::parse_status_text;
use crate::termwindow::render::RenderScreenLineParams;
use config::keyassignment::KeyAssignment;
use mux::renderable::RenderableDimensions;
use mux::Mux;
use termwiz::cell::CellAttributes;
use wezterm_term::color::ColorAttribute;
use window::color::LinearRgba;

/// rows above the workspace list (the "Workspaces" header)
const HEADER_ROWS: usize = 1;

pub struct WorkspaceSidebarEntry {
    pub name: String,
    pub windows: usize,
    pub is_active: bool,
}

impl crate::TermWindow {
    /// The workspaces to show, sorted by name, with the active one flagged
    pub fn workspace_sidebar_entries(&self) -> Vec<WorkspaceSidebarEntry> {
        let mux = Mux::get();
        let active = mux.active_workspace();
        let mut names = mux.iter_workspaces();
        names.sort();
        names
            .into_iter()
            .map(|name| WorkspaceSidebarEntry {
                windows: mux.iter_windows_in_workspace(&name).len(),
                is_active: name == active,
                name,
            })
            .collect()
    }

    /// The pixel y at which the sidebar starts: the very top of the
    /// window, beside the tab bar, which is shifted right to make room.
    pub fn workspace_sidebar_top(&self) -> f32 {
        self.get_os_border().top.get() as f32
    }

    /// Handles a left click on the given sidebar row (0 = header):
    /// a single click switches to the workspace, a double click prompts
    /// to rename it.
    pub fn workspace_sidebar_click(&mut self, row: usize) {
        if row < HEADER_ROWS {
            return;
        }
        let entries = self.workspace_sidebar_entries();
        let Some(entry) = entries.get(row - HEADER_ROWS) else {
            return;
        };

        let now = std::time::Instant::now();
        let is_double = matches!(
            self.workspace_sidebar_last_click,
            Some((when, prev_row)) if prev_row == row
                && now.duration_since(when) < std::time::Duration::from_millis(500)
        );
        self.workspace_sidebar_last_click = if is_double { None } else { Some((now, row)) };

        if is_double {
            self.workspace_sidebar_rename(entry.name.clone());
            return;
        }
        if entry.is_active {
            return;
        }
        let assignment = KeyAssignment::SwitchToWorkspace {
            name: Some(entry.name.clone()),
            spawn: None,
        };
        if let Some(pane) = self.get_active_pane_or_overlay() {
            if let Err(err) = self.perform_key_assignment(&pane, &assignment) {
                log::error!("workspace sidebar: {err:#}");
            }
        }
    }

    /// Handles a right click on the given sidebar row: shows a menu for
    /// the workspace under the cursor, or a generic one on the header
    /// and blank rows.
    pub fn workspace_sidebar_context_menu(&mut self, row: usize, x: f32, y: f32) {
        use crate::termwindow::context_menu::MenuItem;
        let entries = self.workspace_sidebar_entries();
        let entry = if row >= HEADER_ROWS {
            entries.get(row - HEADER_ROWS)
        } else {
            None
        };

        let mut items = vec![];
        if let Some(entry) = entry {
            let name = entry.name.clone();
            if !entry.is_active {
                items.push(MenuItem::assignment(
                    format!("Switch to '{name}'"),
                    KeyAssignment::SwitchToWorkspace {
                        name: Some(name.clone()),
                        spawn: None,
                    },
                ));
            }
            items.push(MenuItem::callback("Rename workspace...", {
                let name = name.clone();
                move |tw| tw.workspace_sidebar_rename(name.clone())
            }));
            items.push(MenuItem::assignment(
                "New window in workspace",
                KeyAssignment::Multiple(vec![
                    KeyAssignment::SwitchToWorkspace {
                        name: Some(name.clone()),
                        spawn: None,
                    },
                    KeyAssignment::SpawnWindow,
                ]),
            ));
            items.push(MenuItem::separator());
            items.push(MenuItem::callback("Close workspace...", {
                let name = name.clone();
                move |tw| tw.close_workspace_with_confirmation(name.clone())
            }));
            items.push(MenuItem::separator());
        }
        items.push(MenuItem::callback("New workspace...", |tw| {
            tw.workspace_sidebar_new_workspace()
        }));
        items.push(MenuItem::assignment(
            "Hide sidebar",
            KeyAssignment::ShowWorkspaceSidebar,
        ));
        self.show_menu_at(items, x, y);
    }

    /// Prompts for the name of a new workspace and switches to it,
    /// spawning its first window.
    fn workspace_sidebar_new_workspace(&mut self) {
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::rename_workspace::new_workspace_prompt(term, window)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    /// Shows a confirmation dialog summarising what closing the workspace
    /// would kill (windows, tabs, running programs) and closes it on
    /// confirmation.
    pub fn close_workspace_with_confirmation(&mut self, name: String) {
        let mux = Mux::get();
        let windows = mux.iter_windows_in_workspace(&name);
        let mut tabs = 0;
        let mut panes = 0;
        let mut procs: Vec<String> = vec![];
        for window_id in &windows {
            let Some(window) = mux.get_window(*window_id) else {
                continue;
            };
            for tab in window.iter_tabs() {
                tabs += 1;
                for pos in tab.iter_panes_ignoring_zoom() {
                    panes += 1;
                    if let Some(proc_name) = pos
                        .pane
                        .get_foreground_process_name(mux::pane::CachePolicy::AllowStale)
                    {
                        let base = std::path::Path::new(&proc_name)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or(proc_name);
                        if !procs.contains(&base) {
                            procs.push(base);
                        }
                    }
                }
            }
        }
        let is_last = mux.iter_workspaces().len() <= 1;
        let mut message = format!(
            "🛑 Really close workspace '{name}'?\n\n\
             This will kill {} window(s), {tabs} tab(s) and {panes} pane(s).",
            windows.len()
        );
        if !procs.is_empty() {
            message.push_str(&format!("\nRunning: {}", procs.join(", ")));
        }
        if is_last {
            message.push_str("\n\nThis is the last workspace; closing it will quit ezterm.");
        }

        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |tab_id, term| {
            crate::overlay::confirm_close_workspace(name, message, term, window, tab_id)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    /// Opens an overlay prompting for a new name for the workspace
    fn workspace_sidebar_rename(&mut self, old_name: String) {
        let mux = Mux::get();
        let Some(tab) = mux.get_active_tab_for_window(self.mux_window_id) else {
            return;
        };
        let Some(window) = self.window.clone() else {
            return;
        };
        let (overlay, future) = crate::overlay::start_overlay(self, &tab, move |_tab_id, term| {
            crate::overlay::rename_workspace::rename_workspace_prompt(term, old_name, window)
        });
        self.assign_overlay(tab.tab_id(), overlay);
        promise::spawn::spawn(future).detach();
    }

    pub fn paint_workspace_sidebar(
        &mut self,
        layers: &mut TripleLayerQuadAllocator,
    ) -> anyhow::Result<()> {
        let width_cells = self.config.workspace_sidebar_width;
        if width_cells == 0 {
            return Ok(());
        }
        let border = self.get_os_border();
        let cell_height = self.render_metrics.cell_size.height as f32;
        let cell_width = self.render_metrics.cell_size.width as f32;
        let top = self.workspace_sidebar_top();
        let bottom = self.dimensions.pixel_height as f32 - border.bottom.get() as f32;
        let rows = ((bottom - top) / cell_height).floor().max(0.) as usize;
        if rows == 0 {
            return Ok(());
        }

        let palette = self.palette().clone();
        let window_is_transparent =
            !self.window_background.is_empty() || self.config.window_background_opacity != 1.0;
        let gl_state = self.render_state.as_ref().unwrap();
        let white_space = gl_state.util_sprites.white_space.texture_coords();
        let filled_box = gl_state.util_sprites.filled_box.texture_coords();
        let default_bg = palette
            .resolve_bg(ColorAttribute::Default)
            .to_linear()
            .mul_alpha(if window_is_transparent {
                0.
            } else {
                self.config.text_background_opacity
            });

        // Build the text for each row: header, then one row per workspace,
        // then blank rows. Every row is padded to the sidebar width and
        // ends with a separator glyph.
        let text_width = width_cells.saturating_sub(1);
        let fit = |s: &str| -> String {
            let mut out = String::new();
            let mut w = 0;
            for ch in s.chars() {
                let cw = termwiz::cell::unicode_column_width(ch.encode_utf8(&mut [0u8; 4]), None);
                if w + cw > text_width {
                    break;
                }
                out.push(ch);
                w += cw;
            }
            while w < text_width {
                out.push(' ');
                w += 1;
            }
            out.push('\u{2502}');
            out
        };

        let entries = self.workspace_sidebar_entries();
        let mut lines: Vec<(String, bool)> = Vec::with_capacity(rows);
        lines.push((fit(" Workspaces"), true));
        for (idx, entry) in entries.iter().enumerate() {
            let marker = if entry.is_active { "*" } else { " " };
            lines.push((
                fit(&format!(
                    "{marker}{:>2} {} ({})",
                    idx + 1,
                    entry.name,
                    entry.windows
                )),
                entry.is_active,
            ));
        }
        while lines.len() < rows {
            lines.push((fit(""), false));
        }
        lines.truncate(rows);

        for (idx, (text, highlight)) in lines.iter().enumerate() {
            let mut attrs = CellAttributes::default();
            if *highlight {
                attrs.set_reverse(true);
            }
            let line = parse_status_text(text, attrs);
            self.render_screen_line(
                RenderScreenLineParams {
                    top_pixel_y: top + idx as f32 * cell_height,
                    left_pixel_x: border.left.get() as f32,
                    pixel_width: width_cells as f32 * cell_width,
                    stable_line_idx: None,
                    line: &line,
                    selection: 0..0,
                    cursor: &Default::default(),
                    palette: &palette,
                    dims: &RenderableDimensions {
                        cols: width_cells,
                        physical_top: 0,
                        scrollback_rows: 0,
                        scrollback_top: 0,
                        viewport_rows: 1,
                        dpi: self.terminal_size.dpi,
                        pixel_height: self.render_metrics.cell_size.height as usize,
                        pixel_width: width_cells * self.render_metrics.cell_size.width as usize,
                        reverse_video: false,
                    },
                    config: &self.config,
                    cursor_border_color: LinearRgba::default(),
                    foreground: palette.foreground.to_linear(),
                    pane: None,
                    is_active: true,
                    selection_fg: LinearRgba::default(),
                    selection_bg: LinearRgba::default(),
                    cursor_fg: LinearRgba::default(),
                    cursor_bg: LinearRgba::default(),
                    cursor_is_default_color: true,
                    white_space,
                    filled_box,
                    window_is_transparent,
                    default_bg,
                    style: None,
                    font: None,
                    use_pixel_positioning: self.config.experimental_pixel_positioning,
                    render_metrics: self.render_metrics,
                    shape_key: None,
                    password_input: false,
                },
                layers,
            )?;
        }

        Ok(())
    }
}
