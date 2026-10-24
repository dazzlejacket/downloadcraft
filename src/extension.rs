//! Setting up the browser extension.
//!
//! The app ships the extension next to its executable; this module stages a
//! browser-loadable copy in the user's data folder, packages it for the
//! browsers that want a file (Firefox wants an `.xpi`, Chrome wants an
//! unpacked folder), and reports whether the browser actually has it
//! installed.
//!
//! Nothing here publishes anything: the extension is loaded from disk, the
//! way a user loads an unpacked extension. The Chrome Web Store is not
//! involved and the app never claims otherwise.

use std::path::{Path, PathBuf};

/// The Chrome/Brave extension id this build uses.
pub const EXTENSION_ID: &str = crate::app::EXTENSION_ID;

/// Where the extension ended up and what the browser thinks of it.
#[derive(Debug, Clone)]
pub struct ExtensionStatus {
    /// The folder the browser loads.
    pub dir: PathBuf,
    /// The version in the manifest.
    pub version: String,
    /// Whether the browser has the extension installed and enabled.
    pub installed: Option<bool>,
    /// Which browser profile was looked at.
    pub profile_note: String,
}

/// Stage the extension files and report where they are.
pub fn prepare(firefox: bool) -> Result<ExtensionStatus, String> {
    let dir = crate::platform::extension_directory(firefox)?;
    let version = read_manifest_version(&dir)?;
    let (installed, profile_note) = match installed_state(firefox) {
        Some((state, note)) => (Some(state), note),
        None => (None, "no browser profile was found on this machine".into()),
    };
    Ok(ExtensionStatus {
        dir,
        version,
        installed,
        profile_note,
    })
}

/// The version recorded in a staged manifest.
fn read_manifest_version(dir: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(dir.join("manifest.json"))
        .map_err(|e| format!("the manifest could not be read: {e}"))?;
    let value: serde_json::Value = serde_json::from_str(&text)
        .map_err(|e| format!("the manifest is not valid JSON: {e}"))?;
    Ok(value
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown")
        .to_string())
}

/// Chrome stores its settings in a JSON file per profile. The extension is
/// listed there once it is installed, with a state of 1 when it is enabled.
fn chrome_installed() -> Option<(bool, String)> {
    let local = dirs::data_local_dir()?;
    let candidates: Vec<(String, PathBuf)> = vec![
        (
            "Google Chrome".into(),
            local.join("Google/Chrome/User Data/Default/Preferences"),
        ),
        (
            "Microsoft Edge".into(),
            local.join("Microsoft/Edge/User Data/Default/Preferences"),
        ),
        (
            "Brave".into(),
            local.join("BraveSoftware/Brave-Browser/User Data/Default/Preferences"),
        ),
        (
            "Vivaldi".into(),
            local.join("Vivaldi/User Data/Default/Preferences"),
        ),
    ];
    for (name, path) in candidates {
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(settings) = value.get("extensions").and_then(|e| e.get("settings")) else {
            continue;
        };
        let Some(entry) = settings.get(EXTENSION_ID) else {
            continue;
        };
        let state = entry.get("state").and_then(|s| s.as_i64()).unwrap_or(0);
        let location = entry
            .get("path")
            .and_then(|p| p.as_str())
            .unwrap_or("")
            .to_string();
        return Some((
            state == 1,
            format!("{name}: {}{}", path.display(), suffix(&location)),
        ));
    }
    None
}

/// Firefox keeps its add-ons in `extensions.json` inside the profile.
fn firefox_installed() -> Option<(bool, String)> {
    let roaming = dirs::config_dir()?;
    let profiles = roaming.join("Mozilla/Firefox/Profiles");
    let entries = std::fs::read_dir(&profiles).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("extensions.json");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&text) else {
            continue;
        };
        let Some(addons) = value.get("addons").and_then(|a| a.as_array()) else {
            continue;
        };
        for addon in addons {
            if addon.get("id").and_then(|i| i.as_str()) == Some(EXTENSION_ID) {
                let active = addon.get("active").and_then(|a| a.as_bool()).unwrap_or(false);
                return Some((active, format!("Firefox: {}", path.display())));
            }
        }
    }
    None
}

fn suffix(location: &str) -> String {
    if location.trim().is_empty() {
        String::new()
    } else {
        format!(" ({location})")
    }
}

/// Is the extension installed in a browser on this machine?
pub fn installed_state(firefox: bool) -> Option<(bool, String)> {
    if firefox {
        firefox_installed()
    } else {
        chrome_installed()
    }
}

/// The steps the user has to follow to load the extension, in order.
pub fn install_steps(firefox: bool, dir: &Path) -> Vec<String> {
    if firefox {
        vec![
            format!("Open the Firefox folder: {}", dir.display()),
            "Open about:debugging#/runtime/this-firefox in Firefox.".into(),
            "Press \"Load Temporary Add-on…\" and pick manifest.json in that folder.".into(),
            "Keep the folder where it is: the extension is loaded from there.".into(),
        ]
    } else {
        vec![
            format!("Open the extension folder: {}", dir.display()),
            "Open chrome://extensions (or edge://extensions, brave://extensions).".into(),
            "Turn on \"Developer mode\" in the top right.".into(),
            "Press \"Load unpacked\" and pick that folder.".into(),
            "Pin \"Send to Download Craft\" from the extensions menu.".into(),
        ]
    }
}

/// Pack the staged extension into a single file.
///
/// Chrome loads an unpacked folder, so the file it gets is a `.zip` the user
/// unpacks first. Firefox takes the `.xpi` directly. Both are built from the
/// same staged files — nothing is duplicated.
pub fn package(firefox: bool) -> Result<PathBuf, String> {
    let status = prepare(firefox)?;
    let version = status.version.clone();
    let out_dir = crate::model::config_dir()
        .ok_or("no config directory")?
        .join("extension-packages");
    std::fs::create_dir_all(&out_dir).map_err(|e| e.to_string())?;
    let name = if firefox {
        format!("DownloadCraft-Firefox-{version}.xpi")
    } else {
        format!("DownloadCraft-Chrome-{version}.zip")
    };
    let target = out_dir.join(&name);
    let file = std::fs::File::create(&target).map_err(|e| e.to_string())?;
    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    add_tree(&mut zip, &status.dir, &status.dir, options)
        .map_err(|e| format!("the package could not be written: {e}"))?;
    zip.finish().map_err(|e| format!("the package could not be finished: {e}"))?;
    Ok(target)
}

fn add_tree(
    zip: &mut zip::ZipWriter<std::fs::File>,
    root: &Path,
    dir: &Path,
    options: zip::write::SimpleFileOptions,
) -> zip::result::ZipResult<()> {
    let entries = std::fs::read_dir(dir).map_err(zip::result::ZipError::Io)?;
    for entry in entries.flatten() {
        let path = entry.path();
        let name = path
            .strip_prefix(root)
            .map_err(|e| zip::result::ZipError::InvalidArchive(e.to_string().into()))?;
        // The same exclusion set the staging copy and
        // `scripts/package_extensions.py` use: a file left over from an
        // earlier staging never reaches the package.
        if !crate::platform::is_extension_build_file(&entry.file_name().to_string_lossy()) {
            continue;
        }
        if path.is_dir() {
            zip.add_directory(name.to_string_lossy().replace('\\', "/"), options)?;
            add_tree(zip, root, &path, options)?;
        } else if path.is_file() {
            zip.start_file(name.to_string_lossy().replace('\\', "/"), options)?;
            let bytes = std::fs::read(&path).map_err(zip::result::ZipError::Io)?;
            std::io::Write::write_all(zip, &bytes).map_err(zip::result::ZipError::Io)?;
        }
    }
    Ok(())
}

/// Is the native messaging host registered for this browser?
pub fn native_host_registered(browser: &str) -> bool {
    #[cfg(windows)]
    {
        let key = match browser {
            "chrome" => r"HKCU\Software\Google\Chrome\NativeMessagingHosts\com.download_craft",
            "edge" => r"HKCU\Software\Microsoft\Edge\NativeMessagingHosts\com.download_craft",
            "brave" => r"HKCU\Software\BraveSoftware\Brave-Browser\NativeMessagingHosts\com.download_craft",
            "opera" => r"HKCU\Software\Opera Software\NativeMessagingHosts\com.download_craft",
            "vivaldi" => r"HKCU\Software\Vivaldi\NativeMessagingHosts\com.download_craft",
            _ => r"HKCU\Software\Mozilla\NativeMessagingHosts\com.download_craft",
        };
        let out = std::process::Command::new("reg")
            .args(["query", key])
            .output();
        matches!(out, Ok(o) if o.status.success())
    }
    #[cfg(not(windows))]
    {
        let _ = browser;
        false
    }
}

/// Round-trip a ping through this app's native host.
///
/// This proves the transport the extension uses actually works: the same
/// binary, the same framing and the same reply the browser would get.
pub fn test_native_host() -> Result<String, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let mut child = std::process::Command::new(exe)
        .arg("--native-host")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .spawn()
        .map_err(|e| format!("the native host could not be started: {e}"))?;
    {
        use std::io::Write;
        let stdin = child.stdin.as_mut().ok_or("the native host has no input")?;
        let body = br#"{"type":"ping"}"#;
        let mut frame = (body.len() as u32).to_le_bytes().to_vec();
        frame.extend_from_slice(body);
        stdin.write_all(&frame).map_err(|e| e.to_string())?;
        stdin.flush().map_err(|e| e.to_string())?;
    }
    // Close the input: the host serves until the browser goes away, and that
    // is exactly what an end of input means to it.
    drop(child.stdin.take());
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    let bytes = out.stdout;
    if bytes.len() < 4 {
        return Err("the native host did not answer".into());
    }
    let len = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    if bytes.len() < 4 + len {
        return Err("the native host answered with a short frame".into());
    }
    let text = String::from_utf8_lossy(&bytes[4..4 + len]).to_string();
    if !text.contains("pong") {
        return Err(format!("the native host answered with something unexpected: {text}"));
    }
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_install_steps_name_the_folder_and_the_browser() {
        let dir = Path::new("/tmp/ext");
        let chrome = install_steps(false, dir);
        assert!(chrome.iter().any(|s| s.contains("chrome://extensions")));
        assert!(chrome.iter().any(|s| s.contains("Developer mode")));
        assert!(chrome.iter().any(|s| s.contains("Load unpacked")));
        assert!(chrome.iter().any(|s| s.contains("/tmp/ext")));
        let firefox = install_steps(true, dir);
        assert!(firefox.iter().any(|s| s.contains("about:debugging")));
        assert!(firefox.iter().any(|s| s.contains("manifest.json")));
    }

    #[test]
    fn the_steps_never_claim_a_web_store_listing() {
        for firefox in [true, false] {
            for step in install_steps(firefox, Path::new("/tmp/ext")) {
                let lower = step.to_ascii_lowercase();
                assert!(!lower.contains("web store"));
                assert!(!lower.contains("publish"));
                assert!(!lower.contains("signed"));
            }
        }
    }
}
