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
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;
use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::Instant;

// TODO use more effecient data structures?
// TODO, currently spaces are ignored? It's not even pushed to query. Is this fine?
// TODO better manage list selection. As currently it remains constant when typing and then falls off when selected exceeds index.
// TODO update query asynchronous and add debouncing to filter logic. i.e. query should be updated independently of filtering logic, and only apply filtering logic after a delay.
// TODO implement a watcher using notify, in order to update fs data without having to rescan entire disk during runtime.

/// Recursively traverse forward from the specified directory
/// in parallel returning a vec of file names.
fn walk_dir_par(dir: &Path) -> Vec<PathBuf> {
    match fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(Result::ok)
            .collect::<Vec<_>>()
            .into_par_iter()
            .flat_map(|entry| {
                let path = entry.path();
                if true {
                    //if !should_ignore(&path) {
                    match entry.file_type() {
                        Ok(file_type) => {
                            if file_type.is_dir() {
                                walk_dir_par(&path)
                            } else if file_type.is_file() {
                                vec![path]
                            } else {
                                vec![]
                            }
                        }
                        Err(_) => vec![],
                    }
                } else {
                    vec![]
                }
            })
            .collect(),
        Err(_) => vec![],
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

fn should_ignore(path: &Path) -> bool {
    path.components().any(|comp| {
        if let std::path::Component::Normal(name) = comp {
            name.to_string_lossy().starts_with('$')
        } else {
            false
        }
    })
}
/*
fn should_ignore(path: &PathBuf) -> bool {
    const EXCLUDED_DIRS: &[&str] = &[
        //"C:/Windows/System32/",
        //"C:/ProgramData/Microsoft/Windows/",
        //"C:/ProgramData/McAfee/wps/",
        //"C:/ProgramData/RivetNetworks/Killer/",
        //"C:/$Recycle.Bin/",
        //"C:/Users/TheoOdendaal/AppData/Local/Microsoft/Edge/User Data/",
        //"C:/Users/TheoOdendaal/AppData/Roaming/",
        "//$",
    ];

    EXCLUDED_DIRS
        .par_iter()
        .any(|excluded| path.starts_with(excluded))
}
*/

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

    if is_match {
        if let Some(first_idx) = first_match_idx {
            score += 20 - first_idx.min(20);
        }
        (true, score as i32)
    } else {
        (false, 0)
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = "paths.bincode";
    let file_population = load_paths(file)?;
    //let start = Instant::now();
    //let file_population = walk_dir_par(Path::new("/"));
    //println!("{:?}", file_population.len());
    //println!("{:?}", start.elapsed());
    //save_paths(&file_population, file)?;

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
    let mut query = String::new();
    let mut last_query = String::from(" ");
    let mut list_state = ListState::default().with_selected(Some(0));
    let mut filtered: Vec<Cow<str>> = Vec::new();

    // Pre cash paths.
    // 0 = file name as bytes (stripped of spaces) - used for matching.
    // 1 = full path - used for display.
    let cached_paths: Vec<(Vec<u8>, Cow<str>)> = file_population
        .par_iter()
        .filter_map(|s| {
            let file_name: Vec<u8> = s
                .file_name()?
                .to_string_lossy()
                .bytes()
                .filter(|b| *b != b' ')
                .collect();
            let full_path = s.to_string_lossy();

            Some((file_name, Cow::Owned(full_path.into_owned())))
        })
        .collect();

    loop {
        // Handle keypresses
        if let crossterm::event::Event::Key(crossterm::event::KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = crossterm::event::read()?
        {
            match code {
                KeyCode::Esc => break,
                // Ignore spaces. Won't render
                KeyCode::Char(' ') => continue,
                KeyCode::Char(c) => query.push(c),
                KeyCode::Backspace => {
                    query.pop();
                }
                KeyCode::Up => {
                    list_state.select_next();
                }

                KeyCode::Down => {
                    list_state.select_previous();
                }
                _ => {}
            }
        }

        if query != last_query {
            let query_as_bytes: Vec<u8> = query.bytes().filter(|b| *b != b' ').collect();

            let mut matched: Vec<(i32, Cow<str>)> = cached_paths
                .par_iter()
                .filter_map(|(name, path)| {
                    let (is_match, score) = greedy_match_score(&query_as_bytes, name);
                    if is_match {
                        Some((score, path.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            matched.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

            filtered = matched
                .into_iter()
                .map(|(_, path)| path)
                .collect::<Vec<Cow<str>>>();

            last_query = query.clone();
        }

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
            let results: List = List::new(
                filtered
                    .iter()
                    .take(30)
                    .map(|s| s.to_string())
                    .collect::<Vec<String>>(),
            )
            .style(app_style)
            .block(Block::default().title("lazy-find").borders(Borders::ALL))
            .highlight_style(Style::new().italic().bold())
            .highlight_symbol(">>")
            .repeat_highlight_symbol(true)
            .highlight_spacing(ratatui::widgets::HighlightSpacing::Always)
            .direction(ListDirection::BottomToTop);

            // Define input widget.
            let input = Paragraph::new(Text::from(Span::raw(&query)))
                .style(app_style)
                .block(
                    Block::default()
                        .title(format!("{}", filtered.len()))
                        .borders(Borders::ALL),
                );

            // Render widgets.
            f.render_stateful_widget(results, chunks[0], &mut list_state);
            f.render_widget(input, chunks[1]);
        })?;
        //}
    }
    Ok(())
}
