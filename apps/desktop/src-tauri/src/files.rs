//! Attachment file actions: save a copy, copy to the clipboard, move to the
//! Trash. Trashing is limited to files inside ~/Library/Messages/Attachments
//! and always goes through the Trash (restorable), never a hard delete.

use std::path::{Path, PathBuf};

use crate::error::{CmdError, CmdResult};

/// `~/Library/<rel>`, resolved.
fn library_dir(rel: &str) -> CmdResult<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| CmdError::internal("HOME is not set"))?;
    let dir = PathBuf::from(home).join("Library").join(rel);
    dir.canonicalize()
        .map_err(|e| CmdError::permission(format!("can't read {}: {e}", dir.display())))
}

/// `path`, resolved, if it's an existing file inside `root`. Every file action
/// goes through this: stored paths come from chat.db, which other people's
/// messages populate, so they're checked rather than trusted.
fn checked_file_in(path: &str, root: &Path) -> CmdResult<PathBuf> {
    if !path.starts_with('/') {
        return Err(CmdError::invalid("not an absolute path"));
    }
    let p = Path::new(path).canonicalize().map_err(|_| {
        CmdError::not_found("the file isn't on this Mac (it may be in iCloud only)")
    })?;
    if !p.is_file() {
        return Err(CmdError::not_found("not a file"));
    }
    if !p.starts_with(root) {
        return Err(CmdError::invalid("only Messages attachments can be used"));
    }
    Ok(p)
}

/// A file under ~/Library/Messages (attachments, stickers): open, save, copy, reveal.
pub fn checked_messages_file(path: &str) -> CmdResult<PathBuf> {
    checked_file_in(path, &library_dir("Messages")?)
}

/// A file under ~/Library/Messages/Attachments: the only place trashing is allowed.
pub fn checked_attachment(path: &str) -> CmdResult<PathBuf> {
    checked_file_in(path, &library_dir("Messages/Attachments")?)
}

/// Types that run code or redirect when opened (a `.command` someone texted
/// you runs a script). These are revealed in Finder instead of opened.
const REVEAL_ONLY: &[&str] = &[
    "app",
    "command",
    "terminal",
    "tool",
    "sh",
    "zsh",
    "bash",
    "pkg",
    "mpkg",
    "fileloc",
    "inetloc",
    "webloc",
    "url",
    "workflow",
    "action",
    "scpt",
    "scptd",
    "applescript",
    "jar",
    "prefpane",
    "kext",
    "bundle",
    "plugin",
    "osax",
    "definition",
    "dylib",
    "shortcut",
    "mobileconfig",
    "dmg",
    "iso",
    "img",
];

/// Whether `path` should be revealed rather than opened.
pub fn reveal_only(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| REVEAL_ONLY.contains(&e.to_ascii_lowercase().as_str()))
}

/// Copy `src` into ~/Downloads without overwriting ("name 2.ext" on clash).
/// Returns the new path.
pub fn save_to_downloads(src: &Path, name: Option<&str>) -> CmdResult<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| CmdError::internal("HOME is not set"))?;
    let dir = PathBuf::from(home).join("Downloads");
    let mut input = std::fs::File::open(src)?;
    for n in 1..1000 {
        let dest = candidate(&dir, src, name, n);
        // create_new reserves the name atomically: no check-then-copy race,
        // and never writes through a symlink planted at the destination.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
        {
            Ok(mut out) => {
                std::io::copy(&mut input, &mut out)?;
                return Ok(dest);
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Err(CmdError::internal(
        "too many files with that name in Downloads",
    ))
}

/// The `n`th candidate path in `dir` for `name` (or `src`'s name): "x.jpg",
/// "x 2.jpg", ... The name is reduced to a plain file name without control
/// or bidi-override characters (U+202E can fake an extension).
fn candidate(dir: &Path, src: &Path, name: Option<&str>, n: u32) -> PathBuf {
    let raw = name
        .map(str::to_string)
        .or_else(|| src.file_name().map(|n| n.to_string_lossy().into_owned()))
        .unwrap_or_default();
    let base = Path::new(&raw)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let clean: String = base
        .chars()
        .filter(|c| !c.is_control() && !matches!(c, '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}' | '\u{200E}' | '\u{200F}'))
        .collect();
    let clean = if clean.trim().is_empty() || clean.starts_with('.') {
        format!("attachment{clean}")
    } else {
        clean
    };
    if n == 1 {
        return dir.join(clean);
    }
    match clean.rsplit_once('.') {
        Some((s, e)) if !s.is_empty() => dir.join(format!("{s} {n}.{e}")),
        _ => dir.join(format!("{clean} {n}")),
    }
}

/// Put the file on the clipboard (as a file: pastes into Messages, Mail,
/// Slack, Finder). The image isn't decoded here, so attacker-sent media is
/// never parsed in this process.
#[cfg(target_os = "macos")]
pub fn copy_to_clipboard(path: &Path) -> CmdResult<()> {
    use objc2::rc::Retained;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSPasteboard, NSPasteboardWriting};
    use objc2_foundation::{NSArray, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let pasteboard = NSPasteboard::generalPasteboard();
    pasteboard.clearContents();
    let item: Retained<ProtocolObject<dyn NSPasteboardWriting>> =
        ProtocolObject::from_retained(url);
    if pasteboard.writeObjects(&NSArray::from_retained_slice(&[item])) {
        Ok(())
    } else {
        Err(CmdError::internal("couldn't write to the clipboard"))
    }
}

/// Open a file in its default app (Launch Services; no shell, no argv).
#[cfg(target_os = "macos")]
pub fn open_file(path: &Path) -> CmdResult<()> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    if NSWorkspace::sharedWorkspace().openURL(&url) {
        Ok(())
    } else {
        Err(CmdError::internal("no app could open this file"))
    }
}

/// Select the file in a Finder window.
#[cfg(target_os = "macos")]
pub fn reveal(path: &Path) -> CmdResult<()> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSArray, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    NSWorkspace::sharedWorkspace()
        .activateFileViewerSelectingURLs(&NSArray::from_retained_slice(&[url]));
    Ok(())
}

/// Open a URL we built (System Settings panes, `sms:`) via Launch Services.
#[cfg(target_os = "macos")]
pub fn open_url(url: &str) -> CmdResult<()> {
    use objc2_app_kit::NSWorkspace;
    use objc2_foundation::{NSString, NSURL};

    let u = NSURL::URLWithString(&NSString::from_str(url))
        .ok_or_else(|| CmdError::invalid("bad URL"))?;
    if NSWorkspace::sharedWorkspace().openURL(&u) {
        Ok(())
    } else {
        Err(CmdError::internal(format!("couldn't open {url}")))
    }
}

/// Move a file to the Trash (restorable from Finder). Returns where it went
/// (the Trash renames on name clashes).
#[cfg(target_os = "macos")]
pub fn move_to_trash(path: &Path) -> CmdResult<PathBuf> {
    use objc2::rc::Retained;
    use objc2_foundation::{NSFileManager, NSString, NSURL};

    let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
    let mut resulting: Option<Retained<NSURL>> = None;
    NSFileManager::defaultManager()
        .trashItemAtURL_resultingItemURL_error(&url, Some(&mut resulting))
        .map_err(|e| CmdError::internal(e.localizedDescription().to_string()))?;
    Ok(resulting
        .and_then(|u| u.path())
        .map_or_else(PathBuf::new, |p| PathBuf::from(p.to_string())))
}

#[cfg(not(target_os = "macos"))]
mod unsupported {
    use super::*;
    pub fn copy_to_clipboard(_: &Path) -> CmdResult<()> {
        Err(CmdError::invalid("macOS only"))
    }
    pub fn open_file(_: &Path) -> CmdResult<()> {
        Err(CmdError::invalid("macOS only"))
    }
    pub fn reveal(_: &Path) -> CmdResult<()> {
        Err(CmdError::invalid("macOS only"))
    }
    pub fn open_url(_: &str) -> CmdResult<()> {
        Err(CmdError::invalid("macOS only"))
    }
    pub fn move_to_trash(_: &Path) -> CmdResult<PathBuf> {
        Err(CmdError::invalid("macOS only"))
    }
}
#[cfg(not(target_os = "macos"))]
pub use unsupported::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn download_names() {
        let dir = Path::new("/d");
        let src = Path::new("/x/IMG_1.HEIC");
        assert_eq!(candidate(dir, src, None, 1), dir.join("IMG_1.HEIC"));
        assert_eq!(candidate(dir, src, None, 2), dir.join("IMG_1 2.HEIC"));
        // Stored names can't escape the folder or hide their extension.
        assert_eq!(
            candidate(dir, src, Some("../../etc/passwd"), 1),
            dir.join("passwd")
        );
        assert_eq!(
            candidate(dir, src, Some("photo\u{202E}gpj.exe"), 1),
            dir.join("photogpj.exe")
        );
        assert_eq!(
            candidate(dir, src, Some(".hidden"), 1),
            dir.join("attachment.hidden")
        );
    }

    #[test]
    fn save_never_overwrites() {
        let tmp = std::env::temp_dir().join(format!("ms-files-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let src = tmp.join("a.txt");
        std::fs::write(&src, b"one").unwrap();
        let dest = candidate(&tmp, &src, Some("out.txt"), 1);
        std::fs::write(&dest, b"keep").unwrap();
        assert!(std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&dest)
            .is_err());
        assert_eq!(std::fs::read(&dest).unwrap(), b"keep");
        std::fs::remove_dir_all(&tmp).unwrap();
    }

    #[test]
    fn reveal_only_types() {
        assert!(reveal_only(Path::new("/a/run.command")));
        assert!(reveal_only(Path::new("/a/Setup.PKG")));
        assert!(!reveal_only(Path::new("/a/IMG_1.HEIC")));
        assert!(!reveal_only(Path::new("/a/doc.pdf")));
    }
}
