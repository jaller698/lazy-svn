use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::types::{ActiveWindow, FileTreeNode};

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
    let diff_paragraph = Paragraph::new(app.current_diff.clone())
        .block(
            Block::default()
                .title(" 4: Diff View (j/k: scroll | {/}: hunk) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(diff_style)),
        )
        .scroll((app.diff_scroll, 0));

    f.render_widget(diff_paragraph, chunks[1]);
}
