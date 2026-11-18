mod scan;
mod ui;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant, SystemTime};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

pub struct Item {
    pub path: PathBuf,
    pub size: u64,
    pub files: u64,
    pub modified: Option<SystemTime>,
    pub has_pkg: bool,
    pub selected: bool,
    pub deleting: bool,
    pub error: Option<String>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Sort {
    SizeDesc,
    SizeAsc,
    AgeOld,
    AgeNew,
    PathAsc,
}

impl Sort {
    fn next(self) -> Sort {
        match self {
            Sort::SizeDesc => Sort::SizeAsc,
            Sort::SizeAsc => Sort::AgeOld,
            Sort::AgeOld => Sort::AgeNew,
            Sort::AgeNew => Sort::PathAsc,
            Sort::PathAsc => Sort::SizeDesc,
        }
    }

    fn label(self) -> &'static str {
        match self {
            Sort::SizeDesc => "size desc",
            Sort::SizeAsc => "size asc",
            Sort::AgeOld => "oldest first",
            Sort::AgeNew => "newest first",
            Sort::PathAsc => "path a-z",
        }
    }
}

enum DelEvent {
    Deleted {
        path: PathBuf,
        freed: u64,
        err: Option<String>,
    },
    Done {
        freed: u64,
        ok: usize,
        failed: usize,
    },
}

pub struct App {
    pub root: PathBuf,
    pub items: Vec<Item>,
    pub visible: Vec<usize>,
    pub cursor: usize,
    pub sort: Sort,
    pub filter: String,
    pub filter_mode: bool,
    pub only_orphan: bool,
    pub confirm: bool,
    pub scanning: bool,
    pub scan_dirs: u64,
    pub scan_current: String,
    pub status: String,
    pub deleting: bool,
    pub freed_total: u64,
    pub spinner: usize,
    dirty: bool,
    scan_rx: Receiver<scan::ScanEvent>,
    del_rx: Receiver<DelEvent>,
    del_tx: Sender<DelEvent>,
}

impl App {
    pub fn new(root: PathBuf) -> Self {
        let (scan_tx, scan_rx) = mpsc::channel();
        let (del_tx, del_rx) = mpsc::channel();
        let mut app = App {
            root: root.clone(),
            items: Vec::new(),
            visible: Vec::new(),
            cursor: 0,
            sort: Sort::SizeDesc,
            filter: String::new(),
            filter_mode: false,
            only_orphan: false,
            confirm: false,
            scanning: true,
            scan_dirs: 0,
            scan_current: String::new(),
            status: "scanning...".into(),
            deleting: false,
            freed_total: 0,
            spinner: 0,
            dirty: true,
            scan_rx,
            del_rx,
            del_tx,
        };
        scan::spawn(root, scan_tx);
        app.status = format!("scanning {}", app.root.display());
        app
    }

    fn rescan(&mut self) {
        let (tx, rx) = mpsc::channel();
        self.scan_rx = rx;
        self.items.clear();
        self.visible.clear();
        self.cursor = 0;
        self.scan_dirs = 0;
        self.scan_current.clear();
        self.scanning = true;
        self.dirty = true;
        self.status = format!("scanning {}", self.root.display());
        scan::spawn(self.root.clone(), tx);
    }

    pub fn pump(&mut self) {
        while let Ok(ev) = self.scan_rx.try_recv() {
            match ev {
                scan::ScanEvent::Found(f) => {
                    self.items.push(Item {
                        path: f.path,
                        size: f.size,
                        files: f.files,
                        modified: f.modified,
                        has_pkg: f.has_pkg,
                        selected: false,
                        deleting: false,
                        error: None,
                    });
                    self.dirty = true;
                }
                scan::ScanEvent::Progress { dirs, current } => {
                    self.scan_dirs = dirs;
                    self.scan_current = current.display().to_string();
                }
                scan::ScanEvent::Done {
                    found,
                    dirs,
                    errors,
                } => {
                    self.scanning = false;
                    self.status = format!(
                        "scan done - {} node_modules, {} folders walked{}",
                        found,
                        dirs,
                        if errors > 0 {
                            format!(", {} unreadable", errors)
                        } else {
                            String::new()
                        }
                    );
                }
            }
        }

        while let Ok(ev) = self.del_rx.try_recv() {
            match ev {
                DelEvent::Deleted { path, freed, err } => {
                    if let Some(i) = self.items.iter().position(|it| it.path == path) {
                        match err {
                            None => {
                                self.freed_total += freed;
                                self.items.remove(i);
                            }
                            Some(e) => {
                                self.items[i].deleting = false;
                                self.items[i].error = Some(e);
                            }
                        }
                    }
                    self.dirty = true;
                }
                DelEvent::Done { freed, ok, failed } => {
                    self.deleting = false;
                    self.status = if failed == 0 {
                        format!("deleted {} folder(s), freed {}", ok, human_size(freed))
                    } else {
                        format!(
                            "deleted {} folder(s), freed {} - {} failed (see red rows)",
                            ok,
                            human_size(freed),
                            failed
                        )
                    };
                }
            }
        }
    }

    pub fn tick(&mut self) {
        self.spinner = self.spinner.wrapping_add(1);
        if self.dirty {
            self.rebuild_visible();
            self.dirty = false;
        }
    }

    fn rebuild_visible(&mut self) {
        let needle = self.filter.to_lowercase();
        let mut idx: Vec<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| {
                (!self.only_orphan || !it.has_pkg)
                    && (needle.is_empty()
                        || it.path.to_string_lossy().to_lowercase().contains(&needle))
            })
            .map(|(i, _)| i)
            .collect();

        let items = &self.items;
        match self.sort {
            Sort::SizeDesc => idx.sort_by(|a, b| items[*b].size.cmp(&items[*a].size)),
            Sort::SizeAsc => idx.sort_by(|a, b| items[*a].size.cmp(&items[*b].size)),
            Sort::AgeOld => idx.sort_by(|a, b| items[*a].modified.cmp(&items[*b].modified)),
            Sort::AgeNew => idx.sort_by(|a, b| items[*b].modified.cmp(&items[*a].modified)),
            Sort::PathAsc => idx.sort_by(|a, b| items[*a].path.cmp(&items[*b].path)),
        }

        self.visible = idx;
        if self.cursor >= self.visible.len() {
            self.cursor = self.visible.len().saturating_sub(1);
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) -> bool {
        if key.kind != KeyEventKind::Press {
            return false;
        }

        // filter input mode
        if self.filter_mode {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => self.filter_mode = false,
                KeyCode::Backspace => {
                    self.filter.pop();
                    self.dirty = true;
                }
                KeyCode::Char(c) => {
                    self.filter.push(c);
                    self.dirty = true;
                }
                _ => {}
            }
            return false;
        }

        // confirm dialog
        if self.confirm {
            match key.code {
                KeyCode::Char('y') | KeyCode::Char('Y') => {
                    self.confirm = false;
                    self.start_delete();
                }
                KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => {
                    self.confirm = false;
                    self.status = "delete cancelled".into();
                }
                _ => {}
            }
            return false;
        }

        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                if self.deleting {
                    self.status = "still deleting, wait...".into();
                    return false;
                }
                return true;
            }
            KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => return true,

            KeyCode::Down | KeyCode::Char('j') => {
                if !self.visible.is_empty() && self.cursor + 1 < self.visible.len() {
                    self.cursor += 1;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.cursor = self.cursor.saturating_sub(1),
            KeyCode::PageDown => {
                self.cursor = (self.cursor + 15).min(self.visible.len().saturating_sub(1));
            }
            KeyCode::PageUp => self.cursor = self.cursor.saturating_sub(15),
            KeyCode::Home | KeyCode::Char('g') => self.cursor = 0,
            KeyCode::End | KeyCode::Char('G') => {
                self.cursor = self.visible.len().saturating_sub(1)
            }

            KeyCode::Char(' ') => {
                if let Some(&i) = self.visible.get(self.cursor) {
                    self.items[i].selected = !self.items[i].selected;
                    if self.cursor + 1 < self.visible.len() {
                        self.cursor += 1;
                    }
                }
            }
            KeyCode::Char('a') => {
                for &i in &self.visible {
                    self.items[i].selected = true;
                }
            }
            KeyCode::Char('n') => {
                for it in self.items.iter_mut() {
                    it.selected = false;
                }
            }
            KeyCode::Char('o') => {
                self.only_orphan = !self.only_orphan;
                self.dirty = true;
                self.status = if self.only_orphan {
                    "showing only folders with no package.json".into()
                } else {
                    "showing all node_modules".into()
                };
            }
            KeyCode::Char('s') => {
                self.sort = self.sort.next();
                self.dirty = true;
                self.status = format!("sort: {}", self.sort.label());
            }
            KeyCode::Char('/') => self.filter_mode = true,
            KeyCode::Char('r') => self.rescan(),
            KeyCode::Char('d') | KeyCode::Delete => self.ask_delete(),
            _ => {}
        }
        false
    }

    fn ask_delete(&mut self) {
        let n = self.items.iter().filter(|i| i.selected).count();
        if n == 0 {
            self.status = "nothing selected - space to select, a to select all".into();
            return;
        }
        if self.deleting {
            self.status = "already deleting...".into();
            return;
        }
        self.confirm = true;
    }

    fn start_delete(&mut self) {
        let targets: Vec<(PathBuf, u64)> = self
            .items
            .iter()
            .filter(|i| i.selected && !i.deleting)
            .map(|i| (i.path.clone(), i.size))
            .collect();

        if targets.is_empty() {
            self.status = "nothing selected".into();
            return;
        }

        for it in self.items.iter_mut() {
            if it.selected {
                it.deleting = true;
                it.error = None;
            }
        }

        self.deleting = true;
        self.status = format!("deleting {} folder(s)...", targets.len());
        let tx = self.del_tx.clone();

        std::thread::spawn(move || {
            let mut freed = 0u64;
            let mut ok = 0usize;
            let mut failed = 0usize;
            for (path, size) in targets {
                match force_remove(&path) {
                    Ok(()) => {
                        ok += 1;
                        freed += size;
                        let _ = tx.send(DelEvent::Deleted {
                            path,
                            freed: size,
                            err: None,
                        });
                    }
                    Err(e) => {
                        failed += 1;
                        let _ = tx.send(DelEvent::Deleted {
                            path,
                            freed: 0,
                            err: Some(e.to_string()),
                        });
                    }
                }
            }
            let _ = tx.send(DelEvent::Done { freed, ok, failed });
        });
    }

    pub fn selected_count(&self) -> usize {
        self.items.iter().filter(|i| i.selected).count()
    }

    pub fn selected_size(&self) -> u64 {
        self.items
            .iter()
            .filter(|i| i.selected)
            .map(|it| it.size)
            .sum()
    }

    pub fn total_size(&self) -> u64 {
        self.items.iter().map(|it| it.size).sum()
    }
}

/// `remove_dir_all`, but tolerates read-only files (common on Windows node_modules).
fn force_remove(path: &Path) -> std::io::Result<()> {
    if path.file_name().map(|n| n == "node_modules") != Some(true) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "refusing to delete a folder that is not named node_modules",
        ));
    }
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(first) => {
            if clear_readonly(path) {
                std::fs::remove_dir_all(path)
            } else {
                Err(first)
            }
        }
    }
}

#[cfg(windows)]
fn clear_readonly(root: &Path) -> bool {
    let mut touched = false;
    for entry in walkdir::WalkDir::new(root).into_iter().flatten() {
        if let Ok(md) = entry.metadata() {
            let mut perms = md.permissions();
            if perms.readonly() {
                perms.set_readonly(false);
                if std::fs::set_permissions(entry.path(), perms).is_ok() {
                    touched = true;
                }
            }
        }
    }
    touched
}

#[cfg(not(windows))]
fn clear_readonly(_root: &Path) -> bool {
    false
}

/// Drops the Windows `\\?\` verbatim prefix so paths stay readable.
pub fn clean_path(p: &Path) -> String {
    let s = p.display().to_string();
    match s.strip_prefix(r"\\?\") {
        Some(rest) => rest.to_string(),
        None => s,
    }
}

pub fn human_size(n: u64) -> String {
    const K: f64 = 1024.0;
    let f = n as f64;
    if f >= K * K * K {
        format!("{:.2} GB", f / (K * K * K))
    } else if f >= K * K {
        format!("{:.1} MB", f / (K * K))
    } else if f >= K {
        format!("{:.0} KB", f / K)
    } else {
        format!("{} B", n)
    }
}

pub fn age_label(t: Option<SystemTime>) -> String {
    let Some(t) = t else {
        return "?".into();
    };
    let Ok(d) = SystemTime::now().duration_since(t) else {
        return "now".into();
    };
    let days = d.as_secs() / 86_400;
    if days == 0 {
        "today".into()
    } else if days < 30 {
        format!("{}d", days)
    } else if days < 365 {
        format!("{}mo", days / 30)
    } else {
        format!("{}y", days / 365)
    }
}

/// Headless mode: print found folders and exit (handy for scripting / testing).
fn list_mode(root: &Path) -> std::io::Result<()> {
    let (tx, rx) = mpsc::channel();
    let root_owned = root.to_path_buf();
    let handle = std::thread::spawn(move || scan::scan_blocking(&root_owned, &tx));
    let mut found: Vec<scan::Found> = Vec::new();
    for ev in rx {
        match ev {
            scan::ScanEvent::Found(f) => found.push(f),
            scan::ScanEvent::Progress { dirs, .. } => {
                eprint!("\rwalked {} folders, found {} node_modules", dirs, found.len());
            }
            scan::ScanEvent::Done { .. } => {}
        }
    }
    handle.join().ok();
    eprintln!();

    found.sort_by(|a, b| b.size.cmp(&a.size));
    let total: u64 = found.iter().map(|f| f.size).sum();
    for f in &found {
        println!(
            "{:>10}  {:<4}  {}",
            human_size(f.size),
            if f.has_pkg { "pkg" } else { "NO-PKG" },
            clean_path(&f.path)
        );
    }
    println!();
    println!(
        "{} node_modules folder(s), {} total",
        found.len(),
        human_size(total)
    );
    Ok(())
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let list = args.iter().any(|a| a == "--list" || a == "-l");
    let arg = args
        .iter()
        .find(|a| !a.starts_with('-'))
        .map(|a| a.to_string());
    let root = match arg {
        _ if args.iter().any(|a| a == "-h" || a == "--help") => {
            println!("nodewolf - find and delete node_modules folders\n");
            println!("usage: nodewolf [PATH]   (default: current directory)\n");
            println!("keys:");
            println!("  j/k, arrows   move        space  select");
            println!("  a / n         select all / none");
            println!("  d             delete selected (asks y/n)");
            println!("  o             toggle only-orphaned filter");
            println!("  s             cycle sort    /  filter text");
            println!("  r             rescan        q  quit");
            return Ok(());
        }
        Some(a) => PathBuf::from(a),
        None => std::env::current_dir()?,
    };

    if !root.is_dir() {
        eprintln!("not a directory: {}", root.display());
        std::process::exit(1);
    }
    let root = root.canonicalize().unwrap_or(root);

    if list {
        return list_mode(&root);
    }

    let mut terminal = ratatui::init();
    let res = run(&mut terminal, root);
    ratatui::restore();
    res
}

fn run(terminal: &mut DefaultTerminal, root: PathBuf) -> std::io::Result<()> {
    let mut app = App::new(root);
    let mut last_tick = Instant::now();

    loop {
        app.pump();
        app.tick();
        terminal.draw(|f| ui::draw(f, &mut app))?;

        if event::poll(Duration::from_millis(80))? {
            if let Event::Key(key) = event::read()? {
                if app.handle_key(key) {
                    break;
                }
            }
        }
        if last_tick.elapsed() >= Duration::from_millis(100) {
            last_tick = Instant::now();
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn mock_tree(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("nodewolf-del-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("orphan/node_modules/pkg")).unwrap();
        fs::write(root.join("orphan/node_modules/pkg/i.js"), vec![b'x'; 8192]).unwrap();
        fs::create_dir_all(root.join("live/node_modules/react")).unwrap();
        fs::write(root.join("live/package.json"), "{}").unwrap();
        fs::write(root.join("live/node_modules/react/i.js"), vec![b'x'; 2048]).unwrap();
        root
    }

    fn wait_idle(app: &mut App) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while (app.scanning || app.deleting) && Instant::now() < deadline {
            app.pump();
            std::thread::sleep(Duration::from_millis(20));
        }
        app.pump();
        app.tick();
    }

    #[test]
    fn deletes_only_selected_mock_node_modules() {
        let root = mock_tree("selected");
        let mut app = App::new(root.clone());
        wait_idle(&mut app);
        assert_eq!(app.items.len(), 2, "expected two node_modules in mock tree");

        for it in app.items.iter_mut() {
            if !it.has_pkg {
                it.selected = true;
            }
        }
        assert_eq!(app.selected_count(), 1);

        app.start_delete();
        wait_idle(&mut app);

        assert!(!root.join("orphan/node_modules").exists(), "orphan should be gone");
        assert!(root.join("live/node_modules").exists(), "healthy project must survive");
        assert_eq!(app.items.len(), 1);
        assert!(app.freed_total >= 8192);

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn delete_handles_read_only_files() {
        let root = mock_tree("readonly");
        let target = root.join("orphan/node_modules");
        let file = target.join("pkg/i.js");
        let mut perms = fs::metadata(&file).unwrap().permissions();
        perms.set_readonly(true);
        fs::set_permissions(&file, perms).unwrap();

        let res = force_remove(&target);
        assert!(res.is_ok(), "read-only content should still be removable: {:?}", res.err());
        assert!(!target.exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn refuses_to_delete_other_folders() {
        let root = std::env::temp_dir().join(format!("nodewolf-guard-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(root.join("precious")).unwrap();

        let res = force_remove(&root.join("precious"));
        assert!(res.is_err(), "guard must reject non-node_modules paths");
        assert!(root.join("precious").exists());

        fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn human_size_formats() {
        assert_eq!(human_size(0), "0 B");
        assert_eq!(human_size(2048), "2 KB");
        assert_eq!(human_size(5 * 1024 * 1024), "5.0 MB");
    }
}
