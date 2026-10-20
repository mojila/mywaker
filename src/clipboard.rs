//! Getting text out of the TUI and onto the user's clipboard.
//!
//! The terminal runs with mouse capture enabled (so the wheel scrolls the roster
//! and cards are clickable), which costs the user the terminal's own
//! click-and-drag selection. That is fine for ordinary reading and fatal for a
//! bug report: the one thing a user needs to paste into an issue — the error log
//! — is the thing they cannot select.
//!
//! So the app copies on the user's behalf. Three routes are tried, because no
//! single one works everywhere:
//!
//! 1. **OSC 52** — an escape sequence the *terminal* interprets and hands to the
//!    system clipboard. It works over SSH and inside tmux (with
//!    `set-clipboard on`), where a process-level clipboard tool would set the
//!    clipboard of the wrong machine.
//! 2. **A native tool** — `pbcopy` on macOS, `wl-copy`/`xclip`/`xsel` on Linux.
//!    Reliable when running locally, which is the common case.
//! 3. **A file** — always works, and the path is reported in the log so the text
//!    is reachable even when both clipboards are unavailable.
//!
//! The file is written unconditionally: it is the fallback the log line points
//! at, so "copy" never fails silently.

use std::io::Write;
use std::path::{Path, PathBuf};

/// The escape sequence that hands `text` to the terminal's clipboard.
///
/// `ESC ] 52 ; c ; <base64> ST` — `c` selects the system clipboard, and `ST`
/// (`ESC \`) terminates. The payload is base64 so arbitrary UTF-8 (box drawing,
/// `→`, `⚠`) survives the terminal's byte-oriented parser.
pub fn osc52(text: &str) -> String {
    use base64::Engine;
    let encoded = base64::engine::general_purpose::STANDARD.encode(text.as_bytes());
    format!("\x1b]52;c;{encoded}\x1b\\")
}

/// Where copied text is written when the user has no clipboard tool.
///
/// The data-local directory is the same root the logs use, so "copy" and "the
/// log file" sit together.
pub fn fallback_dir() -> Option<PathBuf> {
    dirs::data_local_dir().map(|dir| dir.join("mywaker").join("clipboard"))
}

/// Write `text` to a fresh file under [`fallback_dir`], returning its path.
///
/// The file is named for the caller's label and overwritten on each copy, so
/// repeated copies do not litter the directory while the path stays stable
/// enough to mention in a log line.
pub fn write_file(label: &str, text: &str) -> std::io::Result<PathBuf> {
    let dir = fallback_dir().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "no local data directory for the clipboard fallback",
        )
    })?;
    write_file_in(&dir, label, text)
}

/// [`write_file`] against an explicit directory (the seam unit tests drive).
pub fn write_file_in(dir: &Path, label: &str, text: &str) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let path = dir.join(format!("{}.txt", sanitize(label)));
    let mut file = std::fs::File::create(&path)?;
    file.write_all(text.as_bytes())?;
    Ok(path)
}

/// The command that puts stdin on the system clipboard, when one is available.
///
/// Checked in order rather than guessed from `cfg!(target_os)`: a Linux box may
/// have no display server at all, and a macOS box always has `pbcopy`.
fn native_tool() -> Option<&'static str> {
    const CANDIDATES: &[&str] = &["pbcopy", "wl-copy", "xclip", "xsel"];
    CANDIDATES.iter().copied().find(|tool| which(tool))
}

/// True when `tool` is on `PATH`.
fn which(tool: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| {
            std::env::split_paths(&paths).any(|dir| {
                let candidate = dir.join(tool);
                candidate.is_file()
            })
        })
        .unwrap_or(false)
}

/// Pipe `text` into the system clipboard via a native tool.
///
/// `xclip` and `xsel` need their selection argument; the others read stdin
/// bare. Returns `false` when no tool exists or the tool refused — the caller
/// still has OSC 52 and the file.
pub fn to_native(text: &str) -> bool {
    let Some(tool) = native_tool() else {
        return false;
    };
    let mut command = std::process::Command::new(tool);
    match tool {
        "xclip" => {
            command.args(["-selection", "clipboard"]);
        }
        "xsel" => {
            command.args(["--clipboard", "--input"]);
        }
        _ => {}
    }
    let Ok(mut child) = command
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
    else {
        return false;
    };
    if let Some(mut stdin) = child.stdin.take() {
        // A clipboard tool that exits early closes the pipe; that is a failure
        // to report, not a panic.
        if stdin.write_all(text.as_bytes()).is_err() {
            return false;
        }
    }
    child.wait().map(|status| status.success()).unwrap_or(false)
}

/// Make `label` safe as a file name.
fn sanitize(label: &str) -> String {
    let cleaned: String = label
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "clipboard".to_string()
    } else {
        cleaned
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The sequence must be exactly `ESC ] 52 ; c ; <base64> ST`, because a
    /// terminal that does not recognise it silently swallows the rest of the
    /// line — and a malformed one corrupts the screen.
    #[test]
    fn osc52_wraps_the_base64_payload_in_the_standard_sequence() {
        let sequence = osc52("hello");
        assert!(sequence.starts_with("\x1b]52;c;"), "{sequence:?}");
        assert!(sequence.ends_with("\x1b\\"), "{sequence:?}");
        // `aGVsbG8=` is base64 for "hello".
        assert!(sequence.contains("aGVsbG8="), "{sequence:?}");
    }

    /// Non-ASCII survives: a trace block is full of `→`, `⚠` and `·`, and the
    /// payload is base64 precisely so those do not have to.
    #[test]
    fn osc52_encodes_non_ascii_payloads() {
        use base64::Engine;
        let text = "orchestrator → writer · ⚠ timed out";
        let sequence = osc52(text);
        let payload = sequence
            .trim_start_matches("\x1b]52;c;")
            .trim_end_matches("\x1b\\");
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload)
            .expect("payload is valid base64");
        assert_eq!(String::from_utf8(decoded).unwrap(), text);
    }

    #[test]
    fn the_fallback_file_is_written_and_readable() {
        let dir = std::env::temp_dir().join(format!("mywaker-clip-{}", std::process::id()));
        let path = write_file_in(&dir, "error-log", "line one\nline two\n").unwrap();

        assert!(path.exists(), "{}", path.display());
        assert_eq!(
            std::fs::read_to_string(&path).unwrap(),
            "line one\nline two\n"
        );
        assert_eq!(path.file_name().unwrap(), "error-log.txt");

        // A second copy overwrites rather than piling up files.
        write_file_in(&dir, "error-log", "replaced").unwrap();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "replaced");

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A label with spaces or slashes must not escape the directory or produce a
    /// name the shell cannot address.
    #[test]
    fn labels_are_sanitized_into_plain_file_names() {
        assert_eq!(sanitize("error log"), "error-log");
        assert_eq!(sanitize("../../etc/passwd"), "------etc-passwd");
        assert_eq!(sanitize("trace:t1.2"), "trace-t1-2");
        assert_eq!(sanitize(""), "clipboard");
        // The sanitized name never contains a path separator.
        assert!(!sanitize("a/b").contains('/'));
    }

    #[test]
    fn osc52_handles_an_empty_payload() {
        let sequence = osc52("");
        assert_eq!(sequence, "\x1b]52;c;\x1b\\");
    }
}
