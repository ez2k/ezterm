//! A left-hand sidebar listing mux workspaces.
//! Enter or a click on the selected row switches to that workspace;
//! `n` prompts for a name and creates/switches to it.
use crate::termwindow::TermWindowNotif;
use config::keyassignment::KeyAssignment;
use mux::pane::PaneId;
use mux::tab::TabId;
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use std::collections::HashMap;
use std::sync::{LazyLock, Mutex};
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;
use window::WindowOps;

/// rows above the listing (title + help line)
const HEADER_ROWS: usize = 2;
/// rows consumed by header + status
const ROW_OVERHEAD: usize = 3;

/// Tracks the workspace sidebar pane open in each tab so that
/// ShowWorkspaceSidebar acts as a toggle.
static OPEN_SIDEBARS: LazyLock<Mutex<HashMap<TabId, PaneId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_sidebar(tab_id: TabId, pane_id: PaneId) {
    OPEN_SIDEBARS.lock().unwrap().insert(tab_id, pane_id);
}

pub fn take_sidebar(tab_id: TabId) -> Option<PaneId> {
    OPEN_SIDEBARS.lock().unwrap().remove(&tab_id)
}

pub fn unregister_sidebar(tab_id: TabId, pane_id: PaneId) {
    let mut open = OPEN_SIDEBARS.lock().unwrap();
    if open.get(&tab_id) == Some(&pane_id) {
        open.remove(&tab_id);
    }
}

struct NamePromptHost {
    history: BasicHistory,
}

impl LineEditorHost for NamePromptHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        let (line, _cursor) = editor.get_line_and_cursor();
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => line.is_empty().then_some(Action::Cancel),
            _ => None,
        }
    }
}

struct WsEntry {
    name: String,
    windows: usize,
    is_active: bool,
}

struct WorkspaceSidebar {
    window: ::window::Window,
    pane_id: PaneId,
    entries: Vec<WsEntry>,
    active_idx: usize,
    top_row: usize,
    max_items: usize,
    status: String,
    prev_mouse_buttons: MouseButtons,
}

impl WorkspaceSidebar {
    fn new(window: ::window::Window, pane_id: PaneId) -> Self {
        let mut me = Self {
            window,
            pane_id,
            entries: vec![],
            active_idx: 0,
            top_row: 0,
            max_items: 0,
            status: String::new(),
            prev_mouse_buttons: MouseButtons::NONE,
        };
        me.refresh();
        me
    }

    fn refresh(&mut self) {
        let mux = Mux::get();
        let active = mux.active_workspace();
        let mut names = mux.iter_workspaces();
        names.sort();
        self.entries = names
            .into_iter()
            .map(|name| WsEntry {
                windows: mux.iter_windows_in_workspace(&name).len(),
                is_active: name == active,
                name,
            })
            .collect();
        if let Some(idx) = self.entries.iter().position(|e| e.is_active) {
            self.active_idx = idx;
        } else {
            self.active_idx = self.active_idx.min(self.entries.len().saturating_sub(1));
        }
        self.top_row = self.top_row.min(self.active_idx);
    }

    fn switch_to(&mut self, name: String) {
        self.window.notify(TermWindowNotif::PerformAssignment {
            pane_id: self.pane_id,
            assignment: KeyAssignment::SwitchToWorkspace {
                name: Some(name.clone()),
                spawn: None,
            },
            tx: None,
        });
        self.status = format!("Switched to {name}");
    }

    fn switch_selected(&mut self) {
        if let Some(entry) = self.entries.get(self.active_idx) {
            let name = entry.name.clone();
            self.switch_to(name);
        }
    }

    fn prompt_new_workspace(&mut self, term: &mut TermWizTerminal) {
        let _ = term.render(&[
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            Change::ClearScreen(ColorAttribute::Default),
            Change::Text("Create/switch to workspace.\r\n".to_string()),
        ]);
        let mut host = NamePromptHost {
            history: BasicHistory::default(),
        };
        let mut editor = LineEditor::new(term);
        editor.set_prompt("Workspace name: ");
        match editor.read_line(&mut host) {
            Ok(Some(line)) if !line.trim().is_empty() => {
                self.switch_to(line.trim().to_string());
            }
            _ => {
                self.status = "Cancelled".to_string();
            }
        }
    }

    fn move_up(&mut self, count: usize) {
        self.active_idx = self.active_idx.saturating_sub(count);
        if self.active_idx < self.top_row {
            self.top_row = self.active_idx;
        }
    }

    fn move_down(&mut self, count: usize) {
        if self.entries.is_empty() {
            return;
        }
        self.active_idx = (self.active_idx + count).min(self.entries.len() - 1);
        if self.active_idx > self.top_row + self.max_items {
            self.top_row = self.active_idx.saturating_sub(self.max_items);
        }
    }

    fn render(&mut self, term: &mut TermWizTerminal) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(1);
        self.max_items = size.rows.saturating_sub(ROW_OVERHEAD);

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            AttributeChange::Reverse(true).into(),
            Change::Text(truncate_right(" Workspaces ", max_width)),
            AttributeChange::Reverse(false).into(),
            Change::Text("\r\n".to_string()),
            Change::Text(format!(
                "{}\r\n",
                truncate_right(
                    "Enter/click: switch  n: new  r: refresh  q: quit",
                    max_width
                )
            )),
            Change::AllAttributes(CellAttributes::default()),
        ];

        for (row_num, (entry_idx, entry)) in self
            .entries
            .iter()
            .enumerate()
            .skip(self.top_row)
            .enumerate()
        {
            if row_num > self.max_items {
                break;
            }
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(true).into());
            }
            let marker = if entry.is_active { "*" } else { " " };
            let line = format!(" {} {} ({})", marker, entry.name, entry.windows);
            changes.push(Change::Text(truncate_right(&line, max_width)));
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::Text("\r\n".to_string()));
        }

        term.render(&changes)?;
        if !self.status.is_empty() {
            term.render(&[
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(size.rows.saturating_sub(1)),
                },
                Change::ClearToEndOfLine(ColorAttribute::Default),
                Change::Text(truncate_right(&self.status, max_width)),
            ])?;
        }
        Ok(())
    }

    fn run_loop(&mut self, term: &mut TermWizTerminal) -> anyhow::Result<()> {
        self.render(term)?;
        while let Ok(Some(event)) = term.poll_input(None) {
            match event {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('q'),
                    modifiers: Modifiers::NONE,
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Escape,
                    ..
                }) => break,
                InputEvent::Key(KeyEvent {
                    key: KeyCode::UpArrow,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('k'),
                    modifiers: Modifiers::NONE,
                }) => self.move_up(1),
                InputEvent::Key(KeyEvent {
                    key: KeyCode::DownArrow,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('j'),
                    modifiers: Modifiers::NONE,
                }) => self.move_down(1),
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Enter,
                    ..
                }) => {
                    self.status.clear();
                    self.switch_selected();
                    self.refresh();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('n'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.status.clear();
                    self.prompt_new_workspace(term);
                    self.refresh();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('r'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.status.clear();
                    self.refresh();
                }
                InputEvent::Mouse(MouseEvent { mouse_buttons, .. })
                    if mouse_buttons.contains(MouseButtons::VERT_WHEEL) =>
                {
                    if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                        self.move_up(3);
                    } else {
                        self.move_down(3);
                    }
                }
                InputEvent::Mouse(MouseEvent {
                    y, mouse_buttons, ..
                }) => {
                    let left_edge = mouse_buttons.contains(MouseButtons::LEFT)
                        && !self.prev_mouse_buttons.contains(MouseButtons::LEFT);
                    self.prev_mouse_buttons = mouse_buttons;
                    if left_edge {
                        let row = y as usize;
                        if row >= HEADER_ROWS {
                            let idx = self.top_row + (row - HEADER_ROWS);
                            if idx < self.entries.len() {
                                if idx == self.active_idx {
                                    self.status.clear();
                                    self.switch_selected();
                                    self.refresh();
                                } else {
                                    self.active_idx = idx;
                                }
                            }
                        }
                    }
                }
                InputEvent::Resized { .. } => {}
                _ => {}
            }
            self.render(term)?;
        }
        Ok(())
    }
}

pub fn workspace_sidebar(
    mut term: TermWizTerminal,
    window: ::window::Window,
    pane_id: PaneId,
) -> anyhow::Result<()> {
    // enable mouse reporting so clicks and wheel reach us
    term.set_raw_mode()?;
    term.render(&[Change::Title("Workspaces".to_string())])?;
    let mut state = WorkspaceSidebar::new(window, pane_id);
    state.run_loop(&mut term)
}
