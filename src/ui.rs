use ratatui::{
    Frame,
    layout::{Constraint, Direction, Layout},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, List, ListItem, Paragraph},
};

use crate::app::App;
use crate::types::ActiveWindow;

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

    // Render File List
    let items: Vec<ListItem> = app
        .file_list
        .iter()
        .map(|file| {
            let style = match file.status.as_str() {
                "M" => Style::default().fg(Color::Blue),
                "A" => Style::default().fg(Color::Green),
                "D" => Style::default().fg(Color::Red),
                _ => Style::default().fg(Color::White),
            };
            ListItem::new(format!(" {}  {}", file.status, file.path)).style(style)
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
                .title(" Files (j/k) ")
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

    // Other Windows (Placeholders)
    f.render_widget(
        Block::default().title(" Branches ").borders(Borders::ALL),
        left_chunks[1],
    );
    f.render_widget(
        Block::default().title(" Revisions ").borders(Borders::ALL),
        left_chunks[2],
    );

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
                .title(" Diff View (j/k: scroll | {/}: hunk) ")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(diff_style)),
        )
        .scroll((app.diff_scroll, 0));

    f.render_widget(diff_paragraph, chunks[1]);
}
