//! A simple file manager overlay.
//! For local panes it browses the local filesystem.
//! For panes attached to an ssh domain it browses the remote filesystem
//! over the existing ssh session using SFTP, and supports downloading
//! files to the local machine and uploading local files to the remote.
use smol::io::{AsyncReadExt, AsyncWriteExt};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;
use wezterm_ssh::Sftp;

const CHUNK_SIZE: usize = 128 * 1024;
/// rows consumed by the header + status lines
const ROW_OVERHEAD: usize = 3;

pub enum FileManagerBackend {
    Local,
    Remote { sftp: Sftp, label: String },
}

#[derive(Clone)]
struct FmEntry {
    name: String,
    is_dir: bool,
    size: Option<u64>,
}

struct FileManager {
    backend: FileManagerBackend,
    cwd: String,
    entries: Vec<FmEntry>,
    active_idx: usize,
    top_row: usize,
    max_items: usize,
    status: String,
}

struct PathPromptHost {
    history: BasicHistory,
}

impl LineEditorHost for PathPromptHost {
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

fn human_size(size: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = size as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{size} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn expand_tilde(path: &str) -> PathBuf {
    if let Some(rest) = path.strip_prefix("~/") {
        if let Some(home) = dirs_next::home_dir() {
            return home.join(rest);
        }
    } else if path == "~" {
        if let Some(home) = dirs_next::home_dir() {
            return home;
        }
    }
    PathBuf::from(path)
}

fn download_dir() -> PathBuf {
    dirs_next::download_dir()
        .or_else(dirs_next::home_dir)
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Pick a destination that doesn't clobber an existing local file
fn unique_local_dest(dir: &Path, name: &str) -> PathBuf {
    let candidate = dir.join(name);
    if !candidate.exists() {
        return candidate;
    }
    let (stem, ext) = match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() => (stem.to_string(), format!(".{ext}")),
        _ => (name.to_string(), String::new()),
    };
    for n in 1.. {
        let candidate = dir.join(format!("{stem} ({n}){ext}"));
        if !candidate.exists() {
            return candidate;
        }
    }
    unreachable!();
}

/// Join a path onto a remote directory using `/` separators
fn remote_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

fn remote_parent(path: &str) -> Option<String> {
    if path == "/" {
        return None;
    }
    let trimmed = path.trim_end_matches('/');
    match trimmed.rsplit_once('/') {
        Some(("", _)) => Some("/".to_string()),
        Some((parent, _)) => Some(parent.to_string()),
        None => None,
    }
}

impl FileManager {
    fn new(backend: FileManagerBackend, start_dir: Option<String>) -> anyhow::Result<Self> {
        let cwd = match &backend {
            FileManagerBackend::Local => {
                let dir = start_dir
                    .map(PathBuf::from)
                    .filter(|p| p.is_dir())
                    .or_else(dirs_next::home_dir)
                    .unwrap_or_else(|| PathBuf::from("/"));
                dir.to_string_lossy().to_string()
            }
            FileManagerBackend::Remote { sftp, .. } => {
                let hinted = start_dir.filter(|dir| {
                    smol::block_on(sftp.metadata(dir.as_str()))
                        .map(|meta| meta.is_dir())
                        .unwrap_or(false)
                });
                match hinted {
                    Some(dir) => dir,
                    None => smol::block_on(sftp.canonicalize("."))
                        .map(|p| p.to_string())
                        .unwrap_or_else(|_| "/".to_string()),
                }
            }
        };

        let mut fm = Self {
            backend,
            cwd,
            entries: vec![],
            active_idx: 0,
            top_row: 0,
            max_items: 0,
            status: String::new(),
        };
        fm.reload()?;
        Ok(fm)
    }

    fn is_remote(&self) -> bool {
        matches!(self.backend, FileManagerBackend::Remote { .. })
    }

    fn reload(&mut self) -> anyhow::Result<()> {
        let mut entries = match &self.backend {
            FileManagerBackend::Local => {
                let mut entries = vec![];
                for entry in std::fs::read_dir(&self.cwd)? {
                    let entry = entry?;
                    let meta = entry.metadata();
                    let is_dir = meta.as_ref().map(|m| m.is_dir()).unwrap_or(false);
                    entries.push(FmEntry {
                        name: entry.file_name().to_string_lossy().to_string(),
                        is_dir,
                        size: meta.ok().filter(|m| m.is_file()).map(|m| m.len()),
                    });
                }
                entries
            }
            FileManagerBackend::Remote { sftp, .. } => {
                let listing = smol::block_on(sftp.read_dir(self.cwd.as_str()))
                    .map_err(|err| anyhow::anyhow!("sftp read_dir {}: {err:#}", self.cwd))?;
                listing
                    .into_iter()
                    .filter_map(|(path, meta)| {
                        let name = path.file_name()?.to_string();
                        if name == "." || name == ".." {
                            return None;
                        }
                        Some(FmEntry {
                            name,
                            is_dir: meta.is_dir(),
                            size: if meta.is_file() { meta.size } else { None },
                        })
                    })
                    .collect()
            }
        };
        entries.sort_by(|a, b| b.is_dir.cmp(&a.is_dir).then_with(|| a.name.cmp(&b.name)));
        self.entries = entries;
        self.active_idx = 0;
        self.top_row = 0;
        Ok(())
    }

    fn selected(&self) -> Option<&FmEntry> {
        self.entries.get(self.active_idx)
    }

    fn enter_selected(&mut self) {
        let Some(entry) = self.selected().cloned() else {
            return;
        };
        if !entry.is_dir {
            return;
        }
        let new_cwd = if self.is_remote() {
            remote_join(&self.cwd, &entry.name)
        } else {
            Path::new(&self.cwd)
                .join(&entry.name)
                .to_string_lossy()
                .to_string()
        };
        let prior = std::mem::replace(&mut self.cwd, new_cwd);
        if let Err(err) = self.reload() {
            self.status = format!("Error: {err:#}");
            self.cwd = prior;
            let _ = self.reload();
        }
    }

    fn go_parent(&mut self) {
        let parent = if self.is_remote() {
            remote_parent(&self.cwd)
        } else {
            Path::new(&self.cwd)
                .parent()
                .map(|p| p.to_string_lossy().to_string())
        };
        let Some(parent) = parent else { return };
        let prior = std::mem::replace(&mut self.cwd, parent);
        if let Err(err) = self.reload() {
            self.status = format!("Error: {err:#}");
            self.cwd = prior;
            let _ = self.reload();
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

    fn render_status(&self, term: &mut TermWizTerminalRef, text: &str) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        term.render(&[
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(size.rows.saturating_sub(1)),
            },
            Change::ClearToEndOfLine(ColorAttribute::Default),
            Change::Text(truncate_right(text, size.cols.saturating_sub(1))),
        ])
    }

    fn download_selected(&mut self, term: &mut TermWizTerminalRef) {
        let Some(entry) = self.selected().cloned() else {
            self.status = "Nothing selected".to_string();
            return;
        };
        if entry.is_dir {
            self.status = "Directory download is not supported yet".to_string();
            return;
        }
        let FileManagerBackend::Remote { sftp, .. } = &self.backend else {
            self.status = "Download requires a remote (ssh domain) pane".to_string();
            return;
        };
        let sftp = sftp.clone();
        let remote_path = remote_join(&self.cwd, &entry.name);
        let dest = unique_local_dest(&download_dir(), &entry.name);
        let total = entry.size;

        let result = smol::block_on(async {
            let mut src = sftp.open(remote_path.as_str()).await?;
            let mut out = std::fs::File::create(&dest)?;
            let mut buf = vec![0u8; CHUNK_SIZE];
            let mut copied: u64 = 0;
            loop {
                let n = src.read(&mut buf).await?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n])?;
                copied += n as u64;
                let progress = match total {
                    Some(total) if total > 0 => format!(
                        "Downloading {}: {} / {} ({}%)",
                        entry.name,
                        human_size(copied),
                        human_size(total),
                        copied * 100 / total
                    ),
                    _ => format!("Downloading {}: {}", entry.name, human_size(copied)),
                };
                let _ = self.render_status_raw(term, &progress);
            }
            out.flush()?;
            anyhow::Result::<u64>::Ok(copied)
        });

        match result {
            Ok(copied) => {
                self.status = format!(
                    "Downloaded {} ({}) -> {}",
                    entry.name,
                    human_size(copied),
                    dest.display()
                );
            }
            Err(err) => {
                // don't leave a partial file behind
                let _ = std::fs::remove_file(&dest);
                self.status = format!("Download failed: {err:#}");
            }
        }
    }

    // render_status needs &self only; this alias keeps the async block above
    // free of borrow conflicts with `entry`
    fn render_status_raw(&self, term: &mut TermWizTerminalRef, text: &str) -> termwiz::Result<()> {
        self.render_status(term, text)
    }

    fn upload(&mut self, term: &mut TermWizTerminalRef) {
        let FileManagerBackend::Remote { sftp, .. } = &self.backend else {
            self.status = "Upload requires a remote (ssh domain) pane".to_string();
            return;
        };
        let sftp = sftp.clone();

        let local_path = {
            let _ = term.render(&[
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::ClearScreen(ColorAttribute::Default),
                Change::Text("Upload to the current remote directory.\r\n".to_string()),
            ]);
            let mut host = PathPromptHost {
                history: BasicHistory::default(),
            };
            let mut editor = LineEditor::new(term);
            editor.set_prompt("Local file path: ");
            match editor.read_line(&mut host) {
                Ok(Some(line)) if !line.trim().is_empty() => expand_tilde(line.trim()),
                _ => {
                    self.status = "Upload cancelled".to_string();
                    return;
                }
            }
        };

        if !local_path.is_file() {
            self.status = format!("Not a file: {}", local_path.display());
            return;
        }
        let name = match local_path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => {
                self.status = "Invalid path".to_string();
                return;
            }
        };
        let remote_path = remote_join(&self.cwd, &name);
        let total = std::fs::metadata(&local_path).ok().map(|m| m.len());

        let result = smol::block_on(async {
            let mut src = std::fs::File::open(&local_path)?;
            let mut out = sftp.create(remote_path.as_str()).await?;
            let mut buf = vec![0u8; CHUNK_SIZE];
            let mut copied: u64 = 0;
            loop {
                let n = src.read(&mut buf)?;
                if n == 0 {
                    break;
                }
                out.write_all(&buf[..n]).await?;
                copied += n as u64;
                let progress = match total {
                    Some(total) if total > 0 => format!(
                        "Uploading {}: {} / {} ({}%)",
                        name,
                        human_size(copied),
                        human_size(total),
                        copied * 100 / total
                    ),
                    _ => format!("Uploading {}: {}", name, human_size(copied)),
                };
                let _ = self.render_status_raw(term, &progress);
            }
            out.flush().await?;
            anyhow::Result::<u64>::Ok(copied)
        });

        match result {
            Ok(copied) => {
                self.status = format!(
                    "Uploaded {} ({}) -> {}",
                    name,
                    human_size(copied),
                    remote_path
                );
                let _ = self.reload();
            }
            Err(err) => {
                self.status = format!("Upload failed: {err:#}");
            }
        }
    }

    fn render(&mut self, term: &mut TermWizTerminalRef) -> termwiz::Result<()> {
        let size = term.get_screen_size()?;
        let max_width = size.cols.saturating_sub(2);
        self.max_items = size.rows.saturating_sub(ROW_OVERHEAD);

        let location = match &self.backend {
            FileManagerBackend::Local => format!("local: {}", self.cwd),
            FileManagerBackend::Remote { label, .. } => format!("{label}: {}", self.cwd),
        };

        let help = if self.is_remote() {
            "Enter: open  Backspace: up  d: download  u: upload  r: refresh  q: quit"
        } else {
            "Enter: open  Backspace: up  r: refresh  q: quit (transfers need an ssh domain pane)"
        };

        let mut changes = vec![
            Change::ClearScreen(ColorAttribute::Default),
            Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(0),
            },
            AttributeChange::Reverse(true).into(),
            Change::Text(truncate_right(&format!(" {location} "), max_width)),
            AttributeChange::Reverse(false).into(),
            Change::Text("\r\n".to_string()),
            Change::Text(format!("{}\r\n", truncate_right(help, max_width))),
            Change::AllAttributes(CellAttributes::default()),
        ];

        if self.entries.is_empty() {
            changes.push(Change::Text("  (empty directory)\r\n".to_string()));
        }

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
            let size = match (entry.is_dir, entry.size) {
                (true, _) => "<dir>".to_string(),
                (false, Some(size)) => human_size(size),
                (false, None) => String::new(),
            };
            let marker = if entry.is_dir { "/" } else { "" };
            let line = format!(" {:>9}  {}{}", size, entry.name, marker);
            changes.push(Change::Text(truncate_right(&line, max_width)));
            if entry_idx == self.active_idx {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::Text("\r\n".to_string()));
        }

        term.render(&changes)?;
        if !self.status.is_empty() {
            let status = self.status.clone();
            self.render_status(term, &status)?;
        }
        Ok(())
    }

    fn run_loop(&mut self, term: &mut TermWizTerminalRef) -> anyhow::Result<()> {
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
                }) => {
                    self.move_up(1);
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::DownArrow,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('j'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.move_down(1);
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::PageUp,
                    ..
                }) => {
                    self.move_up(self.max_items.max(1));
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::PageDown,
                    ..
                }) => {
                    self.move_down(self.max_items.max(1));
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Enter,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::RightArrow,
                    ..
                }) => {
                    self.status.clear();
                    self.enter_selected();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Backspace,
                    ..
                })
                | InputEvent::Key(KeyEvent {
                    key: KeyCode::LeftArrow,
                    ..
                }) => {
                    self.status.clear();
                    self.go_parent();
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('r'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.status.clear();
                    if let Err(err) = self.reload() {
                        self.status = format!("Error: {err:#}");
                    }
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('d'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.download_selected(term);
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('u'),
                    modifiers: Modifiers::NONE,
                }) => {
                    self.upload(term);
                }
                InputEvent::Resized { .. } => {}
                _ => {}
            }
            self.render(term)?;
        }
        Ok(())
    }
}

use mux::termwiztermtab::TermWizTerminal as TermWizTerminalRef;

pub fn file_manager(
    mut term: TermWizTerminalRef,
    backend: FileManagerBackend,
    start_dir: Option<String>,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.render(&[Change::Title("File Manager".to_string())])?;

    let mut fm = match FileManager::new(backend, start_dir) {
        Ok(fm) => fm,
        Err(err) => {
            term.render(&[Change::Text(format!(
                "Failed to start file manager: {err:#}\r\nPress any key to close.",
            ))])?;
            let _ = term.poll_input(None);
            return Ok(());
        }
    };
    fm.run_loop(&mut term)
}
