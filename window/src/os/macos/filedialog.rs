//! Native open/save panels.
//!
//! These run inside our own process so that the dialog belongs to the
//! application (and is placed over its window) rather than appearing as a
//! window of some helper process such as `osascript`.
#![allow(unexpected_cfgs)] // <https://github.com/SSheldon/rust-objc/issues/125>
use crate::macos::{nsstring, nsstring_to_str};
use cocoa::appkit::NSApp;
use cocoa::base::{id, NO, YES};
use cocoa::foundation::NSInteger;
use objc::*;
use std::path::PathBuf;

/// `NSModalResponseOK`
const MODAL_RESPONSE_OK: NSInteger = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileDialogKind {
    /// choose an existing file
    OpenFile,
    /// choose an existing directory
    OpenFolder,
    /// choose a destination path, prompting to replace an existing file
    SaveFile,
}

#[derive(Debug, Clone, Default)]
pub struct FileDialogParams {
    /// explanatory text shown above the file list
    pub title: String,
    /// directory the panel should start in
    pub start_dir: Option<PathBuf>,
    /// pre-filled name, used by `SaveFile`
    pub default_name: Option<String>,
}

/// Shows a native panel and returns the path the user chose, or `None` if
/// they cancelled.
///
/// This must be called on the main thread: it runs a nested modal event
/// loop, so the caller's thread is blocked until the panel is dismissed.
pub fn run_file_dialog(kind: FileDialogKind, params: &FileDialogParams) -> Option<PathBuf> {
    unsafe {
        let panel: id = match kind {
            FileDialogKind::SaveFile => msg_send![class!(NSSavePanel), savePanel],
            FileDialogKind::OpenFile | FileDialogKind::OpenFolder => {
                msg_send![class!(NSOpenPanel), openPanel]
            }
        };
        if panel.is_null() {
            return None;
        }

        let _: () = msg_send![panel, setMessage: *nsstring(&params.title)];

        match kind {
            FileDialogKind::OpenFile => {
                let _: () = msg_send![panel, setCanChooseFiles: YES];
                let _: () = msg_send![panel, setCanChooseDirectories: NO];
                let _: () = msg_send![panel, setAllowsMultipleSelection: NO];
            }
            FileDialogKind::OpenFolder => {
                let _: () = msg_send![panel, setCanChooseFiles: NO];
                let _: () = msg_send![panel, setCanChooseDirectories: YES];
                let _: () = msg_send![panel, setAllowsMultipleSelection: NO];
            }
            FileDialogKind::SaveFile => {
                if let Some(name) = params.default_name.as_deref() {
                    let _: () = msg_send![panel, setNameFieldStringValue: *nsstring(name)];
                }
            }
        }

        if let Some(dir) = params.start_dir.as_ref() {
            let path = nsstring(&dir.to_string_lossy());
            let url: id = msg_send![class!(NSURL), fileURLWithPath: *path isDirectory: YES];
            if !url.is_null() {
                let _: () = msg_send![panel, setDirectoryURL: url];
            }
        }

        // Make sure we're the active application, otherwise the panel can
        // open behind whatever the user was looking at.
        let app = NSApp();
        if !app.is_null() {
            let _: () = msg_send![app, activateIgnoringOtherApps: YES];
        }

        let response: NSInteger = msg_send![panel, runModal];
        if response != MODAL_RESPONSE_OK {
            return None;
        }

        let url: id = msg_send![panel, URL];
        if url.is_null() {
            return None;
        }
        let path: id = msg_send![url, path];
        if path.is_null() {
            return None;
        }
        Some(PathBuf::from(nsstring_to_str(path)))
    }
}
