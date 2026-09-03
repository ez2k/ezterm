//! A small overlay that prompts for a new workspace name and renames
//! the workspace in the mux.
use mux::termwiztermtab::TermWizTerminal;
use mux::Mux;
use termwiz::input::{InputEvent, KeyCode, KeyEvent};
use termwiz::lineedit::{Action, BasicHistory, History, LineEditor, LineEditorHost};
use termwiz::surface::Change;
use termwiz::terminal::Terminal;
use window::WindowOps;

struct RenameHost {
    history: BasicHistory,
}

impl LineEditorHost for RenameHost {
    fn history(&mut self) -> &mut dyn History {
        &mut self.history
    }

    fn resolve_action(
        &mut self,
        event: &InputEvent,
        _editor: &mut LineEditor<'_>,
    ) -> Option<Action> {
        match event {
            InputEvent::Key(KeyEvent {
                key: KeyCode::Escape,
                ..
            }) => Some(Action::Cancel),
            _ => None,
        }
    }
}

/// Shows a one-line prompt with the given heading and returns the
/// trimmed, non-empty new value if the user confirmed a change.
pub fn prompt_for_name(
    term: &mut TermWizTerminal,
    heading: &str,
    old_name: &str,
) -> anyhow::Result<Option<String>> {
    term.no_grab_mouse_in_raw_mode();
    term.render(&[Change::Text(format!("{heading} (Escape to cancel)\r\n"))])?;

    let mut host = RenameHost {
        history: BasicHistory::default(),
    };
    let mut editor = LineEditor::new(term);
    editor.set_prompt("New name: ");
    let line = editor.read_line_with_optional_initial_value(&mut host, Some(old_name))?;

    Ok(line.and_then(|new_name| {
        let new_name = new_name.trim().to_string();
        if new_name.is_empty() || new_name == old_name {
            None
        } else {
            Some(new_name)
        }
    }))
}

pub fn rename_workspace_prompt(
    mut term: TermWizTerminal,
    old_name: String,
    window: ::window::Window,
) -> anyhow::Result<()> {
    let heading = format!("Rename workspace '{old_name}'");
    if let Some(new_name) = prompt_for_name(&mut term, &heading, &old_name)? {
        promise::spawn::spawn_into_main_thread(async move {
            Mux::get().rename_workspace(&old_name, &new_name);
            window.invalidate();
        })
        .detach();
    }
    Ok(())
}

pub fn rename_tab_prompt(
    mut term: TermWizTerminal,
    tab_id: mux::tab::TabId,
    old_name: String,
    window: ::window::Window,
) -> anyhow::Result<()> {
    if let Some(new_name) = prompt_for_name(&mut term, "Rename tab", &old_name)? {
        promise::spawn::spawn_into_main_thread(async move {
            if let Some(tab) = Mux::get().get_tab(tab_id) {
                tab.set_title(&new_name);
            }
            window.invalidate();
        })
        .detach();
    }
    Ok(())
}
