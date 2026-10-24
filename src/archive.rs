//! Automatic extraction of downloaded archives (ZIP, RAR, 7z).
//!
//! A finished download is often a container rather than the file the user
//! wants. This module unpacks it, with the safety rules an automatic action
//! needs:
//!
//! * **Nothing escapes the destination folder.** Entry names are validated
//!   before anything is written: absolute paths, drive letters, UNC paths and
//!   `..` components are refused, and the joined path is checked again.
//! * **Nothing runs.** Extracted files are only written; they are never
//!   started, and no executable is ever launched.
//! * **A bomb cannot fill the disk.** The number of entries, the size of a
//!   single entry, the total size and the compression ratio all have limits.
//! * **Existing files are kept.** An entry that is already there is skipped
//!   unless overwriting was asked for.
//! * **The password is never written down.** It is used for this one
//!   extraction and then dropped; it is never stored in the queue, the
//!   settings or the log.
//!
//! The three formats are handled by maintained libraries: `zip` (MIT),
//! `sevenz-rust` (Apache-2.0) and `unrar` (the reference UnRAR reader,
//! MIT/Apache-2.0 wrapper).

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// The archive formats this app can open.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArchiveKind {
    Zip,
    Rar,
    SevenZ,
}

impl ArchiveKind {
    pub fn label(&self) -> &'static str {
        match self {
            ArchiveKind::Zip => "ZIP",
            ArchiveKind::Rar => "RAR",
            ArchiveKind::SevenZ => "7Z",
        }
    }
}

/// What went wrong.
#[derive(Debug)]
pub enum ExtractError {
    /// The archive is encrypted and the password was missing or wrong.
    NeedsPassword,
    /// The archive could not be read at all.
    BadArchive(String),
    /// The archive broke one of the safety limits.
    Unsafe(String),
    /// The format is known but this build cannot open it.
    Unsupported(String),
    /// The filesystem said no.
    Io(String),
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::NeedsPassword => write!(f, "the archive is password protected"),
            ExtractError::BadArchive(m) => write!(f, "the archive could not be read: {m}"),
            ExtractError::Unsafe(m) => write!(f, "the archive was refused: {m}"),
            ExtractError::Unsupported(m) => write!(f, "{m}"),
            ExtractError::Io(m) => write!(f, "{m}"),
        }
    }
}

/// How an extraction may behave.
#[derive(Debug, Clone)]
pub struct ExtractOptions {
    /// Where the files go.
    pub dest: PathBuf,
    /// Password for an encrypted archive. Held in memory for this extraction
    /// only; it is never written to disk.
    pub password: Option<String>,
    /// Replace files that already exist.
    pub overwrite: bool,
    /// Refuse to write more than this many bytes in total.
    pub max_total_bytes: u64,
    /// Refuse a single entry larger than this.
    pub max_entry_bytes: u64,
    /// Refuse more entries than this.
    pub max_entries: usize,
    /// Refuse an archive that expands more than this many times.
    pub max_ratio: u64,
}

impl Default for ExtractOptions {
    fn default() -> Self {
        ExtractOptions {
            dest: PathBuf::new(),
            password: None,
            overwrite: false,
            // 20 GiB of extracted data, 8 GiB for a single file, 100k
            // entries and a 1000:1 expansion. Generous for real archives,
            // useless for a bomb.
            max_total_bytes: 20 * 1024 * 1024 * 1024,
            max_entry_bytes: 8 * 1024 * 1024 * 1024,
            max_entries: 100_000,
            max_ratio: 1000,
        }
    }
}

/// What an extraction produced.
#[derive(Debug, Clone, Default)]
pub struct ExtractOutcome {
    /// Files written.
    pub files: usize,
    /// Folders created.
    pub dirs: usize,
    /// Bytes written.
    pub bytes: u64,
    /// Entries left alone because the file was already there.
    pub skipped: usize,
    /// Entries refused because their name tried to leave the folder.
    pub refused: usize,
    /// The extraction was stopped by the caller.
    pub cancelled: bool,
}

/// Progress of a running extraction.
#[derive(Debug, Clone, Copy)]
pub struct ExtractProgress {
    /// Bytes written so far.
    pub done: u64,
    /// Bytes expected in total (0 when the archive does not say).
    pub total: u64,
}

/// Which volume of a multipart set a file name is, and the name the volume
/// carries when the volume marker is taken off ("pack.part1.rar" -> "pack.rar").
fn split_volume(name: &str) -> Option<(u32, String)> {
    // "pack.zip.001", "pack.7z.001" — the number is the last piece.
    if let Some((base, tail)) = name.rsplit_once('.') {
        if let Ok(n) = tail.parse::<u32>() {
            if (1..=9999).contains(&n) && !base.is_empty() {
                return Some((n, base.to_string()));
            }
        }
    }
    // "pack.part1.rar" — the number sits in the middle.
    let lower = name.to_ascii_lowercase();
    if let Some(idx) = lower.find(".part") {
        let rest = &lower[idx + 5..];
        let digits: String = rest.chars().take_while(|c| c.is_ascii_digit()).collect();
        if !digits.is_empty() {
            let n: u32 = digits.parse().unwrap_or(1);
            let tail = &name[idx + 5 + digits.len()..];
            let base = format!("{}{}", &name[..idx], tail);
            if !base.is_empty() {
                return Some((n, base));
            }
        }
    }
    None
}

/// Does this file name look like an archive this app can open?
pub fn detect_kind(path: &Path) -> Option<ArchiveKind> {
    let name = path.file_name()?.to_string_lossy().to_ascii_lowercase();
    // A multipart volume is judged by the name it carries ("pack.zip.001").
    let base = match split_volume(&name) {
        Some((_, base)) => base,
        None => name.clone(),
    };
    if base.ends_with(".zip") {
        Some(ArchiveKind::Zip)
    } else if base.ends_with(".rar") {
        Some(ArchiveKind::Rar)
    } else if base.ends_with(".7z") {
        Some(ArchiveKind::SevenZ)
    } else {
        None
    }
}

/// The volumes of a multipart set, in order, starting at the given file.
fn sibling_volumes(first: &Path) -> Vec<PathBuf> {
    let Some(name) = first.file_name().map(|n| n.to_string_lossy().to_string()) else {
        return Vec::new();
    };
    let Some((_, base)) = split_volume(&name) else {
        return Vec::new();
    };
    let dir = first.parent().unwrap_or_else(|| Path::new("."));
    let mut out = Vec::new();
    for n in 1..=9999u32 {
        let candidate = dir.join(format!("{base}.{n:03}"));
        if !candidate.is_file() {
            break;
        }
        out.push(candidate);
    }
    if out.is_empty() {
        out.push(first.to_path_buf());
    }
    out
}

/// Turn an entry name into a path that is guaranteed to stay inside the
/// destination folder, or refuse it.
///
/// Refused: absolute paths, drive letters, UNC paths, `..`, empty names and
/// names Windows would not accept anyway.
pub fn safe_relative(name: &str) -> Option<PathBuf> {
    let cleaned = name.replace('\\', "/");
    if cleaned.trim().is_empty() {
        return None;
    }
    if cleaned.starts_with('/') {
        return None;
    }
    let bytes = cleaned.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' && (bytes[0] as char).is_ascii_alphabetic() {
        return None;
    }
    if cleaned.contains('\0') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in cleaned.split('/') {
        match part {
            "" | "." => continue,
            ".." => return None,
            p => {
                if p.chars().any(|c| c.is_control()) {
                    return None;
                }
                // Windows refuses these names outright; writing them would
                // produce a file the user can never open.
                let stem = p.split('.').next().unwrap_or("").to_ascii_uppercase();
                let reserved = matches!(
                    stem.as_str(),
                    "CON" | "PRN" | "AUX" | "NUL" | "COM1" | "COM2" | "COM3" | "COM4" | "COM5"
                        | "COM6" | "COM7" | "COM8" | "COM9" | "LPT1" | "LPT2" | "LPT3" | "LPT4"
                        | "LPT5" | "LPT6" | "LPT7" | "LPT8" | "LPT9"
                );
                if reserved || p.ends_with(' ') || p.ends_with('.') {
                    return None;
                }
                out.push(p);
            }
        }
    }
    if out.as_os_str().is_empty() {
        None
    } else {
        Some(out)
    }
}

/// Join a validated relative path onto the destination and check the result
/// once more. Belt and braces: the name was already validated, and this
/// catches anything the platform does to the path.
fn target_path(dest: &Path, rel: &Path) -> Option<PathBuf> {
    // A relative path that still holds a ".." (or a root) can leave the
    // destination, whatever the caller thought it validated.
    if rel.is_absolute() {
        return None;
    }
    if rel
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir | std::path::Component::RootDir))
    {
        return None;
    }
    // Lexical containment alone follows an existing directory junction or
    // symlink out of the extraction folder. Reject reparse points in every
    // component, including a dangling link or the destination file itself.
    let mut checked = dest.to_path_buf();
    for component in rel.components() {
        checked.push(component);
        match std::fs::symlink_metadata(&checked) {
            Ok(meta) => {
                if meta.file_type().is_symlink() { return None; }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 { return None; }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return None,
        }
    }
    let joined = dest.join(rel);
    if !joined.starts_with(dest) {
        return None;
    }
    Some(joined)
}

/// A source the ZIP reader can both read and seek in.
///
/// A plain file and the concatenation of a split archive both qualify.
trait ReadSeek: Read + Seek {}
impl<T: Read + Seek> ReadSeek for T {}

/// A split ZIP (`pack.zip.001`, `pack.zip.002`, ...) is one byte range after
/// another; reading them as one stream restores the original archive without
/// writing a second copy to disk.
struct ConcatReader {
    parts: Vec<std::fs::File>,
    /// Length of each part.
    sizes: Vec<u64>,
    /// Index of the part the cursor is in.
    part: usize,
    /// Offset inside that part.
    offset: u64,
}

impl ConcatReader {
    fn open(parts: Vec<PathBuf>) -> std::io::Result<Self> {
        let mut files = Vec::new();
        let mut sizes = Vec::new();
        for p in &parts {
            let f = std::fs::File::open(p)?;
            sizes.push(f.metadata()?.len());
            files.push(f);
        }
        Ok(ConcatReader {
            parts: files,
            sizes,
            part: 0,
            offset: 0,
        })
    }

    fn total(&self) -> u64 {
        self.sizes.iter().sum()
    }
}

impl Read for ConcatReader {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        if self.part >= self.parts.len() {
            return Ok(0);
        }
        let size = self.sizes[self.part];
        if self.offset >= size {
            self.part += 1;
            self.offset = 0;
            return self.read(buf);
        }
        let file = &mut self.parts[self.part];
        file.seek(SeekFrom::Start(self.offset))?;
        let want = buf.len().min((size - self.offset) as usize);
        let n = file.read(&mut buf[..want])?;
        self.offset += n as u64;
        if n == 0 {
            self.part += 1;
            self.offset = 0;
        }
        Ok(n)
    }
}

impl Seek for ConcatReader {
    fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64> {
        let total = self.total();
        let target = match pos {
            SeekFrom::Start(p) => p as i64,
            SeekFrom::End(p) => total as i64 + p,
            SeekFrom::Current(p) => (self.offset_in_stream() as i64) + p,
        };
        if target < 0 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "negative seek",
            ));
        }
        let target = target as u64;
        let mut remaining = target;
        for (i, size) in self.sizes.iter().enumerate() {
            if remaining < *size {
                self.part = i;
                self.offset = remaining;
                return Ok(target);
            }
            remaining -= size;
        }
        self.part = self.sizes.len();
        self.offset = 0;
        Ok(target)
    }
}

impl ConcatReader {
    fn offset_in_stream(&self) -> u64 {
        self.sizes[..self.part].iter().sum::<u64>() + self.offset
    }
}

/// Open an archive and unpack it.
pub fn extract(
    archive: &Path,
    opts: &ExtractOptions,
    progress: &mut dyn FnMut(ExtractProgress) -> bool,
) -> Result<ExtractOutcome, ExtractError> {
    let kind = detect_kind(archive)
        .ok_or_else(|| ExtractError::BadArchive("not a supported archive".into()))?;
    let dest = opts.dest.clone();
    std::fs::create_dir_all(&dest).map_err(|e| ExtractError::Io(e.to_string()))?;
    // Work against a canonical destination so the "stays inside" check below
    // compares like with like.
    let dest = dest
        .canonicalize()
        .unwrap_or(dest);
    let opts = ExtractOptions {
        dest: dest.clone(),
        ..opts.clone()
    };
    match kind {
        ArchiveKind::Zip => extract_zip(archive, &opts, progress),
        ArchiveKind::Rar => extract_rar(archive, &opts, progress),
        ArchiveKind::SevenZ => extract_7z(archive, &opts, progress),
    }
}

/// Write one entry to disk, honouring the "keep what is already there" rule.
fn write_entry(
    dest: &Path,
    rel: &Path,
    reader: &mut dyn Read,
    overwrite: bool,
) -> Result<Option<u64>, ExtractError> {
    let Some(target) = target_path(dest, rel) else {
        return Err(ExtractError::Unsafe(format!(
            "{} would land outside the destination",
            rel.display()
        )));
    };
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent).map_err(|e| ExtractError::Io(e.to_string()))?;
    }
    if target.exists() && !overwrite {
        // Read the data anyway: the stream has to be consumed for the next
        // entry to line up.
        std::io::copy(reader, &mut std::io::sink()).map_err(|e| ExtractError::Io(e.to_string()))?;
        return Ok(None);
    }
    let mut file = std::fs::File::create(&target).map_err(|e| ExtractError::Io(e.to_string()))?;
    let written = std::io::copy(reader, &mut file).map_err(|e| ExtractError::Io(e.to_string()))?;
    file.flush().ok();
    Ok(Some(written))
}

// ---------------------------------------------------------------------------
// ZIP
// ---------------------------------------------------------------------------

fn extract_zip(
    archive: &Path,
    opts: &ExtractOptions,
    progress: &mut dyn FnMut(ExtractProgress) -> bool,
) -> Result<ExtractOutcome, ExtractError> {
    // A split archive is several files that make up one stream.
    let volumes = sibling_volumes(archive);
    let source_size: u64 = volumes
        .iter()
        .filter_map(|p| std::fs::metadata(p).ok())
        .map(|m| m.len())
        .sum();
    let reader: Box<dyn ReadSeek> = if volumes.len() > 1 {
        Box::new(ConcatReader::open(volumes).map_err(|e| ExtractError::Io(e.to_string()))?)
    } else {
        Box::new(
            std::fs::File::open(archive).map_err(|e| ExtractError::Io(e.to_string()))?,
        )
    };
    let mut zip = zip::ZipArchive::new(reader)
        .map_err(|e| ExtractError::BadArchive(e.to_string()))?;
    if zip.len() > opts.max_entries {
        return Err(ExtractError::Unsafe(format!(
            "{} entries is more than the {} allowed",
            zip.len(),
            opts.max_entries
        )));
    }
    // The declared sizes decide whether this archive is a bomb.
    let mut declared: u64 = 0;
    for i in 0..zip.len() {
        let e = zip
            .by_index(i)
            .map_err(|e| ExtractError::BadArchive(e.to_string()))?;
        declared = declared.saturating_add(e.size());
        if declared > opts.max_total_bytes {
            return Err(ExtractError::Unsafe(format!(
                "the archive expands to more than the {} allowed",
                crate::model::format_bytes(opts.max_total_bytes)
            )));
        }
    }
    if source_size > 0 && declared / source_size.max(1) > opts.max_ratio {
        return Err(ExtractError::Unsafe(format!(
            "the archive expands {} times over — that is a bomb, not a download",
            declared / source_size.max(1)
        )));
    }
    let password = opts.password.clone().unwrap_or_default();
    let mut outcome = ExtractOutcome::default();
    for i in 0..zip.len() {
        let mut entry = zip
            .by_index_decrypt(i, password.as_bytes())
            .map_err(|e| map_zip_error(e))?;
        // A name the library cannot decode is refused rather than guessed.
        let raw_name = match entry.name() {
            Ok(n) => n.to_string(),
            Err(_) => {
                outcome.refused += 1;
                continue;
            }
        };
        let is_dir = entry.is_dir();
        let size = entry.size();
        if size > opts.max_entry_bytes {
            outcome.refused += 1;
            continue;
        }
        if entry.encrypted() && password.is_empty() {
            return Err(ExtractError::NeedsPassword);
        }
        let Some(rel) = safe_relative(&raw_name) else {
            outcome.refused += 1;
            continue;
        };
        if is_dir {
            let Some(target) = target_path(&opts.dest, &rel) else {
                outcome.refused += 1;
                continue;
            };
            std::fs::create_dir_all(&target).map_err(|e| ExtractError::Io(e.to_string()))?;
            outcome.dirs += 1;
            continue;
        }
        match write_entry(&opts.dest, &rel, &mut entry, opts.overwrite) {
            Ok(Some(n)) => {
                outcome.files += 1;
                outcome.bytes += n;
            }
            Ok(None) => outcome.skipped += 1,
            Err(e) => return Err(e),
        }
        if !progress(ExtractProgress {
            done: outcome.bytes,
            total: declared,
        }) {
            outcome.cancelled = true;
            return Ok(outcome);
        }
    }
    Ok(outcome)
}

/// Turn a zip error into something the user can act on.
fn map_zip_error(e: zip::result::ZipError) -> ExtractError {
    let text = e.to_string();
    if text.to_ascii_lowercase().contains("password") {
        ExtractError::NeedsPassword
    } else {
        ExtractError::BadArchive(text)
    }
}

// ---------------------------------------------------------------------------
// RAR
// ---------------------------------------------------------------------------

fn extract_rar(
    archive: &Path,
    opts: &ExtractOptions,
    progress: &mut dyn FnMut(ExtractProgress) -> bool,
) -> Result<ExtractOutcome, ExtractError> {
    use unrar::Archive;

    // A multipart set must be opened at its first volume.
    let mut arch = Archive::new(&archive);
    if let Some(password) = opts.password.as_deref().filter(|p| !p.is_empty()) {
        arch = Archive::with_password(&archive, password);
    }
    let arch = arch.as_first_part();
    let mut open = arch
        .open_for_processing()
        .map_err(|e| ExtractError::BadArchive(e.to_string()))?;
    let mut outcome = ExtractOutcome::default();
    let mut declared: u64 = 0;
    let mut entries = 0usize;
    loop {
        let header = match open.read_header() {
            Ok(Some(h)) => h,
            Ok(None) => break,
            Err(e) => {
                let text = e.to_string();
                if text.contains("password") {
                    return Err(ExtractError::NeedsPassword);
                }
                return Err(ExtractError::BadArchive(text));
            }
        };
        entries += 1;
        if entries > opts.max_entries {
            return Err(ExtractError::Unsafe(format!(
                "more than the {} entries allowed",
                opts.max_entries
            )));
        }
        let filename = header.entry().filename.to_string_lossy().to_string();
        let size = header.entry().unpacked_size;
        let is_dir = header.entry().is_directory();
        let encrypted = header.entry().is_encrypted();
        declared = declared.saturating_add(size);
        if declared > opts.max_total_bytes {
            return Err(ExtractError::Unsafe(format!(
                "the archive expands to more than the {} allowed",
                crate::model::format_bytes(opts.max_total_bytes)
            )));
        }
        if encrypted && opts.password.is_none() {
            return Err(ExtractError::NeedsPassword);
        }
        let Some(rel) = safe_relative(&filename) else {
            outcome.refused += 1;
            open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
            continue;
        };
        if is_dir {
            let Some(target) = target_path(&opts.dest, &rel) else {
                outcome.refused += 1;
                open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
                continue;
            };
            std::fs::create_dir_all(&target).map_err(|e| ExtractError::Io(e.to_string()))?;
            outcome.dirs += 1;
            open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
            continue;
        }
        if size > opts.max_entry_bytes {
            outcome.refused += 1;
            open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
            continue;
        }
        let Some(target) = target_path(&opts.dest, &rel) else {
            outcome.refused += 1;
            open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
            continue;
        };
        if target.exists() && !opts.overwrite {
            outcome.skipped += 1;
            open = header.skip().map_err(|e| ExtractError::BadArchive(e.to_string()))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| ExtractError::Io(e.to_string()))?;
        }
        // `extract_to` writes the entry to exactly this path, which is one we
        // have already proved to be inside the destination.
        open = match header.extract_to(&target) {
            Ok(next) => next,
            Err(e) => {
                let text = e.to_string();
                if text.to_ascii_lowercase().contains("password") {
                    return Err(ExtractError::NeedsPassword);
                }
                return Err(ExtractError::BadArchive(text));
            }
        };
        outcome.files += 1;
        outcome.bytes += size;
        if !progress(ExtractProgress {
            done: outcome.bytes,
            total: declared,
        }) {
            outcome.cancelled = true;
            return Ok(outcome);
        }
    }
    Ok(outcome)
}

// ---------------------------------------------------------------------------
// 7Z
// ---------------------------------------------------------------------------

fn extract_7z(
    archive: &Path,
    opts: &ExtractOptions,
    progress: &mut dyn FnMut(ExtractProgress) -> bool,
) -> Result<ExtractOutcome, ExtractError> {
    use sevenz_rust::Password;
    use std::cell::Cell;

    // A 7z set that was split into volumes cannot be read back by the
    // library; say so instead of failing halfway through.
    if sibling_volumes(archive).len() > 1 {
        return Err(ExtractError::Unsupported(
            "split 7z volumes are not supported — extract the first volume with 7-Zip"
                .into(),
        ));
    }
    let file = std::fs::File::open(archive).map_err(|e| ExtractError::Io(e.to_string()))?;
    let password = match opts.password.as_deref() {
        Some(p) if !p.is_empty() => Password::from(p),
        _ => Password::empty(),
    };
    let dest = opts.dest.clone();
    let outcome = Cell::new(ExtractOutcome::default());
    let total = Cell::new(0u64);
    let overwrite = opts.overwrite;
    let max_entry_bytes = opts.max_entry_bytes;

    let result = sevenz_rust::decompress_with_extract_fn_and_password(
        file,
        &dest,
        password,
        |entry, reader, _library_target| {
            let mut out = outcome.take();
            let name = entry.name().to_string();
            let size = entry.size();
            total.set(total.get().saturating_add(size));
            if size > max_entry_bytes {
                out.refused += 1;
                std::io::copy(reader, &mut std::io::sink()).ok();
                outcome.set(out);
                return Ok(true);
            }
            let Some(rel) = safe_relative(&name) else {
                out.refused += 1;
                std::io::copy(reader, &mut std::io::sink()).ok();
                outcome.set(out);
                return Ok(true);
            };
            if entry.is_directory() {
                if let Some(target) = target_path(&dest, &rel) {
                    std::fs::create_dir_all(&target).ok();
                    out.dirs += 1;
                } else {
                    out.refused += 1;
                }
                outcome.set(out);
                return Ok(true);
            }
            match write_entry(&dest, &rel, reader, overwrite) {
                Ok(Some(n)) => {
                    out.files += 1;
                    out.bytes += n;
                }
                Ok(None) => out.skipped += 1,
                Err(ExtractError::Io(m)) => {
                    outcome.set(out);
                    return Err(sevenz_rust::Error::io(std::io::Error::other(m)));
                }
                Err(e) => {
                    outcome.set(out);
                    return Err(sevenz_rust::Error::io(std::io::Error::other(e.to_string())));
                }
            }
            outcome.set(out);
            Ok(true)
        },
    );
    let mut outcome = outcome.take();
    match result {
        Ok(()) => {}
        Err(e) => {
            let text = e.to_string();
            if text.to_ascii_lowercase().contains("password") {
                return Err(ExtractError::NeedsPassword);
            }
            return Err(ExtractError::BadArchive(text));
        }
    }
    if !progress(ExtractProgress {
        done: outcome.bytes,
        total: total.get(),
    }) {
        outcome.cancelled = true;
    }
    Ok(outcome)
}
#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("fixtures")
            .join(name)
    }

    /// A fresh folder for one extraction.
    fn scratch(tag: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "dc-archive-{}-{}-{:?}",
            std::process::id(),
            tag,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn options(dest: &Path) -> ExtractOptions {
        ExtractOptions {
            dest: dest.to_path_buf(),
            ..Default::default()
        }
    }

    #[test]
    fn a_zip_fixture_is_unpacked() {
        let dest = scratch("zip");
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(&fixture("sample.zip"), &options(&dest), &mut progress)
            .expect("the zip must be readable");
        assert_eq!(out.files, 3, "three files are in the fixture");
        assert!(out.bytes > 0);
        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "hello from the zip fixture\n"
        );
        assert_eq!(
            std::fs::read_to_string(dest.join("folder/nested.txt")).unwrap(),
            "nested file\n"
        );
        assert!(dest.join("folder/deeper/leaf.txt").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_zip_that_tries_to_escape_is_refused() {
        let dest = scratch("traversal");
        let parent = dest.parent().unwrap().to_path_buf();
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(&fixture("traversal.zip"), &options(&dest), &mut progress)
            .expect("the zip must be readable");
        assert_eq!(out.refused, 2, "both escaping names are refused");
        assert_eq!(out.files, 1, "only the safe entry is written");
        assert!(dest.join("safe.txt").is_file());
        // Nothing landed next to the destination folder.
        assert!(!parent.join("escape.txt").exists());
        assert!(!parent.join("escape2.txt").exists());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_windows_device_name_is_refused() {
        let dest = scratch("reserved");
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(
            &fixture("reserved-names.zip"),
            &options(&dest),
            &mut progress,
        )
        .expect("the zip must be readable");
        assert_eq!(out.refused, 1);
        assert!(dest.join("ok.txt").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_seven_zip_fixture_is_unpacked() {
        let dest = scratch("7z");
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(&fixture("sample.7z"), &options(&dest), &mut progress)
            .expect("the 7z must be readable");
        assert_eq!(out.files, 2);
        assert!(dest.join("hello.txt").is_file());
        assert!(dest.join("folder/nested.txt").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_rar4_fixture_is_unpacked() {
        let dest = scratch("rar4");
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(
            &fixture("rar4-best-4096kb-txt.rar"),
            &options(&dest),
            &mut progress,
        )
        .expect("the rar must be readable");
        assert_eq!(out.files, 1);
        assert!(dest.join("text.txt").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_rar5_fixture_is_unpacked() {
        let dest = scratch("rar5");
        let mut progress = |_p: ExtractProgress| true;
        let out = extract(
            &fixture("rar5-best-32mb-txt.rar"),
            &options(&dest),
            &mut progress,
        )
        .expect("the rar must be readable");
        assert_eq!(out.files, 1);
        assert!(dest.join("text.txt").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn an_encrypted_archive_asks_for_its_password() {
        let dest = scratch("rarpw");
        let mut progress = |_p: ExtractProgress| true;
        let err = extract(
            &fixture("rar5-normal-32mb-txt-png-pw-test.rar"),
            &options(&dest),
            &mut progress,
        )
        .expect_err("the password is missing");
        assert!(
            matches!(err, ExtractError::NeedsPassword),
            "expected a password request, got {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn the_right_password_opens_an_encrypted_archive() {
        let dest = scratch("rarpw-ok");
        let mut progress = |_p: ExtractProgress| true;
        let mut opts = options(&dest);
        opts.password = Some("test".to_string());
        let out = extract(
            &fixture("rar5-normal-32mb-txt-png-pw-test.rar"),
            &opts,
            &mut progress,
        )
        .expect("the password is right");
        assert_eq!(out.files, 2);
        assert!(dest.join("text.txt").is_file());
        assert!(dest.join("photo.jpg").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_multipart_rar_is_unpacked_from_its_first_part() {
        let dest = scratch("rarmulti");
        let mut progress = |_p: ExtractProgress| true;
        let mut opts = options(&dest);
        opts.password = Some("test".to_string());
        let out = extract(
            &fixture("rar5-normal-32mb-txt-png-1mb-pw-test.part1.rar"),
            &opts,
            &mut progress,
        )
        .expect("the rar set must be readable");
        assert_eq!(out.files, 2, "both parts of the set are unpacked");
        assert!(dest.join("text.txt").is_file());
        assert!(dest.join("photo.jpg").is_file());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_file_that_is_already_there_is_kept() {
        let dest = scratch("keep");
        let mut progress = |_p: ExtractProgress| true;
        extract(&fixture("sample.zip"), &options(&dest), &mut progress).unwrap();
        // Change a file and unpack again: the change must survive.
        std::fs::write(dest.join("hello.txt"), b"mine now\n").unwrap();
        let out = extract(&fixture("sample.zip"), &options(&dest), &mut progress).unwrap();
        assert_eq!(out.skipped, 3, "every file was already there");
        assert_eq!(out.files, 0);
        assert_eq!(
            std::fs::read_to_string(dest.join("hello.txt")).unwrap(),
            "mine now\n"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn an_archive_that_expands_too_far_is_refused() {
        let dest = scratch("bomb");
        let mut progress = |_p: ExtractProgress| true;
        let mut opts = options(&dest);
        // 10 bytes of allowed output: the fixture holds more than that.
        opts.max_total_bytes = 10;
        let err = extract(&fixture("sample.zip"), &opts, &mut progress)
            .expect_err("the archive must be refused");
        assert!(
            matches!(err, ExtractError::Unsafe(_)),
            "expected a safety refusal, got {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn a_single_entry_that_is_too_big_is_refused() {
        let dest = scratch("bomb-entry");
        let mut progress = |_p: ExtractProgress| true;
        let mut opts = options(&dest);
        opts.max_entry_bytes = 4;
        let out = extract(&fixture("sample.zip"), &opts, &mut progress)
            .expect("the archive is still readable");
        assert_eq!(out.refused, 3, "every entry is over the limit");
        assert_eq!(out.files, 0, "nothing is written");
        assert!(!dest.join("hello.txt").exists());
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn too_many_entries_are_refused() {
        let dest = scratch("bomb-count");
        let mut progress = |_p: ExtractProgress| true;
        let mut opts = options(&dest);
        opts.max_entries = 1;
        let err = extract(&fixture("sample.zip"), &opts, &mut progress)
            .expect_err("the archive must be refused");
        assert!(
            matches!(err, ExtractError::Unsafe(_)),
            "expected a safety refusal, got {err:?}"
        );
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn an_extraction_can_be_stopped() {
        let dest = scratch("cancel");
        let mut progress = |_p: ExtractProgress| false;
        let out = extract(&fixture("sample.zip"), &options(&dest), &mut progress).unwrap();
        assert!(out.cancelled);
        let _ = std::fs::remove_dir_all(&dest);
    }

    #[test]
    fn archive_names_are_recognised() {
        assert_eq!(
            detect_kind(Path::new("pack.zip")),
            Some(ArchiveKind::Zip)
        );
        assert_eq!(
            detect_kind(Path::new("pack.ZIP")),
            Some(ArchiveKind::Zip)
        );
        assert_eq!(
            detect_kind(Path::new("pack.rar")),
            Some(ArchiveKind::Rar)
        );
        assert_eq!(
            detect_kind(Path::new("pack.7z")),
            Some(ArchiveKind::SevenZ)
        );
        assert_eq!(detect_kind(Path::new("movie.mp4")), None);
        assert_eq!(detect_kind(Path::new("notes.txt")), None);
    }

    #[test]
    fn multipart_names_are_recognised_by_their_base_name() {
        assert_eq!(
            detect_kind(Path::new("pack.zip.001")),
            Some(ArchiveKind::Zip)
        );
        assert_eq!(
            detect_kind(Path::new("pack.7z.001")),
            Some(ArchiveKind::SevenZ)
        );
        assert_eq!(
            detect_kind(Path::new("pack.part1.rar")),
            Some(ArchiveKind::Rar)
        );
        assert_eq!(
            detect_kind(Path::new("pack.part2.rar")),
            Some(ArchiveKind::Rar)
        );
    }

    #[test]
    fn a_traversing_name_is_refused() {
        assert!(safe_relative("../evil.txt").is_none());
        assert!(safe_relative("a/../../evil.txt").is_none());
        assert!(safe_relative("/etc/passwd").is_none());
        assert!(safe_relative("C:\\Windows\\evil.dll").is_none());
        assert!(safe_relative("\\\\server\\share\\evil.dll").is_none());
        assert!(safe_relative("").is_none());
        assert!(safe_relative("   ").is_none());
    }

    #[test]
    fn an_ordinary_name_is_accepted() {
        assert_eq!(
            safe_relative("folder/file.txt"),
            Some(PathBuf::from("folder").join("file.txt"))
        );
        assert_eq!(
            safe_relative("./folder//file.txt"),
            Some(PathBuf::from("folder").join("file.txt"))
        );
        assert_eq!(
            safe_relative("folder\\file.txt"),
            Some(PathBuf::from("folder").join("file.txt"))
        );
    }

    #[test]
    fn safe_relative_refuses_windows_device_names() {
        assert!(safe_relative("CON").is_none());
        assert!(safe_relative("dir/NUL.txt").is_none());
        assert!(safe_relative("COM1").is_none());
    }

    #[test]
    fn a_joined_path_can_never_leave_the_destination() {
        let dest = Path::new("/tmp/out");
        assert!(target_path(dest, Path::new("a/b.txt")).is_some());
        assert!(target_path(dest, Path::new("../b.txt")).is_none());
    }

    #[test]
    fn the_concat_reader_reads_several_files_as_one_stream() {
        let dir = std::env::temp_dir().join(format!("dc-concat-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let a = dir.join("a.bin");
        let b = dir.join("b.bin");
        std::fs::write(&a, b"hello ").unwrap();
        std::fs::write(&b, b"world").unwrap();
        let mut r = ConcatReader::open(vec![a.clone(), b.clone()]).unwrap();
        let mut buf = Vec::new();
        r.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"hello world");
        // Seeking back must land in the right part.
        let mut r = ConcatReader::open(vec![a, b]).unwrap();
        r.seek(SeekFrom::Start(6)).unwrap();
        let mut tail = String::new();
        r.read_to_string(&mut tail).unwrap();
        assert_eq!(tail, "world");
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[cfg(test)]
mod final_audit_path_tests {
    use super::*;
    #[test]
    fn extraction_refuses_existing_directory_links() {
        let root = std::env::temp_dir().join(format!("dc-archive-link-{}", std::process::id()));
        let dest = root.join("dest"); let outside = root.join("outside");
        std::fs::create_dir_all(&dest).unwrap(); std::fs::create_dir_all(&outside).unwrap();
        let link = dest.join("redirect");
        #[cfg(windows)]
        {
            let result = std::process::Command::new("cmd").args(["/C", "mklink", "/J"]).arg(&link).arg(&outside).output().unwrap();
            assert!(result.status.success(), "test junction must be created");
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        assert!(target_path(&dest, Path::new("redirect/escaped.txt")).is_none());
        let mut input = std::io::Cursor::new(b"must stay inside");
        assert!(write_entry(&dest, Path::new("redirect/escaped.txt"), &mut input, false).is_err());
        assert!(!outside.join("escaped.txt").exists());
        #[cfg(windows)]
        std::fs::remove_dir(&link).unwrap();
        #[cfg(unix)]
        std::fs::remove_file(&link).unwrap();
        std::fs::remove_dir(&dest).unwrap(); std::fs::remove_dir(&outside).unwrap(); std::fs::remove_dir(&root).unwrap();
    }
}
