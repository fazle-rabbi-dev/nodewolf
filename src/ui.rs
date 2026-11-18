use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Cell, Clear, Paragraph, Row, Table, TableState, Wrap};
use ratatui::Frame;

use crate::{age_label, clean_path, human_size, App, Item};

pub fn draw(f: &mut Frame, app: &mut App) {
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(3),
        Constraint::Length(2),
    ])
    .split(f.area());

    draw_header(f, app, chunks[0]);
    draw_table(f, app, chunks[1]);
    draw_footer(f, app, chunks[2]);

    if app.confirm {
        draw_confirm(f, app);
    }
}

fn draw_header(f: &mut Frame, app: &App, area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(format!(" nodewolf | {} ", clean_path(&app.root)));

    let line1 = Line::from(vec![
        Span::raw("found "),
        Span::styled(app.items.len().to_string(), Style::new().fg(Color::Yellow)),
        Span::raw("   total "),
        Span::styled(human_size(app.total_size()), Style::new().fg(Color::Yellow)),
        Span::raw("   selected "),
        Span::styled(
            format!(
                "{} ({})",
                app.selected_count(),
                human_size(app.selected_size())
            ),
            Style::new().fg(Color::Cyan),
        ),
        Span::raw("   sort "),
        Span::styled(app.sort.label(), Style::new().fg(Color::Magenta)),
        Span::raw(if app.only_orphan {
            "   [only no-package.json]"
        } else {
            ""
        }),
        Span::raw(if app.freed_total > 0 {
            format!("   freed {}", human_size(app.freed_total))
        } else {
            String::new()
        }),
    ]);

    let status = if app.scanning {
        let frames = ["|", "/", "-", "\\"];
        format!(
            "{} scanning... {} folders | {}",
            frames[app.spinner % 4],
            app.scan_dirs,
            truncate(&app.scan_current, area.width as usize / 2)
        )
    } else {
        app.status.clone()
    };
    let color = if app.scanning || app.deleting {
        Color::Yellow
    } else {
        Color::Gray
    };

    let p = Paragraph::new(vec![line1, Line::from(Span::styled(status, Style::new().fg(color)))])
        .block(block)
        .wrap(Wrap { trim: true });
    f.render_widget(p, area);
}

fn draw_table(f: &mut Frame, app: &mut App, area: Rect) {
    let header = Row::new(vec![
        Cell::from(""),
        Cell::from("     SIZE"),
        Cell::from("AGE"),
        Cell::from("PROJECT"),
        Cell::from("PATH"),
        Cell::from("PKG"),
    ])
    .style(Style::new().fg(Color::Gray).add_modifier(Modifier::BOLD));

    let rows: Vec<Row> = app
        .visible
        .iter()
        .map(|&i| row_for(&app.items[i], &app.root))
        .collect();

    let widths = [
        Constraint::Length(3),
        Constraint::Length(9),
        Constraint::Length(6),
        Constraint::Length(22),
        Constraint::Min(20),
        Constraint::Length(7),
    ];

    let mut state = TableState::default();
    if !app.visible.is_empty() {
        state.select(Some(app.cursor));
    }

    let table = Table::new(rows, widths)
        .header(header)
        .block(Block::default().borders(Borders::ALL))
        .row_highlight_style(Style::new().bg(Color::Indexed(236)))
        .highlight_symbol("> ");

    f.render_stateful_widget(table, area, &mut state);
}

fn row_for(item: &Item, root: &std::path::Path) -> Row<'static> {
    let check = if item.deleting {
        Cell::from(Span::styled(
            "...",
            Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD),
        ))
    } else if item.selected {
        Cell::from(Span::styled(
            "[x]",
            Style::new().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        ))
    } else {
        Cell::from(Span::styled("[ ]", Style::new().fg(Color::DarkGray)))
    };

    let project = item
        .path
        .parent()
        .and_then(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "(drive root)".into());

    let rel = item
        .path
        .strip_prefix(root)
        .unwrap_or(&item.path)
        .display()
        .to_string();

    let path_cell = match &item.error {
        Some(e) => Cell::from(format!("{}  ! {}", rel, e)).style(Style::new().fg(Color::Red)),
        None => Cell::from(rel).style(Style::new().fg(Color::DarkGray)),
    };

    let pkg = if item.has_pkg {
        Cell::from("pkg").style(Style::new().fg(Color::Green))
    } else {
        Cell::from("NO-PKG").style(
            Style::new()
                .fg(Color::Red)
                .add_modifier(Modifier::BOLD),
        )
    };

    let mut row = Row::new(vec![
        check,
        Cell::from(human_size(item.size)).style(Style::new().fg(Color::Yellow)),
        Cell::from(age_label(item.modified)),
        Cell::from(project),
        path_cell,
        pkg,
    ]);

    if item.selected {
        row = row.style(Style::new().add_modifier(Modifier::BOLD));
    }
    row
}

fn draw_footer(f: &mut Frame, app: &App, area: Rect) {
    let text = if app.filter_mode {
        Line::from(vec![
            Span::styled("filter: ", Style::new().fg(Color::Cyan)),
            Span::raw(&app.filter),
            Span::styled("_", Style::new().fg(Color::Cyan)),
            Span::styled("   (enter/esc to close)", Style::new().fg(Color::DarkGray)),
        ])
    } else {
        Line::from(vec![Span::styled(
            "j/k move  space select  a all  n none  d delete  o orphans-only  s sort  / filter  r rescan  q quit",
            Style::new().fg(Color::DarkGray),
        )])
    };

    let help = if app.deleting {
        "deleting..."
    } else {
        ""
    };

    let p = Paragraph::new(vec![text, Line::from(Span::styled(help, Style::new().fg(Color::Yellow)))]);
    f.render_widget(p, area);
}

fn draw_confirm(f: &mut Frame, app: &App) {
    let area = centered(f.area(), 62, 7);
    f.render_widget(Clear, area);

    let text = vec![
        Line::from(Span::styled(
            format!(
                "Delete {} folder(s)?",
                app.selected_count()
            ),
            Style::new().fg(Color::Red).add_modifier(Modifier::BOLD),
        )),
        Line::from(format!(
            "{} of disk space will be freed.",
            human_size(app.selected_size())
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled("y", Style::new().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" = delete    "),
            Span::styled("n/esc", Style::new().fg(Color::Gray)),
            Span::raw(" = cancel"),
        ]),
    ];

    let p = Paragraph::new(text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(" confirm ")
                .style(Style::new().bg(Color::Black)),
        )
        .alignment(Alignment::Center);
    f.render_widget(p, area);
}

fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + (area.width.saturating_sub(w)) / 2,
        y: area.y + (area.height.saturating_sub(h)) / 2,
        width: w,
        height: h,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if max == 0 {
        return String::new();
    }
    if s.chars().count() <= max {
        return s.to_string();
    }
    let keep = max.saturating_sub(1);
    let mut out: String = s.chars().skip(s.chars().count() - keep).collect();
    out.insert(0, '~');
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::App;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    fn mock_tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nodewolf-ui-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("orphan/node_modules/pkg")).unwrap();
        std::fs::write(root.join("orphan/node_modules/pkg/i.js"), vec![b'x'; 4096]).unwrap();
        std::fs::create_dir_all(root.join("live/node_modules/react")).unwrap();
        std::fs::write(root.join("live/package.json"), "{}").unwrap();
        std::fs::write(root.join("live/node_modules/react/i.js"), vec![b'x'; 1024]).unwrap();
        root
    }

    fn wait_scan(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while app.scanning && Instant::now() < deadline {
            app.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        app.pump();
        app.tick();
    }

    fn screen_text(app: &mut App, w: u16, h: u16) -> String {
        let mut terminal = Terminal::new(TestBackend::new(w, h)).unwrap();
        terminal.draw(|f| draw(f, app)).unwrap();
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(|c| c.symbol())
            .collect()
    }

    #[test]
    fn renders_rows_with_orphan_flag() {
        let root = mock_tree("render");
        let mut app = App::new(root.clone());
        wait_scan(&mut app);
        assert_eq!(app.items.len(), 2);

        let text = screen_text(&mut app, 120, 20);
        assert!(text.contains("nodewolf"), "header missing");
        assert!(text.contains("NO-PKG"), "orphan flag missing");
        assert!(text.contains("react") || text.contains("orphan"), "rows missing");
        assert!(text.contains("d delete"), "footer missing");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn confirm_dialog_renders() {
        let root = mock_tree("confirm");
        let mut app = App::new(root.clone());
        wait_scan(&mut app);
        for &i in app.visible.clone().iter() {
            app.items[i].selected = true;
        }
        app.confirm = true;

        let text = screen_text(&mut app, 120, 20);
        assert!(text.contains("Delete 2 folder(s)?"));

        std::fs::remove_dir_all(&root).ok();
    }
}

