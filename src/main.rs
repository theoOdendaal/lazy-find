use bincode::decode_from_std_read;
use crossterm::event::{KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Span, Text};
use ratatui::widgets::{Block, Borders, List, ListDirection, ListState, Paragraph};
use rayon::iter::{
    IntoParallelIterator, IntoParallelRefIterator, ParallelBridge, ParallelIterator,
};
use rayon::slice::ParallelSliceMut;
use std::borrow::Cow;
use std::collections::HashSet;
use std::fs::File;
use std::fs::{self, DirEntry};
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::Instant;

// TODO use more effecient data structures?
// TODO, currently spaces are ignored? It's not even pushed to query. Is this fine?
// TODO better manage list selection. As currently it remains constant when typing and then falls off when selected exceeds index.
// TODO update query asynchronous and add debouncing to filter logic. i.e. query should be updated independently of filtering logic, and only apply filtering logic after a delay.
// TODO implement a watcher using notify, in order to update fs data without having to rescan entire disk during runtime.
// TODO perhaps let walk_dir_par return an iter? as there is technically no need to collect it?
// TODO refactor code to be more modular?

/// Recursively traverse the directory tree from `dir` in parallel,
/// returning a list of file paths (excluding directories).
pub fn walk_dir_par(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(read_dir) => read_dir
            .par_bridge()
            .flat_map(|entry_result| match entry_result {
                Ok(entry) => collect_entry_paths(entry),
                Err(_) => vec![],
            })
            .collect(),
        Err(_) => vec![],
    }
}

/// Handle one directory entry: recurse if it's a dir, return path if it's a file.
fn collect_entry_paths(entry: DirEntry) -> Vec<PathBuf> {
    let path = entry.path();

    match entry.file_type() {
        Ok(file_type) if file_type.is_file() => vec![path],
        Ok(file_type) if file_type.is_dir() && !should_ignore(&entry) => walk_dir_par(&path),
        _ => vec![],
    }
}

fn load_paths(path: &str) -> Result<Vec<PathBuf>, Box<dyn std::error::Error>> {
    let start = Instant::now();
    let mut file = BufReader::new(File::open(path)?);
    let cfg = bincode::config::standard();
    let paths = decode_from_std_read(&mut file, cfg)?;
    println!("{:?}", start.elapsed());
    Ok(paths)
}

fn save_paths(paths: &Vec<PathBuf>, path: &str) -> Result<(), Box<dyn std::error::Error>> {
    let mut file = BufWriter::new(File::create(path)?);
    let cfg = bincode::config::standard();
    bincode::encode_into_std_write(paths, &mut file, cfg)?;
    Ok(())
}

fn should_ignore(entry: &DirEntry) -> bool {
    const EXCLUDED_DIRS: &[&str] = &["/$Recycle.Bin/", "/$SysReset", "/Windows"];
    let path = entry.path();

    EXCLUDED_DIRS.iter().any(|excluded| {
        let excluded_path = Path::new(excluded);
        path.starts_with(excluded_path)
    })
}

fn greedy_match_score(query_bytes: &[u8], target_bytes: &[u8]) -> (bool, i32) {
    let mut q_idx = 0;
    let mut score = 0;
    let mut last_match_idx = None;
    let mut first_match_idx = None;

    for (t_idx, &t_char) in target_bytes.iter().enumerate() {
        if q_idx == query_bytes.len() {
            break;
        }

        if t_char == query_bytes[q_idx] {
            score += 10;

            if let Some(last) = last_match_idx {
                let gap = t_idx - last;
                if gap <= 1 {
                    score += 5;
                }
            } else {
                first_match_idx = Some(t_idx);
            }

            last_match_idx = Some(t_idx);
            q_idx += 1;
        }
    }

    let is_match = q_idx == query_bytes.len();
    // TODO potentially allow 1 deviation?
    //let is_match = q_idx + 1 >= query_bytes.len();

    if is_match {
        if let Some(first_idx) = first_match_idx {
            score += 20 - first_idx.min(20);
        }
        (true, score as i32)
    } else {
        (false, 0)
    }
}

/// Applies greedy matching logic to sort pre-processed data.
/// Expect a string as bytes, second tuple value represents the
/// displayed value.
fn greedy_match_filter<'a>(
    target: Vec<u8>,
    pre_processed: &'a [(Vec<u8>, Cow<'a, str>)],
) -> Vec<Cow<'a, str>> {
    let mut matched: Vec<(i32, Cow<str>)> = pre_processed
        .par_iter()
        .filter_map(|(value, disp)| {
            let (is_match, score) = greedy_match_score(&target, value);
            if is_match {
                Some((score, disp.clone()))
            } else {
                None
            }
        })
        .collect();

    matched.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

    matched
        .into_par_iter()
        .map(|(_, path)| path)
        .collect::<Vec<Cow<str>>>()
}

fn prepare_fuzzy_target(target: &str) -> Vec<u8> {
    target
        .to_lowercase()
        .bytes()
        .filter(|b| *b != b' ')
        .collect()
}

// How to make this faster?
async fn unique_parent_dirs(paths: &[PathBuf]) -> Vec<String> {
    let mut unique_dirs = HashSet::new();

    for path in paths {
        if let Some(parent) = path.parent() {
            unique_dirs.insert(parent.to_path_buf());
        }
    }
    unique_dirs
        .into_par_iter()
        .map(|pb| pb.to_string_lossy().to_string())
        .collect()
}

#[derive(PartialEq, Eq)]
enum AppMode {
    FileSearch,
    DirSearch,
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    //let file = "paths.bincode";

    let start = Instant::now();
    let file_population = walk_dir_par(Path::new("/"));
    println!("{:?}", start.elapsed());
    //let file_population = tokio::task::spawn_blocking(|| walk_dir_par(Path::new("/"))).await?;
    //save_paths(&file_population, file)?;

    //let file_population =
    //    tokio::task::spawn_blocking(|| load_paths(file).unwrap_or_default()).await?;

    enable_raw_mode()?;
    let mut stdout = io::stdout();

    execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
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

    let mut current_app_mode = AppMode::FileSearch;

    let mut diretory_query = String::new();
    let mut last_directory_query = String::from(" ");
    let mut filtered_dirs: Vec<Cow<str>> = Vec::new();
    let mut dir_list_state = ListState::default().with_selected(Some(0));

    // Pre-processing stored files.
    // Currently matching is only performed on the file name
    // and not the full path, while the full path is displayed
    // in the tui. The file name is stripped of spaces, and converted
    // to bytes for faster iterations compared to chars().
    // Space stripped file name is stored as bytes at pos 0 of the tuple,
    // while the full path is stored at pos 1.
    let cached_paths: Vec<(Vec<u8>, Cow<str>)> = file_population
        .par_iter()
        .filter_map(|s| {
            let file_name: Vec<u8> = s
                .file_name()?
                .to_string_lossy()
                .to_lowercase()
                .bytes()
                .filter(|b| *b != b' ')
                .collect();
            let full_path = s.to_string_lossy();

            Some((file_name, Cow::Owned(full_path.into_owned())))
        })
        .collect();

    let raw_dirs = unique_parent_dirs(&file_population).await;
    let cached_dirs: Vec<(Vec<u8>, Cow<str>)> = raw_dirs
        .par_iter()
        .map(|s| {
            (
                s.to_lowercase().bytes().filter(|b| *b != b' ').collect(),
                Cow::Owned(s.clone()),
            )
        })
        .collect();

    //////////////////////////////////////// ASYNC
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
        match current_app_mode {
            AppMode::FileSearch => match event.code {
                KeyCode::Char(':') => current_app_mode = AppMode::DirSearch,
                KeyCode::Esc => break,
                KeyCode::Char(' ') => continue,
                KeyCode::Char(c) => file_query.push(c),
                KeyCode::Backspace => {
                    file_query.pop();
                }
                KeyCode::Up => file_list_state.select_previous(),
                KeyCode::Down => file_list_state.select_next(),
                _ => {}
            },
            AppMode::DirSearch => match event.code {
                KeyCode::Char(':') => current_app_mode = AppMode::FileSearch,
                KeyCode::Esc => break,
                KeyCode::Char(' ') => continue,
                KeyCode::Char(c) => diretory_query.push(c),
                KeyCode::Backspace => {
                    diretory_query.pop();
                }
                KeyCode::Up => dir_list_state.select_previous(),
                KeyCode::Down => dir_list_state.select_next(),
                _ => {}
            },
        }

        // Matching logic is only applied if query is updated. Due to the tui rendeing being
        // event drive, not having this condition would cause the matching logic to be be applied
        // for all keystrokes, even those that does not have an impact on the results presented.
        // ex. Using up and down arrows to navigate.
        if file_query != last_file_query && current_app_mode == AppMode::FileSearch {
            let query_as_bytes = prepare_fuzzy_target(&file_query);
            filtered_files = greedy_match_filter(query_as_bytes, &cached_paths);
            last_file_query = file_query.clone();
        }

        if diretory_query != last_directory_query && current_app_mode == AppMode::DirSearch {
            let dirs_query_as_bytes = prepare_fuzzy_target(&diretory_query);
            filtered_dirs = greedy_match_filter(dirs_query_as_bytes, &cached_dirs);
            last_directory_query = diretory_query.clone();
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

            let upper_horizontal_chunks = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
                .split(chunks[0]);

            // Define overarching style.
            let app_style = Style::default()
                .bg(Color::Rgb(31, 35, 53))
                .fg(Color::Rgb(220, 215, 186));

            // Define results widget.
            let file_results: List = List::new(
                filtered_files
                    .iter()
                    .take(25)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
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

            // Define directories widget.
            let dir_results: List = List::new(
                filtered_dirs
                    .iter()
                    .take(25)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            )
            .style(app_style)
            .block(
                Block::default()
                    .title(format!("{}", filtered_dirs.len()))
                    .borders(Borders::ALL),
            )
            .highlight_style(Style::new().italic().bold())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .direction(ListDirection::TopToBottom);

            // Define input widget.
            let current_query_to_show = match current_app_mode {
                AppMode::FileSearch => &file_query,
                AppMode::DirSearch => &diretory_query,
            };

            let input = Paragraph::new(Text::from(Span::raw(current_query_to_show)))
                .style(app_style)
                .block(Block::default().title("lazy-find").borders(Borders::ALL));

            // Render widgets.
            f.render_stateful_widget(
                file_results,
                upper_horizontal_chunks[0],
                &mut file_list_state,
            );
            f.render_stateful_widget(dir_results, upper_horizontal_chunks[1], &mut dir_list_state);
            f.render_widget(input, chunks[1]);
        })?;
        //}
    }
    Ok(())
}
