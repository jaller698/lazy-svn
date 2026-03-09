use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Text},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::types::{ActiveWindow, FileTreeNode, KEYBINDINGS};

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
                name,
                depth,
                collapsed,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let icon = if *collapsed { "▶" } else { "▼" };
                ListItem::new(format!("{}{} {}/", indent, icon, name))
                    .style(Style::default().fg(Color::LightBlue))
            }
            FileTreeNode::File {
                status,
                name,
                depth,
                ..
            } => {
                let indent = "  ".repeat(*depth);
                let style = match status.as_str() {
                    "M" => Style::default().fg(Color::Blue),
                    "A" => Style::default().fg(Color::Green),
                    "D" => Style::default().fg(Color::Red),
                    _ => Style::default().fg(Color::White),
                };
                ListItem::new(format!("{}{} {}", indent, status, name)).style(style)
            }
        })
        .collect();

    let border_color = if app.active_window == ActiveWindow::ChangedFiles {
        Color::Yellow
    } else {
        Color::Gray
    };
    let list = List::new(items)
        .block(
            Block::default()
                .title(" 1: Files (j/k | Space: fold) ")
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

    let rev_items: Vec<ListItem> = app
        .revision_list
        .iter()
        .map(|rev| {
            let rev_num: Option<u64> = rev.revision.trim_start_matches('r').parse().ok();

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
        .collect();

    let rev_border_color = if app.active_window == ActiveWindow::Revisions {
        Color::Yellow
    } else {
        Color::Gray
    };

    let rev_list = List::new(rev_items)
        .block(
            Block::default()
                .title(" 3: Revisions (j/k: navigate | Enter: update) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(rev_border_color)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(60, 60, 60))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(">> ");

    f.render_stateful_widget(rev_list, left_chunks[2], &mut app.revision_list_state);

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
                .title(" 4: Diff View (j/k: scroll | {/}: hunk) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(diff_style)),
        )
        .scroll((app.diff_scroll, 0))
        .wrap(ratatui::widgets::Wrap { trim: false });

    f.render_widget(diff_paragraph, chunks[1]);

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

fn centered_rect(percent_x: u16, percent_y: u16, r: Rect) -> Rect {
    let popup_layout = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Fill(1),
            Constraint::Percentage(percent_y),
            Constraint::Fill(1),
        ])
        .split(r);

    Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Fill(1),
            Constraint::Percentage(percent_x),
            Constraint::Fill(1),
        ])
        .split(popup_layout[1])[1]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::app::App;
    use crate::types::KEYBINDINGS;
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
}
