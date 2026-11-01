use crate::commands::{self, Command};
use eframe::egui::{self, Align, Align2, Context, Key, Layout, Modifiers, RichText};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;

/// Pop-up list of commands filtered by the text typed by the user.
#[derive(Default)]
pub struct CommandPalette {
    pub visible: bool,
    query: String,
    selected: usize,
    matcher: SkimMatcherV2,
}

/// Commands whose title matches the query, best match first.
/// With an empty query all commands are returned in registry order.
fn matching_commands(matcher: &SkimMatcherV2, query: &str) -> Vec<Command> {
    if query.is_empty() {
        return commands::ALL.to_vec();
    }
    let mut scored: Vec<(i64, Command)> = commands::ALL
        .iter()
        .filter_map(|cmd| {
            matcher
                .fuzzy_match(cmd.title(), query)
                .map(|score| (score, *cmd))
        })
        .collect();
    scored.sort_by_key(|(score, _)| -score); // Stable, so ties keep registry order.
    scored.into_iter().map(|(_, cmd)| cmd).collect()
}

impl CommandPalette {
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        self.query.clear();
        self.selected = 0;
    }

    /// Shows the palette when it is visible. Returns the command chosen by the user.
    #[must_use]
    pub fn show(&mut self, ctx: &Context) -> Option<Command> {
        if !self.visible {
            return None;
        }
        // Take navigation keys before the text field gets them.
        let (enter, up, down, escape) = ctx.input_mut(|i| {
            (
                i.consume_key(Modifiers::NONE, Key::Enter),
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowUp),
                i.count_and_consume_key(Modifiers::NONE, Key::ArrowDown),
                i.consume_key(Modifiers::NONE, Key::Escape),
            )
        });
        if escape {
            self.toggle();
            return None;
        }

        let mut chosen = None;
        let mut query_changed = false;
        let window = egui::Window::new("Command palette")
            .title_bar(false)
            .resizable(false)
            .anchor(Align2::CENTER_TOP, [0.0, 60.0])
            .fixed_size([480.0, 0.0])
            .show(ctx, |ui| {
                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("Type a command")
                        .desired_width(f32::INFINITY),
                );
                edit.request_focus();
                query_changed = edit.changed();
                if query_changed {
                    self.selected = 0;
                }

                let found = matching_commands(&self.matcher, &self.query);
                if found.is_empty() {
                    ui.weak("No matching commands");
                    return;
                }
                self.selected = (self.selected + down).saturating_sub(up).min(found.len() - 1);
                if enter {
                    chosen = Some(found[self.selected]);
                }
                ui.separator();
                egui::ScrollArea::vertical()
                    .max_height(320.0)
                    .show(ui, |ui| {
                        for (n, cmd) in found.iter().enumerate() {
                            let selected = n == self.selected;
                            let row = ui.scope(|ui| {
                                ui.horizontal(|ui| {
                                    ui.set_width(ui.available_width());
                                    let title = ui.selectable_label(selected, cmd.title());
                                    if title.clicked() {
                                        chosen = Some(*cmd);
                                    }
                                    if let Some(shortcut) = cmd.display_shortcut() {
                                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                            ui.label(RichText::new(ctx.format_shortcut(&shortcut)).weak());
                                        });
                                    }
                                });
                            });
                            if selected && (up > 0 || down > 0 || query_changed) {
                                row.response.scroll_to_me(None);
                            }
                        }
                    });
            });

        if chosen.is_some() {
            self.toggle();
        } else if let Some(window) = window
            && ctx.input(|i| i.pointer.any_pressed())
            && let Some(pos) = ctx.input(|i| i.pointer.interact_pos())
            && !window.response.rect.contains(pos)
        {
            self.toggle();
        }
        chosen
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_query_lists_everything_in_registry_order() {
        let found = matching_commands(&SkimMatcherV2::default(), "");
        assert_eq!(found, commands::ALL);
    }

    #[test]
    fn query_filters_by_title() {
        let found = matching_commands(&SkimMatcherV2::default(), "transp");
        assert_eq!(found.len(), 2);
        assert!(found.contains(&Command::TransposeUp));
        assert!(found.contains(&Command::TransposeDown));
        assert!(matching_commands(&SkimMatcherV2::default(), "zzzz").is_empty());
    }

    #[test]
    fn fuzzy_query_matches_non_adjacent_letters() {
        let found = matching_commands(&SkimMatcherV2::default(), "undo");
        assert_eq!(found.first(), Some(&Command::Undo));
    }
}
