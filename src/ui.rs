use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
};

use crate::app::App;
use crate::types::{ActiveWindow, CommitField, FileTreeNode, KEYBINDINGS};

/// Returns a centred rectangle of the given percentage size inside `r`.
fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Percentage((100 - percent_y) / 2),
            Constraint::Percentage(percent_y),
            Constraint::Percentage((100 - percent_y) / 2),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage((100 - percent_x) / 2),
            Constraint::Percentage(percent_x),
            Constraint::Percentage((100 - percent_x) / 2),
        ])
        .split(popup_layout[1])[1]
}

pub fn ui(f: &mut Frame, app: &mut App) {
    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(30), Constraint::Percentage(70)])
        .split(f.area());

    let left_chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
            Constraint::Ratio(1, 3),
        ])
        .split(chunks[0]);

    // Render File Tree
    let items: Vec<ListItem> = app
        .visible_items
        .iter()
        .map(|node| match node {
            FileTreeNode::Dir {
                path,
                name,
                depth,
                collapsed,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let icon = if *collapsed { "▶" } else { "▼" };
                let has_marked_child = app
                    .selected_files
                    .iter()
                    .any(|selected_path| selected_path.starts_with(path.as_str()));
                let style = if has_marked_child {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::LightBlue)
                };
                ListItem::new(format!("{}{} {}/", indent, icon, name))
                    .style(style)
            }
            FileTreeNode::File {
                status,
                name,
                depth,
                path,
            } => {
                let indent = "  ".repeat(*depth);
                let is_marked = app.selected_files.contains(path);
                let selected_marker = if is_marked {
                    "✓"
                } else {
                    " "
                };
                let style = if is_marked {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    match status.as_str() {
                        "M" => Style::default().fg(Color::Blue),
                        "A" => Style::default().fg(Color::Green),
                        "D" => Style::default().fg(Color::Red),
                        _ => Style::default().fg(Color::White),
                    }
                };
                ListItem::new(format!(
                    "{}[{}] {} {}",
                    indent, selected_marker, status, name
                ))
                .style(style)
            }
        })
        .collect();

    let border_color = if app.active_window == ActiveWindow::ChangedFiles
        || app.active_window == ActiveWindow::Commit
        || app.active_window == ActiveWindow::ConfirmDelete
        || app.active_window == ActiveWindow::ConfirmRevert
        || app.active_window == ActiveWindow::ConfirmIgnore
    {
        Color::Yellow
    } else {
        Color::Gray
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(" 1: Files (j/k | Space: select | Enter: fold | a: add | d: delete | r: revert | u: undo | c: commit | i: ignore) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(list, left_chunks[0], &mut app.file_list_state);

    // Branches List
    let branch_border_color = if app.active_window == ActiveWindow::Branches {
        Color::Yellow
    } else {
        Color::Gray
    };
    let branch_items: Vec<ListItem> = app
        .branch_list
        .iter()
        .map(|name| ListItem::new(format!(" {}", name)))
        .collect();
    let branch_list = List::new(branch_items)
        .block(
            Block::default()
                .title(" 2: Branches (j/k) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(branch_border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");
    f.render_stateful_widget(branch_list, left_chunks[1], &mut app.branch_list_state);

    // Revisions List
    let wc_rev_num: Option<u64> = app
        .working_copy_revision
        .as_deref()
        .and_then(|r| r.parse().ok());

    let rev_border_color = if app.active_window == ActiveWindow::Revisions
        || app.active_window == ActiveWindow::RevisionFiles
    {
        Color::Yellow
    } else {
        Color::Gray
    };

    let (rev_items, rev_title, use_revision_file_state): (Vec<ListItem>, &str, bool) =
        if app.active_window == ActiveWindow::RevisionFiles {
            (
                app.revision_files
                    .iter()
                    .map(|f| ListItem::new(format!("{} {}", f.status, f.path)))
                    .collect(),
                " 3: Revision Files (j/k: navigate | Enter: open diff | Esc: back) ",
                true,
            )
        } else {
            (
                app.revision_list
                    .iter()
                    .map(|rev| {
                        let rev_num: Option<u64> =
                            rev.revision.trim_start_matches('r').parse().ok();

                        let is_current = rev_num.is_some() && rev_num == wc_rev_num;
                        let is_remote = match (rev_num, wc_rev_num) {
                            (Some(r), Some(wc)) => r > wc,
                            _ => false,
                        };

                        let label = if rev.message.is_empty() {
                            format!("{} | {} | {}", rev.revision, rev.author, rev.date)
                        } else {
                            format!(
                                "{} | {} | {} | {}",
                                rev.revision, rev.author, rev.date, rev.message
                            )
                        };

                        if is_current {
                            ListItem::new(format!("{} [working copy]", label))
                                .style(Style::default().fg(Color::Cyan))
                        } else if is_remote {
                            ListItem::new(format!("{} [remote]", label))
                                .style(Style::default().fg(Color::Yellow))
                        } else {
                            ListItem::new(label)
                        }
                    })
                    .collect(),
                " 3: Revisions (j/k: navigate | Enter: update | f: files) ",
                false,
            )
        };

    let rev_list = List::new(rev_items)
        .block(
            Block::default()
                .title(rev_title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(rev_border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    if use_revision_file_state {
        f.render_stateful_widget(rev_list, left_chunks[2], &mut app.revision_file_list_state);
    } else {
        f.render_stateful_widget(rev_list, left_chunks[2], &mut app.revision_list_state);
    }

    // Diff View
    let diff_style = if app.active_window == ActiveWindow::Diff {
        Color::Yellow
    } else {
        Color::Gray
    };

    // Pass the pre-styled lines from our app state
    // But also ensure the lines are wrapped in a Paragraph with the correct border and scroll offset
    let diff_paragraph = Paragraph::new(app.current_diff.clone())
        .block(
            Block::default()
                .title(" 4: Diff View (j/k: scroll | {/}: hunk | o: open in Neovim) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(diff_style)),
        )
        .scroll((app.diff_scroll, 0))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(diff_paragraph, chunks[1]);

    // Commit popup overlay
    if app.active_window == ActiveWindow::Commit {
        let area = centered_rect(65, 55, f.area());
        f.render_widget(Clear, area);

        let selected_count = app.selected_files.len();
        let scope_hint = if selected_count == 0 {
            format!("Committing all {} changed file(s)", app.file_list.len())
        } else {
            format!("Committing {} selected file(s)", selected_count)
        };

        // Styles for active vs inactive field labels.
        let active_label = Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD);
        let inactive_label = Style::default().fg(Color::DarkGray);
        let value_style = Style::default().fg(Color::White);
        let hint_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Red);

        // Helper: cursor suffix shown only when a field is active.
        let cur = |field: &CommitField| -> &str {
            if *field == app.commit_active_field {
                "_"
            } else {
                ""
            }
        };

        // ── Message field ───────────────────────────────────────────────
        let msg_label_style = if app.commit_active_field == CommitField::Message {
            active_label
        } else {
            inactive_label
        };
        let is_msg_active = app.commit_active_field == CommitField::Message;
        let mut msg_lines: Vec<Line> = vec![Line::from(Span::styled("Message:", msg_label_style))];
        if app.commit_message.trim().is_empty() {
            msg_lines.push(Line::from(vec![
                Span::styled(if is_msg_active { "_" } else { "" }, value_style),
                Span::styled("  (required)", warn_style),
            ]));
        } else {
            // Determine cursor position (line, col) for rendering.
            let (cursor_line, cursor_col) = app.commit_cursor_line_col();
            let raw_lines: Vec<&str> = app.commit_message.split('\n').collect();
            for (i, raw) in raw_lines.iter().enumerate() {
                if is_msg_active && i == cursor_line {
                    // Split this line into before-cursor, cursor-char/marker, after-cursor.
                    let mut chars = raw.chars();
                    let before: String = chars.by_ref().take(cursor_col).collect();
                    // The remaining iterator starts at the cursor position.
                    let cursor_char = match chars.next() {
                        Some(c) => c.to_string(),
                        // Cursor is at end of line – show a block marker.
                        None => "_".to_string(),
                    };
                    let after_cursor: String = chars.collect();
                    msg_lines.push(Line::from(vec![
                        Span::styled(before, value_style),
                        Span::styled(
                            cursor_char,
                            Style::default().fg(Color::Black).bg(Color::White),
                        ),
                        Span::styled(after_cursor, value_style),
                    ]));
                } else {
                    msg_lines.push(Line::from(Span::styled(*raw, value_style)));
                }
            }
        }

        // ── Username field ──────────────────────────────────────────────
        let user_label_style = if app.commit_active_field == CommitField::Username {
            active_label
        } else {
            inactive_label
        };
        let user_line = Line::from(vec![
            Span::styled("Username (optional): ", user_label_style),
            Span::styled(app.commit_username.clone(), value_style),
            Span::styled(cur(&CommitField::Username), value_style),
        ]);

        // ── Password field ──────────────────────────────────────────────
        let pass_label_style = if app.commit_active_field == CommitField::Password {
            active_label
        } else {
            inactive_label
        };
        let masked = "*".repeat(app.commit_password.len());
        let pass_line = Line::from(vec![
            Span::styled("Password (optional): ", pass_label_style),
            Span::styled(masked, value_style),
            Span::styled(cur(&CommitField::Password), value_style),
        ]);

        // ── Assemble all lines ──────────────────────────────────────────
        let mut popup_lines: Vec<Line> = vec![
            Line::from(Span::styled(scope_hint, Style::default().fg(Color::Cyan))),
            Line::from(""),
        ];
        popup_lines.extend(msg_lines);
        popup_lines.push(Line::from(""));
        popup_lines.push(user_line);
        popup_lines.push(pass_line);
        popup_lines.push(Line::from(""));
        popup_lines.push(Line::from(Span::styled(
            "[Ctrl+Enter] commit  [Tab] next field  [Esc] cancel",
            hint_style,
        )));

        let popup = Paragraph::new(popup_lines)
            .block(
                Block::default()
                    .title(" Commit ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(popup, area);
    }
    // Confirm-delete popup overlay
    if app.active_window == ActiveWindow::ConfirmDelete {
        let area = centered_rect(55, 40, f.area());
        f.render_widget(Clear, area);

        let hint_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Red);
        let path_style = Style::default().fg(Color::White);

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                format!(
                    "About to delete {} item(s):",
                    app.delete_targets.len()
                ),
                warn_style,
            )),
            Line::from(""),
        ];
        for path in app.delete_targets.iter().take(8) {
            lines.push(Line::from(Span::styled(format!("  {}", path), path_style)));
        }
        if app.delete_targets.len() > 8 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", app.delete_targets.len() - 8),
                hint_style,
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[y] confirm  [n / Esc] cancel",
            hint_style,
        )));

        let popup = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Confirm Delete ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Red)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(popup, area);
    }
    // Confirm-revert popup overlay
    if app.active_window == ActiveWindow::ConfirmRevert {
        let area = centered_rect(55, 40, f.area());
        f.render_widget(Clear, area);

        let hint_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Yellow);
        let path_style = Style::default().fg(Color::White);

        let mut lines: Vec<Line> = vec![
            Line::from(Span::styled(
                format!(
                    "About to revert {} item(s):",
                    app.revert_targets.len()
                ),
                warn_style,
            )),
            Line::from(""),
        ];
        for path in app.revert_targets.iter().take(8) {
            lines.push(Line::from(Span::styled(format!("  {}", path), path_style)));
        }
        if app.revert_targets.len() > 8 {
            lines.push(Line::from(Span::styled(
                format!("  … and {} more", app.revert_targets.len() - 8),
                hint_style,
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "[y] confirm  [n / Esc] cancel",
            hint_style,
        )));

        let popup = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Confirm Revert ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(popup, area);
    }
    // Confirm-ignore popup overlay
    if app.active_window == ActiveWindow::ConfirmIgnore {
        let area = centered_rect(55, 25, f.area());
        f.render_widget(Clear, area);

        let hint_style = Style::default().fg(Color::DarkGray);
        let warn_style = Style::default().fg(Color::Yellow);
        let path_style = Style::default().fg(Color::White);

        let file_name = app
            .ignore_target
            .as_deref()
            .unwrap_or("<unknown>");

        let lines: Vec<Line> = vec![
            Line::from(Span::styled(
                "Add to ignore list?",
                warn_style,
            )),
            Line::from(""),
            Line::from(Span::styled(format!("  {}", file_name), path_style)),
            Line::from(""),
            Line::from(Span::styled(
                "[y] confirm  [n / Esc] cancel",
                hint_style,
            )),
        ];

        let popup = Paragraph::new(lines)
            .block(
                Block::default()
                    .title(" Confirm Ignore ")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Yellow)),
            )
            .wrap(Wrap { trim: false });

        f.render_widget(popup, area);
    }
    if app.active_window == ActiveWindow::Help {
        let area = centered_rect(60, 60, f.area());
        let lines: Vec<Line> = std::iter::once(Line::from(" Keybindings - press ? to close"))
            .chain(std::iter::once(Line::from("")))
            .chain(
                KEYBINDINGS
                    .iter()
                    .map(|kb| Line::from(format!("  {:6}  {}", kb.key, kb.description))),
            )
            .collect();
        let help_text = Text::from(lines);
        let popup = Paragraph::new(help_text).block(
            Block::default()
                .title(" Help ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow)),
        );
        f.render_widget(Clear, area);
        f.render_widget(popup, area);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::types::{FileTreeNode, KEYBINDINGS};
    use ratatui::{Terminal, backend::TestBackend};

    /// Render the UI with the help window open and collect every cell's symbol into
    /// a single string.  We then verify that every key listed in `KEYBINDINGS` appears
    /// somewhere in that rendered text.  Adding a new entry to `KEYBINDINGS` is
    /// sufficient to make the test cover it automatically.
    #[test]
    fn test_help_window_shows_all_keybindings() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::test_new();
        app.open_help();

        terminal.draw(|f| ui(f, &mut app)).unwrap();

        let buffer = terminal.backend().buffer().clone();
        // Join all cell symbols row-by-row so that multi-character keys like "Tab"
        // or "Enter" are preserved as contiguous substrings.
        let content: String = (0..buffer.area().height)
            .map(|y| {
                (0..buffer.area().width)
                    .map(|x| buffer[(x, y)].symbol().to_string())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n");

        for kb in KEYBINDINGS {
            assert!(
                content.contains(kb.key),
                "Help window is missing keybinding: '{}' (description: '{}')",
                kb.key,
                kb.description
            );
        }
    }

    fn find_fg_for_substring(
        buffer: &ratatui::buffer::Buffer,
        needle: &str,
    ) -> Option<ratatui::style::Color> {
        for y in 0..buffer.area().height {
            let row = (0..buffer.area().width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>();
            if let Some(idx) = row.find(needle) {
                return Some(buffer[((idx as u16), y)].fg);
            }
        }
        None
    }

    #[test]
    fn test_marked_files_and_parents_are_colored() {
        let backend = TestBackend::new(120, 40);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::test_new();

        app.visible_items = vec![
            FileTreeNode::Dir {
                path: "src/".to_string(),
                name: "src".to_string(),
                depth: 0,
                collapsed: false,
            },
            FileTreeNode::Dir {
                path: "src/lib/".to_string(),
                name: "lib".to_string(),
                depth: 1,
                collapsed: false,
            },
            FileTreeNode::File {
                status: "M".to_string(),
                path: "src/lib/main.rs".to_string(),
                name: "main.rs".to_string(),
                depth: 2,
            },
        ];
        app.selected_files.insert("src/lib/main.rs".to_string());

        terminal.draw(|f| ui(f, &mut app)).unwrap();
        let buffer = terminal.backend().buffer().clone();

        assert_eq!(find_fg_for_substring(&buffer, "src/"), Some(Color::Yellow));
        assert_eq!(find_fg_for_substring(&buffer, "lib/"), Some(Color::Yellow));
        assert_eq!(find_fg_for_substring(&buffer, "main.rs"), Some(Color::Yellow));
    }
}
