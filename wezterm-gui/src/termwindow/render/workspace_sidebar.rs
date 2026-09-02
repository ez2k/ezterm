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

    /// Handles a left click on the given sidebar row (0 = header)
    pub fn workspace_sidebar_click(&mut self, row: usize) {
        if row < HEADER_ROWS {
            return;
        }
        let entries = self.workspace_sidebar_entries();
        let Some(entry) = entries.get(row - HEADER_ROWS) else {
            return;
        };
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
        for entry in &entries {
            let marker = if entry.is_active { "*" } else { " " };
            lines.push((
                fit(&format!("{marker} {} ({})", entry.name, entry.windows)),
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
