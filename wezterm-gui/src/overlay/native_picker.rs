//! Native file/folder pickers driven through OS-provided helpers, so the
//! file manager (which runs off the GUI thread) can ask the user to
//! choose a file without linking a toolkit or touching the main thread.
//!
//! macOS: `osascript` (`choose file` / `choose folder` / `choose file name`)
//! Linux: `zenity`, falling back to `kdialog`
//! Windows: PowerShell with the WinForms dialogs
//!
//! Each function returns `Ok(Some(path))` on selection, `Ok(None)` when the
//! user cancelled, and `Err` when no picker is available (callers then
//! fall back to a typed prompt).
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PickKind {
    OpenFile,
    OpenFolder,
    SaveFile,
}

pub struct PickRequest<'a> {
    pub kind: PickKind,
    pub title: &'a str,
    pub start_dir: Option<&'a Path>,
    /// default file name for `SaveFile`
    pub default_name: Option<&'a str>,
}

fn output_to_path(out: std::process::Output) -> anyhow::Result<Option<PathBuf>> {
    if !out.status.success() {
        // cancel typically exits non-zero with empty stdout
        return Ok(None);
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if s.is_empty() {
        Ok(None)
    } else {
        Ok(Some(PathBuf::from(s)))
    }
}

#[cfg(target_os = "macos")]
pub fn pick(req: &PickRequest) -> anyhow::Result<Option<PathBuf>> {
    fn esc(s: &str) -> String {
        s.replace('\\', "\\\\").replace('"', "\\\"")
    }
    let title = esc(req.title);
    let loc = req
        .start_dir
        .map(|d| {
            format!(
                " default location (POSIX file \"{}\")",
                esc(&d.to_string_lossy())
            )
        })
        .unwrap_or_default();
    let script = match req.kind {
        PickKind::OpenFile => {
            format!("POSIX path of (choose file with prompt \"{title}\"{loc})")
        }
        PickKind::OpenFolder => {
            format!("POSIX path of (choose folder with prompt \"{title}\"{loc})")
        }
        PickKind::SaveFile => {
            let name = req
                .default_name
                .map(|n| format!(" default name \"{}\"", esc(n)))
                .unwrap_or_default();
            format!("POSIX path of (choose file name with prompt \"{title}\"{name}{loc})")
        }
    };
    let out = Command::new("osascript").arg("-e").arg(script).output()?;
    output_to_path(out)
}

#[cfg(all(unix, not(target_os = "macos")))]
pub fn pick(req: &PickRequest) -> anyhow::Result<Option<PathBuf>> {
    fn have(cmd: &str) -> bool {
        Command::new(cmd)
            .arg("--version")
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false)
    }
    if have("zenity") {
        let mut cmd = Command::new("zenity");
        cmd.arg("--file-selection")
            .arg(format!("--title={}", req.title));
        match req.kind {
            PickKind::OpenFile => {}
            PickKind::OpenFolder => {
                cmd.arg("--directory");
            }
            PickKind::SaveFile => {
                cmd.arg("--save").arg("--confirm-overwrite");
            }
        }
        let mut filename = req.start_dir.map(|d| d.to_path_buf()).unwrap_or_default();
        if let (PickKind::SaveFile, Some(name)) = (req.kind, req.default_name) {
            filename.push(name);
        } else if !filename.as_os_str().is_empty() {
            // trailing separator makes zenity treat it as a directory
            filename.push("");
        }
        if !filename.as_os_str().is_empty() {
            cmd.arg(format!("--filename={}", filename.display()));
        }
        return output_to_path(cmd.output()?);
    }
    if have("kdialog") {
        let mut cmd = Command::new("kdialog");
        cmd.arg("--title").arg(req.title);
        let start = req
            .start_dir
            .map(|d| d.to_string_lossy().to_string())
            .unwrap_or_else(|| ".".to_string());
        match req.kind {
            PickKind::OpenFile => {
                cmd.arg("--getopenfilename").arg(&start);
            }
            PickKind::OpenFolder => {
                cmd.arg("--getexistingdirectory").arg(&start);
            }
            PickKind::SaveFile => {
                let mut p = PathBuf::from(&start);
                if let Some(name) = req.default_name {
                    p.push(name);
                }
                cmd.arg("--getsavefilename").arg(p);
            }
        }
        return output_to_path(cmd.output()?);
    }
    anyhow::bail!("no native file dialog available (install zenity or kdialog)")
}

#[cfg(windows)]
pub fn pick(req: &PickRequest) -> anyhow::Result<Option<PathBuf>> {
    fn esc(s: &str) -> String {
        s.replace('\'', "''")
    }
    let title = esc(req.title);
    let dir = req
        .start_dir
        .map(|d| esc(&d.to_string_lossy()))
        .unwrap_or_default();
    let script = match req.kind {
        PickKind::OpenFile => format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.OpenFileDialog; \
             $d.Title = '{title}'; $d.InitialDirectory = '{dir}'; \
             if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.FileName }}"
        ),
        PickKind::OpenFolder => format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $d = New-Object System.Windows.Forms.FolderBrowserDialog; \
             $d.Description = '{title}'; $d.SelectedPath = '{dir}'; \
             if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.SelectedPath }}"
        ),
        PickKind::SaveFile => {
            let name = esc(req.default_name.unwrap_or(""));
            format!(
                "Add-Type -AssemblyName System.Windows.Forms; \
                 $d = New-Object System.Windows.Forms.SaveFileDialog; \
                 $d.Title = '{title}'; $d.InitialDirectory = '{dir}'; $d.FileName = '{name}'; \
                 if ($d.ShowDialog() -eq 'OK') {{ Write-Output $d.FileName }}"
            )
        }
    };
    let out = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-STA", "-Command", &script])
        .output()?;
    output_to_path(out)
}
