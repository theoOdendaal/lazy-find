use std::fs::read_dir;
use std::path::{Path, PathBuf};

use crossterm::execute;
use crossterm::terminal::{self, disable_raw_mode, enable_raw_mode};
use fuzzy_matcher::FuzzyMatcher;
use fuzzy_matcher::skim::SkimMatcherV2;
use ratatui::Terminal;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::prelude::CrosstermBackend;
use ratatui::text::{Line, Span, Text};
use ratatui::widgets::{Block, Borders, Paragraph};
use rayon::prelude::*;
use walkdir::WalkDir;

use std::io;
use std::time::{Duration, Instant};

use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

/*
#[derive(Serialize, Deserialize, Debug, Clone)]
struct FileEntry {
    path: String,
    file_name: String,
    extension: Option<String>,
    modified: Option<u64>,
    size: Option<u64>,
}
*/

fn watch_for_changes() -> notify::Result<()> {
    let (tx, rx) = channel();

    let config = Config::default().with_poll_interval(Duration::from_secs(2));
    let mut watcher: RecommendedWatcher = Watcher::new(tx, config)?;

    watcher.watch("/".as_ref(), RecursiveMode::Recursive)?;

    std::thread::spawn(move || {
        let _watcher = watcher;

        for res in rx {
            match res {
                Ok(Event { paths, .. }) => {
                    // match on event type.
                    for path in paths {
                        if !should_ignore(&path) {
                            println!("{}", path.display());
                            // TODO update index.
                            // Changes should be accumulated, and updated periodically.
                        }
                    }
                }
                Err(e) => println!("watch error: {:?}", e),
            }
        }
    });

    Ok(())
}

fn should_ignore<P: AsRef<Path>>(path: P) -> bool {
    const EXCLUDED_DIRS: &[&str] = &[
        "C:/Windows/System32/",
        "C:/ProgramData/Microsoft/Windows/",
        "C:/ProgramData/McAfee/wps/",
        "C:/ProgramData/RivetNetworks/Killer/",
        "C:/$Recycle.Bin/",
        "C:/Users/TheoOdendaal/AppData/Local/Microsoft/Edge/User Data/",
        "C:/Users/TheoOdendaal/AppData/Roaming/",
    ];

    let path = path.as_ref();

    EXCLUDED_DIRS.par_iter().any(|excluded| {
        let excluded_path = Path::new(excluded);
        path.starts_with(excluded_path)
    })
}

// Function used to load all files from a directory, non-recursive.
//fn load_dir_non_par<P: AsRef<Path>>(dir: P) -> Vec<PathBuf> {}

// TODO Incorporate should_ignore function.
fn load_fs_par<P: AsRef<Path>>(dir: P) -> Vec<String> {
    // Load path of all file system entries as PathBuf by traversing the
    // specified directory in parallel.
    let start = Instant::now();
    let root_entries: Vec<PathBuf> = read_dir(dir)
        .expect("Directory not found")
        .filter_map(|res| res.ok())
        .map(|entry| entry.path())
        .collect();

    let mut file_population = Vec::new();
    let mut dirs_to_walk = Vec::new();

    for entry in root_entries {
        if entry.is_file() {
            if let Some(name) = entry
                .file_name()
                .and_then(|s| s.to_str())
                .map(|s| s.to_string())
            {
                file_population.push(name);
            }
        } else if entry.is_dir() {
            dirs_to_walk.push(entry);
        }
    }

    let mut other_file_population: Vec<String> = dirs_to_walk
        .par_iter()
        .filter(|dir| !should_ignore(dir))
        .flat_map_iter(|dir| {
            WalkDir::new(dir)
                .into_iter()
                .filter_map(Result::ok)
                .filter(|e| e.file_type().is_file() && !should_ignore(e.path()))
                .filter_map(|e| {
                    let full = e.path().to_path_buf();
                    let name = full.file_name()?.to_str()?.to_string();
                    Some(name)
                })
                .collect::<Vec<_>>()
        })
        .collect();

    file_population.append(&mut other_file_population);
    println!("Scanned fs in: {:?}", start.elapsed());
    file_population
}

fn run_app<B: ratatui::backend::Backend>(
    terminal: &mut Terminal<B>,
    file_population: Vec<String>,
) -> io::Result<()> {
    //let query = String::new();
    let query = "bloem".to_string();
    let matcher = SkimMatcherV2::default();

    // Fuzzy logic
    let mut matches: Vec<(String, i64)> = file_population
        .par_iter()
        //.with_min_len(100)
        .filter_map(|entry| {
            matcher
                .fuzzy_match(entry, &query)
                .map(|score| (entry.clone(), score))
        })
        .collect();

    matches.par_sort_unstable_by(|a, b| b.1.cmp(&a.1));
    let filtered: Vec<String> = matches.into_iter().map(|(p, _)| p).collect();
    // Draw Terminal
    terminal.draw(|f| {
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints([
                Constraint::Min(1),    // Main area
                Constraint::Length(5), // Input box height
            ])
            .split(f.area());

        // Top block (could hold other content)
        let result_text = filtered
            .par_iter()
            .take(38)
            .map(|line| {
                let path_str = Path::new(line).display().to_string();
                Line::from(Span::raw(path_str))
            })
            .collect::<Vec<_>>();

        let result_paragraph = Paragraph::new(result_text)
            .block(Block::default().title("lazy-find").borders(Borders::ALL));
        f.render_widget(result_paragraph, chunks[0]);

        // Bottom input box
        let query_block = Paragraph::new(Text::from(Span::raw(&query)))
            .block(Block::default().title("query").borders(Borders::ALL));
        f.render_widget(query_block, chunks[1]);
    })?;

    for k in filtered.iter() {
        println!("{:?}", k);
    }

    /*
    if crossterm::event::poll(std::time::Duration::from_millis(100))? {
        if let crossterm::event::Event::Key(key_event) = crossterm::event::read()? {
            match key_event.code {
                crossterm::event::KeyCode::Char('q') => break, // Quit
                crossterm::event::KeyCode::Char(c) => query.push(c), // Add typed char
                crossterm::event::KeyCode::Backspace => {
                    query.pop(); // Remove last character
                }
                _ => {}
            }
        }
    }
    */
    Ok(())
}

/*
fn main() -> notify::Result<()> {
    watch_for_changes()?;

    println!("Watching for file changes. Press Ctrl+C to exit.");
    loop {
        std::thread::sleep(std::time::Duration::from_secs(5));
    }
}
*/

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file_population = load_fs_par("/");

    enable_raw_mode()?;
    let mut stdout = io::stdout();

    execute!(
        stdout,
        crossterm::terminal::EnterAlternateScreen,
        crossterm::event::EnableMouseCapture
    )?;

    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let res = run_app(&mut terminal, file_population);

    disable_raw_mode()?;
    execute!(
        terminal.backend_mut(),
        crossterm::terminal::LeaveAlternateScreen,
        crossterm::event::DisableMouseCapture
    )?;
    terminal.show_cursor()?;

    Ok(res?)
}
