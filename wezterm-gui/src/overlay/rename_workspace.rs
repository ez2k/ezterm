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

pub fn rename_workspace_prompt(
    mut term: TermWizTerminal,
    old_name: String,
    window: ::window::Window,
) -> anyhow::Result<()> {
    term.no_grab_mouse_in_raw_mode();
    term.render(&[Change::Text(format!(
        "Rename workspace '{old_name}' (Escape to cancel)\r\n"
    ))])?;

    let mut host = RenameHost {
        history: BasicHistory::default(),
    };
    let mut editor = LineEditor::new(&mut term);
    editor.set_prompt("New name: ");
    let line = editor.read_line_with_optional_initial_value(&mut host, Some(&old_name))?;

    if let Some(new_name) = line {
        let new_name = new_name.trim().to_string();
        if !new_name.is_empty() && new_name != old_name {
            promise::spawn::spawn_into_main_thread(async move {
                Mux::get().rename_workspace(&old_name, &new_name);
                window.invalidate();
            })
            .detach();
        }
    }
    Ok(())
}
