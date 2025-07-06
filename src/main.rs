use bincode::decode_from_std_read;
use crossterm::event;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::execute;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use ratatui::Terminal;
use ratatui::layout::Constraint;
use ratatui::layout::Direction;
use ratatui::layout::Layout;
use ratatui::prelude::CrosstermBackend;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::text::Text;
use ratatui::widgets::Block;
use ratatui::widgets::Borders;
use ratatui::widgets::Paragraph;
use rayon::iter::IntoParallelIterator;
use rayon::iter::IntoParallelRefIterator;
use rayon::iter::ParallelIterator;
use std::fs;
use std::fs::File;
use std::io;
use std::io::BufReader;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;
use std::time::Instant;

// TODO replace String with Cow<String> ?

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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = "paths.bincode";
    let file_population = load_paths(file)?;
    //let file_population = walk_dir_par(Path::new("/"));
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
            if event::poll(std::time::Duration::from_millis(50)).unwrap() {
                if let Ok(Event::Key(key_event)) = event::read() {
                    if tx.send(key_event).await.is_err() {
                        break;
                    }
                }
            }
        }
    });

    let cached_paths: Vec<(String, String)> = file_population
        .par_iter()
        .filter_map(|s| {
            let file_name = s.file_name()?.to_string_lossy().to_string();
            let full_path = s.to_string_lossy().to_string();
            Some((file_name, full_path))
        })
        .collect();

    let mut filtered: Vec<String> = Vec::new();

    loop {
        if query != last_query {
            filtered = cached_paths
                .par_iter()
                .filter(|(name, _)| name.contains(&query))
                .map(|(_, path)| path.clone())
                .collect();

            last_query = query.clone();

            terminal.draw(|f| {
                let chunks = Layout::default()
                    .direction(Direction::Vertical)
                    .margin(1)
                    .constraints([Constraint::Min(1), Constraint::Length(3)])
                    .split(f.area());

                let result_text: Vec<Line> = filtered
                    .iter()
                    .take(38)
                    .map(|line| Line::from(Span::raw(line)))
                    .collect();

                let result_paragraph = Paragraph::new(result_text)
                    .block(Block::default().title("lazy-find").borders(Borders::ALL));
                f.render_widget(result_paragraph, chunks[0]);

                let input_paragraph = Paragraph::new(Text::from(Span::raw(&query)))
                    .block(Block::default().title("query").borders(Borders::ALL));
                f.render_widget(input_paragraph, chunks[1]);
            })?;
        }

        if let Some(KeyEvent {
            code,
            kind: KeyEventKind::Press,
            ..
        }) = rx.recv().await
        {
            match code {
                KeyCode::Char('q') => break,
                KeyCode::Char(c) => query.push(c),
                KeyCode::Backspace => {
                    query.pop();
                }
                _ => {}
            }
        }
    }

    Ok(())
}
