use bincode::decode_from_std_read;
use crossterm::event::{KeyCode, KeyEventKind};
use crossterm::execute;
use crossterm::terminal::{disable_raw_mode, enable_raw_mode};
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::CrosstermBackend;
use ratatui::style::{Color, Modifier, Style, Stylize};
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use rayon::iter::{IntoParallelIterator, IntoParallelRefIterator, ParallelIterator};
use rayon::slice::ParallelSliceMut;
use std::borrow::Cow;
use std::fs;
use std::fs::File;
use std::io::{self, BufReader, BufWriter};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};
use tokio::select;

// TODO use more effecient data structures?

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
    //let file = "paths.bincode";
    //let file_population = load_paths(file)?;
    //let start = Instant::now();
    let file_population = walk_dir_par(Path::new("/"));
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

    let (tx, mut rx) = tokio::sync::mpsc::channel(100);

    tokio::spawn(async move {
        loop {
            if crossterm::event::poll(std::time::Duration::from_millis(50)).unwrap() {
                if let Ok(crossterm::event::Event::Key(key)) = crossterm::event::read() {
                    if tx.send(key).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    // Pre cash paths.
    // 0 = file name (stripped of spaces) - used for matching.
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

    let debounce_timer = tokio::time::sleep(Duration::from_millis(50));
    tokio::pin!(debounce_timer);

    loop {
        select! {
                    // Handle keypresses
                    Some(key_event) = rx.recv() => {
                        if let crossterm::event::KeyEvent { code, kind: KeyEventKind::Press, .. } = key_event {
                            match code {
                                KeyCode::Char('q') => break,
                                KeyCode::Char(c) => query.push(c),
                                KeyCode::Backspace => { query.pop(); },
                                _ => {}
                            }

                            debounce_timer.as_mut().reset(tokio::time::Instant::now() + Duration::from_millis(50));
                        }
                    }

                    () = &mut debounce_timer => {

                        if query != last_query {

                            let query_as_bytes: Vec<u8> = query.bytes().filter(|b| *b != b' ').collect();

                            let mut filtered: Vec<(i32, Cow<str>)> = cached_paths
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

                            filtered.par_sort_unstable_by(|a, b| b.0.cmp(&a.0));

                            let filtered: Vec<Cow<str>> = filtered
                                .into_iter()
                                .map(|(_, path)| path)
                                .collect();

                            last_query = query.clone();

                            // Render terminal
                            terminal.draw(|f| {
                                let chunks = Layout::default()
                                    .direction(Direction::Vertical)
                                    .margin(1)
                                    .constraints([
                                        Constraint::Min(1),
                                        Constraint::Length(3),
                                    ])
                                    .split(f.area());

                                let result_lines: Vec<Line> = filtered
                                    .iter()
                                    .take(30)
                                    .map(|line| {
                                        Line::from(Span::raw(
                                            line.to_string()
                                        ))
                                    })
                                    .collect();
                                /*
                                let results = Paragraph::new(result_lines)
                                    .block(Block::default().title("lazy-find").borders(Borders::ALL));
                                f.render_widget(results, chunks[0]);

                                let input = Paragraph::new(Text::from(Span::raw(&query)))
                                    .block(Block::default().borders(Borders::ALL));
                                f.render_widget(input, chunks[1]);
                                */

                                let results = Paragraph::new(result_lines)
            .block(
                Block::default()
                    .title(Span::styled(
                        " lazy-find ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)),
            )
            .style(Style::default().fg(Color::White));
        f.render_widget(results, chunks[0]);

        let input = Paragraph::new(Text::from(Span::styled(
            &query,
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::ITALIC),
        )))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(Span::styled(
                    " query ",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                ))
                .border_style(Style::default().fg(Color::DarkGray)),
        );
        f.render_widget(input, chunks[1]);

                        })?;
                    }}
                }
    }

    Ok(())
}
