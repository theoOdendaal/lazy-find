use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::sync::mpsc::channel;

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
