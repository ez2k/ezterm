//! A simple file manager overlay.
//! For local panes it browses the local filesystem.
//! For panes attached to an ssh domain it browses the remote filesystem
//! over the existing ssh session using SFTP, and supports downloading
//! files to the local machine and uploading local files to the remote.
use mux::pane::PaneId;
use mux::tab::TabId;
use smol::io::{AsyncReadExt, AsyncWriteExt};
use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::{LazyLock, Mutex};
use termwiz::cell::{AttributeChange, CellAttributes};
use termwiz::color::ColorAttribute;
use termwiz::input::{InputEvent, KeyCode, KeyEvent, Modifiers, MouseButtons, MouseEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::{Change, Position};
use termwiz::terminal::Terminal;
use termwiz_funcs::truncate_right;
use wezterm_ssh::Sftp;

const CHUNK_SIZE: usize = 128 * 1024;
/// rows consumed by the header + status lines
const ROW_OVERHEAD: usize = 3;
/// number of rows above the file listing (location header + help line)
const HEADER_ROWS: usize = 2;

pub enum FileManagerBackend {
    Local,
    Remote { sftp: Sftp, label: String },
}

/// Tracks the file manager sidebar pane that is open in each tab,
/// so that ShowFileManager can act as a toggle.
static OPEN_SIDEBARS: LazyLock<Mutex<HashMap<TabId, PaneId>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

pub fn register_sidebar(tab_id: TabId, pane_id: PaneId) {
    OPEN_SIDEBARS.lock().unwrap().insert(tab_id, pane_id);
}

/// Removes and returns the sidebar pane registered for the tab, if any
pub fn take_sidebar(tab_id: TabId) -> Option<PaneId> {
    OPEN_SIDEBARS.lock().unwrap().remove(&tab_id)
}

/// Removes the registration only if it still refers to the given pane
pub fn unregister_sidebar(tab_id: TabId, pane_id: PaneId) {
    let mut open = OPEN_SIDEBARS.lock().unwrap();
    if open.get(&tab_id) == Some(&pane_id) {
        open.remove(&tab_id);
    }
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
    /// browser-style navigation history: directories we can go back
    /// and forward to
    back_stack: Vec<String>,
    fwd_stack: Vec<String>,
    /// a single click delivers both a press and a release event with
    /// the button set; this tracks the edge so we act only once per click
    prev_mouse_buttons: MouseButtons,
    /// an open in-pane action menu, if any
    menu: Option<FmMenu>,
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum FmAction {
    Open,
    View,
    Parent,
    Back,
    Forward,
    Refresh,
    Download,
    Upload,
    Rename,
    Delete,
    Quit,
}

impl FmAction {
    fn label(&self) -> &'static str {
        match self {
            Self::Open => "Open",
            Self::View => "View file",
            Self::Parent => "Go to parent",
            Self::Back => "Back",
            Self::Forward => "Forward",
            Self::Refresh => "Refresh",
            Self::Download => "Download",
            Self::Upload => "Upload here...",
            Self::Rename => "Rename...",
            Self::Delete => "Delete...",
            Self::Quit => "Close file manager",
        }
    }
}

/// A small popup menu drawn over the listing
struct FmMenu {
    items: Vec<FmAction>,
    selected: usize,
    /// top-left cell of the box (including its border)
    x: usize,
    y: usize,
}

impl FmMenu {
    fn width(&self) -> usize {
        self.items
            .iter()
            .map(|a| a.label().len())
            .max()
            .unwrap_or(0)
            + 4
    }

    fn height(&self) -> usize {
        self.items.len() + 2
    }

    /// Maps a cell position to an item index, if it is over one
    fn hit(&self, x: usize, y: usize) -> Option<usize> {
        if x > self.x
            && x < self.x + self.width() - 1
            && y > self.y
            && y < self.y + self.height() - 1
        {
            Some(y - self.y - 1)
        } else {
            None
        }
    }
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
            back_stack: vec![],
            fwd_stack: vec![],
            prev_mouse_buttons: MouseButtons::NONE,
            menu: None,
        };
        fm.reload()?;
        Ok(fm)
    }

    /// Change directory, recording history. Restores the prior cwd on error.
    fn navigate_to(&mut self, new_cwd: String, record_history: bool) {
        if new_cwd == self.cwd {
            return;
        }
        let prior = std::mem::replace(&mut self.cwd, new_cwd);
        match self.reload() {
            Ok(()) => {
                if record_history {
                    self.back_stack.push(prior);
                    self.fwd_stack.clear();
                }
            }
            Err(err) => {
                self.status = format!("Error: {err:#}");
                self.cwd = prior;
                let _ = self.reload();
            }
        }
    }

    fn go_back(&mut self) {
        let Some(dir) = self.back_stack.pop() else {
            self.status = "No previous directory".to_string();
            return;
        };
        let prior = std::mem::replace(&mut self.cwd, dir);
        match self.reload() {
            Ok(()) => self.fwd_stack.push(prior),
            Err(err) => {
                self.status = format!("Error: {err:#}");
                self.cwd = prior;
                let _ = self.reload();
            }
        }
    }

    fn go_forward(&mut self) {
        let Some(dir) = self.fwd_stack.pop() else {
            self.status = "No next directory".to_string();
            return;
        };
        let prior = std::mem::replace(&mut self.cwd, dir);
        match self.reload() {
            Ok(()) => self.back_stack.push(prior),
            Err(err) => {
                self.status = format!("Error: {err:#}");
                self.cwd = prior;
                let _ = self.reload();
            }
        }
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

    fn enter_selected(&mut self, term: &mut TermWizTerminalRef) {
        let Some(entry) = self.selected().cloned() else {
            return;
        };
        if !entry.is_dir {
            // opening a file shows it in the viewer
            self.view_selected(term);
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
        self.navigate_to(new_cwd, true);
    }

    /// Loads the selected file's content as lines of text
    fn load_selected_file(&self) -> anyhow::Result<(String, Vec<String>)> {
        const VIEW_MAX: u64 = 4 * 1024 * 1024;

        let entry = self
            .selected()
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("nothing selected"))?;
        if entry.is_dir {
            anyhow::bail!("not a file");
        }
        if let Some(size) = entry.size {
            if size > VIEW_MAX {
                anyhow::bail!("{} is too large to view ({})", entry.name, human_size(size));
            }
        }

        let bytes = match &self.backend {
            FileManagerBackend::Local => std::fs::read(Path::new(&self.cwd).join(&entry.name))?,
            FileManagerBackend::Remote { sftp, .. } => {
                let sftp = sftp.clone();
                let path = remote_join(&self.cwd, &entry.name);
                smol::block_on(async {
                    let mut file = sftp.open(path.as_str()).await?;
                    let mut bytes = Vec::new();
                    let mut buf = vec![0u8; CHUNK_SIZE];
                    loop {
                        let n = file.read(&mut buf).await?;
                        if n == 0 {
                            break;
                        }
                        bytes.extend_from_slice(&buf[..n]);
                        if bytes.len() as u64 > VIEW_MAX {
                            anyhow::bail!("{} is too large to view", entry.name);
                        }
                    }
                    anyhow::Result::<Vec<u8>>::Ok(bytes)
                })?
            }
        };

        if bytes.iter().take(8192).any(|&b| b == 0) {
            anyhow::bail!("{} looks like a binary file", entry.name);
        }

        let text = String::from_utf8_lossy(&bytes);
        let lines = text
            .lines()
            .map(|line| line.replace('\t', "    "))
            .collect();
        Ok((entry.name, lines))
    }

    fn view_selected(&mut self, term: &mut TermWizTerminalRef) {
        match self.load_selected_file() {
            Ok((title, lines)) => {
                if let Err(err) = viewer_loop(term, &title, &lines) {
                    self.status = format!("Viewer error: {err:#}");
                }
            }
            Err(err) => {
                self.status = format!("{err:#}");
            }
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
        self.navigate_to(parent, true);
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

    /// Full path of the selected entry, in the backend's notation
    fn selected_path(&self) -> Option<(FmEntry, String)> {
        let entry = self.selected()?.clone();
        let path = match &self.backend {
            FileManagerBackend::Local => Path::new(&self.cwd)
                .join(&entry.name)
                .to_string_lossy()
                .to_string(),
            FileManagerBackend::Remote { .. } => remote_join(&self.cwd, &entry.name),
        };
        Some((entry, path))
    }

    /// Asks a yes/no question on the status line; Escape/n = no
    fn confirm(&mut self, term: &mut TermWizTerminalRef, question: &str) -> bool {
        let _ = self.render(term);
        let _ = self.render_status(term, &format!("{question} [y/N]"));
        while let Ok(Some(event)) = term.poll_input(None) {
            match event {
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('y') | KeyCode::Char('Y'),
                    ..
                }) => return true,
                InputEvent::Key(KeyEvent { .. }) => return false,
                _ => {}
            }
        }
        false
    }

    /// Deletes the selected file (or empty directory) after confirmation
    fn delete_selected(&mut self, term: &mut TermWizTerminalRef) {
        let Some((entry, path)) = self.selected_path() else {
            self.status = "Nothing selected".to_string();
            return;
        };
        let what = if entry.is_dir { "directory" } else { "file" };
        if !self.confirm(term, &format!("Delete {what} '{}'?", entry.name)) {
            self.status = "Delete cancelled".to_string();
            return;
        }
        let result: anyhow::Result<()> = match &self.backend {
            FileManagerBackend::Local => {
                if entry.is_dir {
                    std::fs::remove_dir(&path).map_err(|e| anyhow::anyhow!("{e}"))
                } else {
                    std::fs::remove_file(&path).map_err(|e| anyhow::anyhow!("{e}"))
                }
            }
            FileManagerBackend::Remote { sftp, .. } => {
                let sftp = sftp.clone();
                smol::block_on(async {
                    if entry.is_dir {
                        sftp.remove_dir(path.as_str()).await?;
                    } else {
                        sftp.remove_file(path.as_str()).await?;
                    }
                    anyhow::Ok(())
                })
            }
        };
        match result {
            Ok(()) => {
                self.status = format!("Deleted {}", entry.name);
                if let Err(err) = self.reload() {
                    self.status = format!("Error: {err:#}");
                }
            }
            Err(err) => {
                self.status = if entry.is_dir {
                    format!(
                        "Could not delete '{}' (only empty directories can be deleted): {err:#}",
                        entry.name
                    )
                } else {
                    format!("Could not delete '{}': {err:#}", entry.name)
                };
            }
        }
    }

    /// Renames the selected entry within the current directory
    fn rename_selected(&mut self, term: &mut TermWizTerminalRef) {
        let Some((entry, src)) = self.selected_path() else {
            self.status = "Nothing selected".to_string();
            return;
        };
        let new_name = {
            let _ = term.render(&[
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::ClearScreen(ColorAttribute::Default),
                Change::Text(format!("Rename '{}' (Escape to cancel)\r\n", entry.name)),
            ]);
            let mut host = PathPromptHost {
                history: BasicHistory::default(),
            };
            let mut editor = LineEditor::new(term);
            editor.set_prompt("New name: ");
            match editor.read_line_with_optional_initial_value(&mut host, Some(&entry.name)) {
                Ok(Some(line)) => line.trim().to_string(),
                _ => String::new(),
            }
        };
        if new_name.is_empty() || new_name == entry.name {
            self.status = "Rename cancelled".to_string();
            return;
        }
        if new_name.contains('/') || new_name.contains('\\') {
            self.status = "The new name must not contain path separators".to_string();
            return;
        }
        let dst = match &self.backend {
            FileManagerBackend::Local => Path::new(&self.cwd)
                .join(&new_name)
                .to_string_lossy()
                .to_string(),
            FileManagerBackend::Remote { .. } => remote_join(&self.cwd, &new_name),
        };
        if self.entries.iter().any(|e| e.name == new_name) {
            self.status = format!("'{new_name}' already exists");
            return;
        }
        let result: anyhow::Result<()> = match &self.backend {
            FileManagerBackend::Local => {
                std::fs::rename(&src, &dst).map_err(|e| anyhow::anyhow!("{e}"))
            }
            FileManagerBackend::Remote { sftp, .. } => {
                let sftp = sftp.clone();
                smol::block_on(async {
                    sftp.rename(
                        src.as_str(),
                        dst.as_str(),
                        wezterm_ssh::RenameOptions::default(),
                    )
                    .await?;
                    anyhow::Ok(())
                })
            }
        };
        match result {
            Ok(()) => {
                self.status = format!("Renamed to {new_name}");
                if let Err(err) = self.reload() {
                    self.status = format!("Error: {err:#}");
                }
                if let Some(idx) = self.entries.iter().position(|e| e.name == new_name) {
                    self.active_idx = idx;
                }
            }
            Err(err) => {
                self.status = format!("Could not rename '{}': {err:#}", entry.name);
            }
        }
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

    /// Opens the action menu for the selected entry near cell (x, y)
    fn open_menu(&mut self, term: &mut TermWizTerminalRef, x: usize, y: usize) {
        let mut items = vec![];
        match self.selected() {
            Some(e) if e.is_dir => items.push(FmAction::Open),
            Some(_) => {
                items.push(FmAction::View);
                if self.is_remote() {
                    items.push(FmAction::Download);
                }
            }
            None => {}
        }
        if self.is_remote() {
            items.push(FmAction::Upload);
        }
        if self.selected().is_some() {
            items.push(FmAction::Rename);
            items.push(FmAction::Delete);
        }
        items.push(FmAction::Parent);
        if !self.back_stack.is_empty() {
            items.push(FmAction::Back);
        }
        if !self.fwd_stack.is_empty() {
            items.push(FmAction::Forward);
        }
        items.push(FmAction::Refresh);
        items.push(FmAction::Quit);

        let mut menu = FmMenu {
            items,
            selected: 0,
            x,
            y,
        };
        // keep the box inside the pane
        if let Ok(size) = term.get_screen_size() {
            let w = menu.width();
            let h = menu.height();
            if menu.x + w > size.cols {
                menu.x = size.cols.saturating_sub(w);
            }
            if menu.y + h > size.rows {
                menu.y = size.rows.saturating_sub(h);
            }
        }
        self.menu = Some(menu);
    }

    fn run_menu_action(&mut self, term: &mut TermWizTerminalRef, action: FmAction) -> bool {
        self.status.clear();
        match action {
            FmAction::Open | FmAction::View => self.enter_selected(term),
            FmAction::Parent => self.go_parent(),
            FmAction::Back => self.go_back(),
            FmAction::Forward => self.go_forward(),
            FmAction::Refresh => {
                if let Err(err) = self.reload() {
                    self.status = format!("Error: {err:#}");
                }
            }
            FmAction::Download => self.download_selected(term),
            FmAction::Upload => self.upload(term),
            FmAction::Rename => self.rename_selected(term),
            FmAction::Delete => self.delete_selected(term),
            FmAction::Quit => return true,
        }
        false
    }

    /// Handles input while the menu is open. Returns true if the file
    /// manager should exit.
    fn handle_menu_input(&mut self, term: &mut TermWizTerminalRef, event: InputEvent) -> bool {
        let Some(menu) = self.menu.as_mut() else {
            return false;
        };
        let n = menu.items.len();
        let mut activate: Option<FmAction> = None;
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape | KeyCode::Char('q') | KeyCode::Char('m'),
                ..
            }) => {
                self.menu = None;
                return false;
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::UpArrow | KeyCode::Char('k'),
                ..
            }) => {
                menu.selected = (menu.selected + n - 1) % n;
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::DownArrow | KeyCode::Char('j'),
                ..
            }) => {
                menu.selected = (menu.selected + 1) % n;
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Enter,
                ..
            }) => {
                activate = Some(menu.items[menu.selected]);
            }
            InputEvent::Mouse(MouseEvent {
                x,
                y,
                mouse_buttons,
                ..
            }) => {
                let pressed = mouse_buttons.contains(MouseButtons::LEFT)
                    && !self.prev_mouse_buttons.contains(MouseButtons::LEFT);
                let any_new = !mouse_buttons.is_empty() && self.prev_mouse_buttons.is_empty();
                self.prev_mouse_buttons = mouse_buttons;
                match menu.hit(x as usize, y as usize) {
                    Some(idx) => {
                        menu.selected = idx;
                        if pressed {
                            activate = Some(menu.items[idx]);
                        }
                    }
                    None if any_new => {
                        // click outside dismisses
                        self.menu = None;
                        return false;
                    }
                    None => {}
                }
            }
            _ => {}
        }
        if let Some(action) = activate {
            self.menu = None;
            return self.run_menu_action(term, action);
        }
        false
    }

    fn render_menu(&self, term: &mut TermWizTerminalRef) -> termwiz::Result<()> {
        let Some(menu) = self.menu.as_ref() else {
            return Ok(());
        };
        let w = menu.width();
        let inner = w - 2;
        let mut changes = vec![Change::AllAttributes(CellAttributes::default())];
        let at = |y: usize| Change::CursorPosition {
            x: Position::Absolute(menu.x),
            y: Position::Absolute(y),
        };
        changes.push(at(menu.y));
        changes.push(Change::Text(format!("┌{}┐", "─".repeat(inner))));
        for (idx, item) in menu.items.iter().enumerate() {
            let label = format!(" {:<width$} ", item.label(), width = inner - 2);
            changes.push(at(menu.y + 1 + idx));
            changes.push(Change::Text("│".to_string()));
            if idx == menu.selected {
                changes.push(AttributeChange::Reverse(true).into());
            }
            changes.push(Change::Text(label));
            if idx == menu.selected {
                changes.push(AttributeChange::Reverse(false).into());
            }
            changes.push(Change::Text("│".to_string()));
        }
        changes.push(at(menu.y + menu.height() - 1));
        changes.push(Change::Text(format!("└{}┘", "─".repeat(inner))));
        term.render(&changes)
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
            "Enter: open  BS: up  RClick: back  m: menu  d/u: down/upload  F2/Del: rename/delete  q"
        } else {
            "Enter: open  BS: up  RClick: back  MClick: fwd  m: menu  F2/Del: rename/delete  q"
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
        self.render_menu(term)?;
        Ok(())
    }

    fn run_loop(&mut self, term: &mut TermWizTerminalRef) -> anyhow::Result<()> {
        self.render(term)?;
        while let Ok(Some(event)) = term.poll_input(None) {
            if self.menu.is_some() {
                if self.handle_menu_input(term, event) {
                    break;
                }
                self.render(term)?;
                continue;
            }
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
                    key: KeyCode::Function(2),
                    ..
                }) => {
                    self.rename_selected(term);
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Delete,
                    ..
                }) => {
                    self.delete_selected(term);
                }
                InputEvent::Key(KeyEvent {
                    key: KeyCode::Char('m'),
                    modifiers: Modifiers::NONE,
                }) => {
                    let y = HEADER_ROWS + self.active_idx.saturating_sub(self.top_row);
                    self.open_menu(term, 2, y);
                }
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
                    self.enter_selected(term);
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
                    x,
                    y,
                    mouse_buttons,
                    modifiers,
                }) => {
                    // Both the press and the release of a click carry the
                    // button, so act only on the press edge.
                    let newly = |b: MouseButtons| {
                        mouse_buttons.contains(b.clone()) && !self.prev_mouse_buttons.contains(b)
                    };
                    let left_edge = newly(MouseButtons::LEFT);
                    let right_edge = newly(MouseButtons::RIGHT);
                    let middle_edge = newly(MouseButtons::MIDDLE);
                    self.prev_mouse_buttons = mouse_buttons;

                    if left_edge {
                        let row = y as usize;
                        if row >= HEADER_ROWS {
                            let idx = self.top_row + (row - HEADER_ROWS);
                            if idx < self.entries.len() {
                                if idx == self.active_idx {
                                    // clicking the already-selected entry opens it
                                    self.status.clear();
                                    self.enter_selected(term);
                                } else {
                                    self.active_idx = idx;
                                }
                            }
                        }
                    } else if right_edge && modifiers.intersects(Modifiers::SHIFT | Modifiers::CTRL)
                    {
                        // modified right click: action menu for the row
                        let row = y as usize;
                        if row >= HEADER_ROWS {
                            let idx = self.top_row + (row - HEADER_ROWS);
                            if idx < self.entries.len() {
                                self.active_idx = idx;
                            }
                        }
                        self.open_menu(term, x as usize, y as usize);
                    } else if right_edge {
                        // browser-style back
                        self.status.clear();
                        self.go_back();
                    } else if middle_edge {
                        // browser-style forward
                        self.status.clear();
                        self.go_forward();
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

use mux::termwiztermtab::TermWizTerminal as TermWizTerminalRef;

/// A simple vi-style read-only file viewer:
/// j/k or arrows scroll, CTRL-d/u half page, PageUp/PageDown/space full
/// page, g/G jump to start/end, / searches, n/N next/previous match,
/// q or Escape returns to the listing.
fn viewer_loop(
    term: &mut TermWizTerminalRef,
    title: &str,
    lines: &[String],
) -> termwiz::Result<()> {
    let mut top: usize = 0;
    let mut pattern = String::new();

    let render =
        |term: &mut TermWizTerminalRef, top: usize, pattern: &str| -> termwiz::Result<()> {
            let size = term.get_screen_size()?;
            let content_rows = size.rows.saturating_sub(1);
            let max_width = size.cols.saturating_sub(1);
            let mut changes = vec![
                Change::ClearScreen(ColorAttribute::Default),
                Change::CursorPosition {
                    x: Position::Absolute(0),
                    y: Position::Absolute(0),
                },
                Change::AllAttributes(CellAttributes::default()),
            ];
            for line in lines.iter().skip(top).take(content_rows) {
                changes.push(Change::Text(truncate_right(line, max_width)));
                changes.push(Change::Text("\r\n".to_string()));
            }
            let percent = if lines.is_empty() {
                100
            } else {
                ((top + content_rows).min(lines.len())) * 100 / lines.len()
            };
            let mut status = format!(
                " {} - {}/{} ({}%)  j/k scroll  / search  q close ",
                title,
                (top + 1).min(lines.len().max(1)),
                lines.len(),
                percent
            );
            if !pattern.is_empty() {
                status.push_str(&format!(" /{pattern}"));
            }
            changes.push(Change::CursorPosition {
                x: Position::Absolute(0),
                y: Position::Absolute(size.rows.saturating_sub(1)),
            });
            changes.push(AttributeChange::Reverse(true).into());
            changes.push(Change::Text(truncate_right(&status, max_width)));
            changes.push(AttributeChange::Reverse(false).into());
            term.render(&changes)
        };

    let find = |from: usize, pattern: &str, forward: bool| -> Option<usize> {
        if pattern.is_empty() {
            return None;
        }
        if forward {
            (from..lines.len()).find(|&i| lines[i].contains(pattern))
        } else {
            (0..from).rev().find(|&i| lines[i].contains(pattern))
        }
    };

    render(term, top, &pattern)?;
    while let Ok(Some(event)) = term.poll_input(None) {
        let size = term.get_screen_size()?;
        let content_rows = size.rows.saturating_sub(1).max(1);
        let max_top = lines.len().saturating_sub(content_rows);
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
                key: KeyCode::Char('j'),
                modifiers: Modifiers::NONE,
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::DownArrow,
                ..
            }) => top = (top + 1).min(max_top),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('k'),
                modifiers: Modifiers::NONE,
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::UpArrow,
                ..
            }) => top = top.saturating_sub(1),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('D'),
                modifiers: Modifiers::CTRL,
            }) => top = (top + content_rows / 2).min(max_top),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('U'),
                modifiers: Modifiers::CTRL,
            }) => top = top.saturating_sub(content_rows / 2),
            InputEvent::Key(KeyEvent {
                key: KeyCode::PageDown,
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::Char(' '),
                modifiers: Modifiers::NONE,
            }) => top = (top + content_rows).min(max_top),
            InputEvent::Key(KeyEvent {
                key: KeyCode::PageUp,
                ..
            }) => top = top.saturating_sub(content_rows),
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('g'),
                modifiers: Modifiers::NONE,
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::Home, ..
            }) => top = 0,
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('G'),
                ..
            })
            | InputEvent::Key(KeyEvent {
                key: KeyCode::End, ..
            }) => top = max_top,
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('/'),
                modifiers: Modifiers::NONE,
            }) => {
                term.render(&[
                    Change::CursorPosition {
                        x: Position::Absolute(0),
                        y: Position::Absolute(size.rows.saturating_sub(1)),
                    },
                    Change::ClearToEndOfLine(ColorAttribute::Default),
                ])?;
                let mut host = PathPromptHost {
                    history: BasicHistory::default(),
                };
                let mut editor = LineEditor::new(term);
                editor.set_prompt("/");
                if let Ok(Some(line)) = editor.read_line(&mut host) {
                    if !line.is_empty() {
                        pattern = line;
                        if let Some(hit) = find(top.saturating_add(1), &pattern, true)
                            .or_else(|| find(0, &pattern, true))
                        {
                            top = hit.min(max_top);
                        }
                    }
                }
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('n'),
                modifiers: Modifiers::NONE,
            }) => {
                if let Some(hit) = find(top.saturating_add(1), &pattern, true) {
                    top = hit.min(max_top);
                }
            }
            InputEvent::Key(KeyEvent {
                key: KeyCode::Char('N'),
                ..
            }) => {
                if let Some(hit) = find(top, &pattern, false) {
                    top = hit.min(max_top);
                }
            }
            InputEvent::Mouse(MouseEvent { mouse_buttons, .. })
                if mouse_buttons.contains(MouseButtons::VERT_WHEEL) =>
            {
                if mouse_buttons.contains(MouseButtons::WHEEL_POSITIVE) {
                    top = top.saturating_sub(3);
                } else {
                    top = (top + 3).min(max_top);
                }
            }
            InputEvent::Resized { .. } => {}
            _ => {}
        }
        render(term, top, &pattern)?;
    }
    Ok(())
}

pub fn file_manager(
    mut term: TermWizTerminalRef,
    backend: FileManagerBackend,
    start_dir: Option<String>,
) -> anyhow::Result<()> {
    // Enables mouse reporting so clicks and wheel scrolling reach us;
    // without this the pane never grabs the mouse and the GUI keeps
    // mouse events for itself.
    term.set_raw_mode()?;
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
