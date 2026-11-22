//! A local crash log: a panic hook writes the panic, a backtrace, the app version and the OS to
//! `crashes/crash-<unix seconds>.txt` next to the prefs, keeps the last [`KEEP`] files, and then
//! calls the hook that was installed before. Nothing is sent anywhere; the next launch only logs
//! the path of a crash it has not mentioned yet.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// How many crash files are kept.
pub const KEEP: usize = 5;
const SEEN: &str = "last-seen";

/// The directory crash files go in: `crashes/` beside `prefs.json`.
pub fn crash_dir() -> Option<PathBuf> {
    let prefs = rune::prefs::Prefs::open(crate::real::APP_ID);
    Some(prefs.path()?.parent()?.join("crashes"))
}

/// Installs the panic hook, wrapping whatever hook is set now.
pub fn install(dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let report = format!(
            "thehideout {} ({})\n{}\nthread: {}\n\nbacktrace:\n{}\n",
            env!("CARGO_PKG_VERSION"),
            std::env::consts::OS,
            info,
            thread.name().unwrap_or("<unnamed>"),
            std::backtrace::Backtrace::force_capture(),
        );
        if let Err(e) = write_report(&dir, &report, now_secs()) {
            eprintln!("could not write a crash report: {e}");
        }
        previous(info);
    }));
}

/// On launch: logs the newest crash file if it is newer than the last one mentioned.
pub fn report_unseen(dir: &Path) {
    let names = crash_files(dir);
    let seen = fs::read_to_string(dir.join(SEEN)).ok();
    if let Some(name) = unseen(&names, seen.as_deref().map(str::trim)) {
        eprintln!("thehideout crashed last time; the report is {}", dir.join(name).display());
        let _ = fs::write(dir.join(SEEN), name);
    }
}

fn now_secs() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0)
}

fn is_crash_file(name: &str) -> bool {
    name.starts_with("crash-") && name.ends_with(".txt")
}

/// Crash file names in `dir`, oldest first (the names sort by time).
fn crash_files(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| e.file_name().into_string().ok())
        .filter(|n| is_crash_file(n))
        .collect();
    names.sort();
    names
}

/// The newest of `names` (sorted, oldest first) when it is after `seen`.
fn unseen<'a>(names: &'a [String], seen: Option<&str>) -> Option<&'a str> {
    let newest = names.last()?;
    match seen {
        Some(seen) if seen >= newest.as_str() => None,
        _ => Some(newest),
    }
}

/// Writes `report` as a new crash file for `secs` and drops all but the newest [`KEEP`].
fn write_report(dir: &Path, report: &str, secs: u64) -> std::io::Result<PathBuf> {
    fs::create_dir_all(dir)?;
    let mut n = 0;
    let (path, mut file) = loop {
        let name = if n == 0 { format!("crash-{secs:012}.txt") } else { format!("crash-{secs:012}-{n}.txt") };
        let path = dir.join(name);
        match fs::OpenOptions::new().write(true).create_new(true).open(&path) {
            Ok(file) => break (path, file),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => n += 1,
            Err(e) => return Err(e),
        }
    };
    file.write_all(report.as_bytes())?;
    let names = crash_files(dir);
    for old in names.iter().take(names.len().saturating_sub(KEEP)) {
        let _ = fs::remove_file(dir.join(old));
    }
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("thehideout-crash-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    #[test]
    fn keeps_only_the_last_five() {
        let dir = temp("rotate");
        for secs in 1..=8 {
            write_report(&dir, &format!("report {secs}"), secs).unwrap();
        }
        let names = crash_files(&dir);
        assert_eq!(names.len(), KEEP);
        assert_eq!(names.first().unwrap(), "crash-000000000004.txt");
        assert_eq!(names.last().unwrap(), "crash-000000000008.txt");
        assert_eq!(fs::read_to_string(dir.join("crash-000000000008.txt")).unwrap(), "report 8");
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn same_second_crashes_do_not_overwrite() {
        let dir = temp("same");
        let a = write_report(&dir, "a", 7).unwrap();
        let b = write_report(&dir, "b", 7).unwrap();
        assert_ne!(a, b);
        assert_eq!(crash_files(&dir).len(), 2);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn other_files_are_not_counted_or_rotated() {
        let dir = temp("other");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join(SEEN), "x").unwrap();
        for secs in 1..=6 {
            write_report(&dir, "r", secs).unwrap();
        }
        assert!(dir.join(SEEN).exists());
        assert_eq!(crash_files(&dir).len(), KEEP);
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn mentions_a_crash_once() {
        let names = vec!["crash-000000000001.txt".to_owned(), "crash-000000000002.txt".to_owned()];
        assert_eq!(unseen(&[], None), None);
        assert_eq!(unseen(&names, None), Some("crash-000000000002.txt"));
        assert_eq!(unseen(&names, Some("crash-000000000001.txt")), Some("crash-000000000002.txt"));
        assert_eq!(unseen(&names, Some("crash-000000000002.txt")), None);
    }
}
