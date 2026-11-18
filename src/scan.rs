//! Directory scanner: walks a root and reports every `node_modules` folder.
//!
//! Rules:
//! - does not descend *into* a `node_modules` (nested ones are part of the parent)
//! - does not follow symlinks / junctions
//! - skips drive junk folders ($RECYCLE.BIN, System Volume Information)

use std::path::{Path, PathBuf};
use std::sync::mpsc::Sender;
use std::time::SystemTime;

pub struct Found {
    pub path: PathBuf,
    pub size: u64,
    pub files: u64,
    pub modified: Option<SystemTime>,
    pub has_pkg: bool,
}

pub enum ScanEvent {
    Found(Found),
    Progress { dirs: u64, current: PathBuf },
    Done { found: u64, dirs: u64, errors: u64 },
}

const SKIP_DIRS: &[&str] = &["$RECYCLE.BIN", "System Volume Information", ".git"];
const MAX_DEPTH: usize = 48;

pub fn spawn(root: PathBuf, tx: Sender<ScanEvent>) {
    std::thread::spawn(move || scan_blocking(&root, &tx));
}

pub fn scan_blocking(root: &Path, tx: &Sender<ScanEvent>) {
    let mut dirs: u64 = 0;
    let mut found: u64 = 0;
    let mut errors: u64 = 0;

    let mut it = walkdir::WalkDir::new(root)
        .follow_links(false)
        .max_depth(MAX_DEPTH)
        .into_iter();

    while let Some(next) = it.next() {
        let entry = match next {
            Ok(e) => e,
            Err(_) => {
                errors += 1;
                continue;
            }
        };

        if !entry.file_type().is_dir() {
            continue;
        }
        // symlink / junction dirs are skipped by walkdir (follow_links = false),
        // but bail out explicitly just in case.
        if entry.path_is_symlink() {
            it.skip_current_dir();
            continue;
        }

        let name = entry.file_name().to_string_lossy();

        if name == "node_modules" {
            let path = entry.path().to_path_buf();
            it.skip_current_dir();

            let (size, files, modified) = dir_stats(&path);
            found += 1;
            let has_pkg = path
                .parent()
                .map(|p| p.join("package.json").is_file())
                .unwrap_or(false);

            if tx
                .send(ScanEvent::Found(Found {
                    path,
                    size,
                    files,
                    modified,
                    has_pkg,
                }))
                .is_err()
            {
                return;
            }
            continue;
        }

        if SKIP_DIRS.iter().any(|s| s.eq_ignore_ascii_case(&name)) {
            it.skip_current_dir();
            continue;
        }

        dirs += 1;
        if dirs % 300 == 0
            && tx
                .send(ScanEvent::Progress {
                    dirs,
                    current: entry.path().to_path_buf(),
                })
                .is_err()
        {
            return;
        }
    }

    let _ = tx.send(ScanEvent::Done {
        found,
        dirs,
        errors,
    });
}

/// Total size, file count and newest inner mtime of a folder.
pub fn dir_stats(path: &Path) -> (u64, u64, Option<SystemTime>) {
    let mut size = 0u64;
    let mut files = 0u64;
    let mut newest: Option<SystemTime> = std::fs::metadata(path).ok().and_then(|m| m.modified().ok());

    for entry in walkdir::WalkDir::new(path)
        .follow_links(false)
        .into_iter()
        .flatten()
    {
        if !entry.file_type().is_file() {
            continue;
        }
        files += 1;
        if let Ok(md) = entry.metadata() {
            size += md.len();
            if let Ok(m) = md.modified() {
                if newest.map(|n| m > n).unwrap_or(true) {
                    newest = Some(m);
                }
            }
        }
    }
    (size, files, newest)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn tmp(name: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("nodewolf-test-{}-{}", name, std::process::id()));
        let _ = fs::remove_dir_all(&p);
        fs::create_dir_all(&p).unwrap();
        p
    }

    fn collect(root: &Path) -> Vec<Found> {
        let (tx, rx) = std::sync::mpsc::channel();
        scan_blocking(root, &tx);
        drop(tx);
        let mut out = Vec::new();
        for ev in rx {
            if let ScanEvent::Found(f) = ev {
                out.push(f);
            }
        }
        out.sort_by(|a, b| a.path.cmp(&b.path));
        out
    }

    #[test]
    fn finds_nested_projects_and_skips_double_nesting() {
        let root = tmp("nested");

        // a: real project
        fs::create_dir_all(root.join("a/node_modules/pkg")).unwrap();
        fs::write(root.join("a/package.json"), "{}").unwrap();
        fs::write(root.join("a/node_modules/pkg/index.js"), "x".repeat(100)).unwrap();

        // b: orphan (no package.json)
        fs::create_dir_all(root.join("b/node_modules")).unwrap();
        fs::write(root.join("b/node_modules/a.js"), "x".repeat(50)).unwrap();

        // nested node_modules inside node_modules must not be reported twice
        fs::create_dir_all(root.join("a/node_modules/dep/node_modules/x")).unwrap();

        // c: node_modules hidden deeper
        fs::create_dir_all(root.join("x/y/c/node_modules")).unwrap();

        let found = collect(&root);
        let names: Vec<String> = found
            .iter()
            .map(|f| {
                f.path
                    .strip_prefix(&root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/")
            })
            .collect();

        assert_eq!(names, vec!["a/node_modules", "b/node_modules", "x/y/c/node_modules"]);
        assert!(found[0].has_pkg);
        assert!(!found[1].has_pkg);
        assert_eq!(found[0].size, 100);
        assert_eq!(found[1].size, 50);
        assert!(found[0].files >= 1);

        fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn skips_recycle_bin() {
        let root = tmp("recycle");
        fs::create_dir_all(root.join("$RECYCLE.BIN/S-1-5/node_modules")).unwrap();
        fs::create_dir_all(root.join("keep/node_modules")).unwrap();
        let found = collect(&root);
        assert_eq!(found.len(), 1);
        fs::remove_dir_all(&root).unwrap();
    }
}
