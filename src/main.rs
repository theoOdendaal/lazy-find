use crossterm::event::{KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Span, Text};
use ratatui::widgets::{Block, Borders, List, ListDirection, ListState, Paragraph};
use std::borrow::Cow;

use std::io::{self};
use std::path::{Path, PathBuf};

mod fs_walk;
mod greedy_match;
mod persistence;

// TODO use more effecient data structures?
// TODO, currently spaces are ignored? It's not even pushed to query. Is this fine?
// TODO better manage list selection. As currently it remains constant when typing and then falls off when selected exceeds index.
// TODO update query asynchronous and add debouncing to filter logic. i.e. query should be updated independently of filtering logic, and only apply filtering logic after a delay.
// TODO implement a watcher using notify, in order to update fs data without having to rescan entire disk during runtime.
// TODO perhaps let walk_dir_par return an iter? as there is technically no need to collect it?
// TODO refactor code to be more modular?

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = "paths.bincode";

    //let start = Instant::now();
    //let file_population = fs_walk::walk_dir_par(Path::new("."));
    //println!("{:?}", start.elapsed());
    //persistence::save_paths(&file_population, file)?;

    let file_population = persistence::load_paths(file).await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();

    execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture,
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = run_app(&mut terminal, file_population).await;

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(result?)
}

async fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    file_population: Vec<PathBuf>,
) -> io::Result<()> {
    let mut file_query = String::new();
    // Initialze last_query as " ", otherwise tui won't render.
    let mut last_file_query = String::from(" ");
    let mut file_list_state = ListState::default().with_selected(Some(0));
    let mut filtered_files: Vec<Cow<str>> = Vec::new();

    // Pre-processing stored files.
    // Currently matching is only performed on the file name
    // and not the full path, while the full path is displayed
    // in the tui. The file name is stripped of spaces, and converted
    // to bytes for faster iterations compared to chars().
    // Space stripped file name is stored as bytes at pos 0 of the tuple,
    // while the full path is stored at pos 1.
    let cached_paths = greedy_match::prepare_paths_for_search(&file_population);

    // Create channel for keystrokes using tokio.
    // Used to decouple keystrokes from other logic.
    let (tx_query, mut rx_query) = tokio::sync::mpsc::channel(100);
    let tx_query_initial = tx_query.clone();
    // Runs async task in the background.
    tokio::spawn(async move {
        loop {
            if crossterm::event::poll(std::time::Duration::from_millis(10)).unwrap() {
                if let crossterm::event::Event::Key(key_event) = crossterm::event::read().unwrap() {
                    if key_event.kind == KeyEventKind::Press
                        && tx_query.send(key_event).await.is_err()
                    {
                        break;
                    }
                }
            }
        }
    });

    // Initial dummy even to start the event loop,
    // otherwise the app isn't rendered on startup.
    tx_query_initial
        .send(crossterm::event::KeyEvent {
            code: KeyCode::Null,
            modifiers: crossterm::event::KeyModifiers::NONE,
            kind: KeyEventKind::Press,
            state: crossterm::event::KeyEventState::NONE,
        })
        .await
        .unwrap();

    while let Some(event) = rx_query.recv().await {
        match event.code {
            KeyCode::Esc => break,
            KeyCode::Char(' ') => continue,
            KeyCode::Char(c) => file_query.push(c),
            KeyCode::Backspace => {
                file_query.pop();
            }
            KeyCode::Up => file_list_state.select_previous(),
            KeyCode::Down => file_list_state.select_next(),
            _ => {}
        }

        // Matching logic is only applied if query is updated. Due to the tui rendeing being
        // event drive, not having this condition would cause the matching logic to be be applied
        // for all keystrokes, even those that does not have an impact on the results presented.
        // ex. Using up and down arrows to navigate.
        if file_query != last_file_query {
            let query_as_bytes = greedy_match::prepare_fuzzy_target(&file_query);
            filtered_files = greedy_match::greedy_match_filter(query_as_bytes, &cached_paths);
            last_file_query = file_query.clone();
        }

        // TUI rendering
        // Render terminal
        terminal.draw(|f| {
            // Define layout.
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .margin(1)
                .constraints([Constraint::Min(1), Constraint::Length(3)])
                .split(f.area());

            // Define overarching style.
            let app_style = Style::default()
                .bg(Color::Rgb(31, 35, 53))
                .fg(Color::Rgb(220, 215, 186));

            // Define results widget.
            let file_results: List = List::new(
                filtered_files
                    .iter()
                    .take(25)
                    .map(|s| s.as_ref())
                    .collect::<Vec<&str>>(),
            )
            .style(app_style)
            .block(
                Block::default()
                    .title(format!("{}", filtered_files.len()))
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::new().italic().bold())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .direction(ListDirection::TopToBottom);

            // Define input widget.

            let input = Paragraph::new(Text::from(Span::raw(&file_query)))
                .style(app_style)
                .block(Block::default().title("lazy-find").borders(Borders::ALL));

            // Render widgets.
            f.render_stateful_widget(file_results, chunks[0], &mut file_list_state);
            f.render_widget(input, chunks[1]);
        })?;
        //}
    }
    Ok(())
}
