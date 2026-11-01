use eframe::egui::{Context, Key, KeyboardShortcut, Modifiers};
use std::collections::HashMap;
use std::sync::OnceLock;
use strum::{EnumIter, IntoEnumIterator};

/// All user-invokable actions, listed in the palette in declaration order. Single source of titles and keyboard shortcuts
/// for shortcut dispatch and the command palette.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, EnumIter)]
pub enum Command {
    CommandPalette,
    // App
    Pause,
    Export,
    FollowPlayback,
    ScrollPageLeft,
    ScrollPageRight,
    // Stave: selection and view
    ClearNoteSelection,
    ZoomToFit,
    // Stave: edits
    TempoUp,
    TempoDown,
    DeleteSelection,
    InsertTape,
    ShiftTailLater,
    ShiftTailEarlier,
    ShiftNotesLater,
    ShiftNotesEarlier,
    StretchNotesLonger,
    StretchNotesShorter,
    TransposeUp,
    TransposeDown,
    AccentUp,
    AccentDown,
    Undo,
    Redo,
    Copy,
    Paste,
    SetBookmark,
    ClearBookmark,
    // Stave: cursor navigation
    PrevBookmark,
    NextBookmark,
    PrevEvent,
    NextEvent,
    CursorStart,
    CursorEnd,
}

pub struct CommandDescription {
    pub title: &'static str,
    /// Key combinations that trigger the command, primary first.
    pub shortcuts: Vec<KeyboardShortcut>,
    /// Shortcut to show next to the title.
    pub display_shortcut: Option<KeyboardShortcut>,
}

fn plain(key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(Modifiers::NONE, key)
}

fn with(modifiers: Modifiers, key: Key) -> KeyboardShortcut {
    KeyboardShortcut::new(modifiers, key)
}

fn desc<const N: usize>(
    title: &'static str,
    shortcuts: [KeyboardShortcut; N],
) -> CommandDescription {
    CommandDescription {
        title,
        display_shortcut: shortcuts.first().copied(),
        shortcuts: shortcuts.into(),
    }
}

/// Copy and Paste are delivered by egui as events, not keys, so they have no dispatch shortcuts.
fn event_desc(title: &'static str, display_shortcut: KeyboardShortcut) -> CommandDescription {
    CommandDescription {
        title,
        shortcuts: vec![],
        display_shortcut: Some(display_shortcut),
    }
}

fn build_description(command: Command) -> CommandDescription {
    let ctrl = Modifiers::CTRL;
    let ctrl_shift = Modifiers::CTRL | Modifiers::SHIFT;
    let alt_shift = Modifiers::ALT | Modifiers::SHIFT;
    match command {
        Command::CommandPalette => desc("Command palette", [with(ctrl_shift, Key::P)]),
        Command::Pause => desc("Play/pause", [plain(Key::Space)]),
        Command::Export => desc("Export to MIDI file", [with(ctrl, Key::S)]),
        Command::FollowPlayback => desc("Toggle follow playback", [plain(Key::F)]),
        Command::ScrollPageLeft => desc("Scroll page left", [plain(Key::PageUp)]),
        Command::ScrollPageRight => desc("Scroll page right", [plain(Key::PageDown)]),
        Command::ClearNoteSelection => desc("Clear note selection", [plain(Key::Q)]),
        Command::ZoomToFit => desc("Zoom to fit", [with(Modifiers::ALT, Key::A)]),
        Command::TempoUp => desc("Tempo: slow down selection", [plain(Key::CloseBracket)]),
        Command::TempoDown => desc("Tempo: speed up selection", [plain(Key::OpenBracket)]),
        Command::DeleteSelection => desc("Delete selection", [plain(Key::Delete)]),
        Command::InsertTape => desc("Insert tape at time selection", [plain(Key::Insert)]),
        Command::ShiftTailLater => {
            desc("Shift track tail later", [with(ctrl_shift, Key::ArrowRight)])
        }
        Command::ShiftTailEarlier => {
            desc("Shift track tail earlier", [with(ctrl_shift, Key::ArrowLeft)])
        }
        Command::ShiftNotesLater => desc(
            "Move selected notes later",
            [with(alt_shift, Key::ArrowRight), with(Modifiers::SHIFT, Key::L)],
        ),
        Command::ShiftNotesEarlier => desc(
            "Move selected notes earlier",
            [with(alt_shift, Key::ArrowLeft), with(Modifiers::SHIFT, Key::H)],
        ),
        Command::StretchNotesLonger => desc("Make selected notes longer", [plain(Key::L)]),
        Command::StretchNotesShorter => desc("Make selected notes shorter", [plain(Key::H)]),
        Command::TransposeUp => desc("Transpose selected notes up", [plain(Key::U)]),
        Command::TransposeDown => desc("Transpose selected notes down", [plain(Key::J)]),
        Command::AccentUp => desc("Increase accent of selected notes", [plain(Key::I)]),
        Command::AccentDown => desc("Decrease accent of selected notes", [plain(Key::K)]),
        Command::Undo => desc("Undo", [with(ctrl, Key::Z)]),
        Command::Redo => desc("Redo", [with(ctrl, Key::Y), with(ctrl_shift, Key::Z)]),
        Command::Copy => event_desc("Copy time selection", with(Modifiers::COMMAND, Key::C)),
        Command::Paste => event_desc("Paste at cursor", with(Modifiers::COMMAND, Key::V)),
        Command::SetBookmark => desc("Set bookmark at cursor", [plain(Key::M)]),
        Command::ClearBookmark => desc("Clear bookmark at cursor", [plain(Key::N)]),
        Command::PrevBookmark => desc("Go to previous bookmark", [with(ctrl, Key::ArrowLeft)]),
        Command::NextBookmark => desc("Go to next bookmark", [with(ctrl, Key::ArrowRight)]),
        Command::PrevEvent => desc("Go to previous event", [with(Modifiers::ALT, Key::ArrowLeft)]),
        Command::NextEvent => desc("Go to next event", [with(Modifiers::ALT, Key::ArrowRight)]),
        Command::CursorStart => desc("Go to track start", [with(ctrl, Key::Home)]),
        Command::CursorEnd => desc("Go to track end", [with(ctrl, Key::End)]),
    }
}

impl Command {
    pub fn description(self) -> &'static CommandDescription {
        static DESCRIPTIONS: OnceLock<HashMap<Command, CommandDescription>> = OnceLock::new();
        &DESCRIPTIONS
            .get_or_init(|| Command::iter().map(|c| (c, build_description(c))).collect())[&self]
    }
}

/// All dispatchable shortcuts, most specific first.
fn shortcut_table() -> &'static [(KeyboardShortcut, Command)] {
    static TABLE: OnceLock<Vec<(KeyboardShortcut, Command)>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut table: Vec<_> = Command::iter()
            .flat_map(|cmd| cmd.description().shortcuts.iter().map(move |s| (*s, cmd)))
            .collect();
        // egui matches `Ctrl+Shift+Z` also as `Ctrl+Z`, and `Shift+L` as `L`, so test most specific first.
        table.sort_by_key(|(s, _)| -(s.modifiers.shift as i32 + s.modifiers.alt as i32));
        table
    })
}

/// Consume the shortcut pressed in this frame, if it matches any command.
/// Does nothing while a text field is being edited.
pub fn pressed(ctx: &Context) -> Option<Command> {
    if ctx.egui_wants_keyboard_input() {
        return None;
    }
    ctx.input_mut(|i| {
        shortcut_table()
            .iter()
            .find(|(shortcut, _)| i.consume_shortcut(shortcut))
            .map(|(_, cmd)| *cmd)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn shortcuts_do_not_clash() {
        let mut seen = HashSet::new();
        for cmd in Command::iter() {
            for s in &cmd.description().shortcuts {
                assert!(
                    seen.insert((s.logical_key, s.modifiers)),
                    "Shortcut of {:?} is used twice",
                    cmd
                );
            }
        }
    }
}
