//! Installing an update on macOS. The verified zip unpacks beside the installed
//! `Spaceman.app` (same volume) into `.Spaceman-update-<version>`; the bundle in
//! it must be `anomaly.spaceman`, satisfy Anomaly's notarized Developer ID
//! requirement (`app/update-requirement.txt`) and carry team `5NZ4Q7NXJ4`.
//! After Spaceman has restored every parked window, the two bundles swap in one
//! step (`renamex_np` with `RENAME_SWAP`) and the new one opens once this process
//! exits (or, at a quit, at the next launch). The old bundle waits in the
//! staging folder until the next launch removes it. A read-only or translocated
//! location never stages an update; it is offered as a download instead.

use std::ffi::CString;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const BUNDLE_ID: &str = "anomaly.spaceman";
const TEAM_ID: &str = "5NZ4Q7NXJ4";
const REQUIREMENT: &str = include_str!("../../app/update-requirement.txt");
/// Only folders with this exact prefix beside the bundle are ever removed.
const PREFIX: &str = ".Spaceman-update-";
const APP: &str = "Spaceman.app";
/// The executable whose hash identifies an installed build (`super::receipt`).
pub(super) const EXECUTABLE: &str = "Contents/MacOS/spaceman-agent";

/// An unpacked, verified bundle and the installed bundle it replaces.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Staged {
    pub(super) update: PathBuf,
    pub(super) installed: PathBuf,
}

/// Whether `bundle` is the Anomaly release this updater is allowed to replace.
/// Any other identifier, including SpaceFred, never checks for those releases.
pub(super) fn is_anomaly_release(bundle: &Path) -> bool {
    bundle_identifier(bundle).as_deref() == Some(BUNDLE_ID)
}

fn bundle_identifier(bundle: &Path) -> Option<String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(bundle.join("Contents/Info.plist"))
        .stdin(Stdio::null())
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8(output.stdout).ok())?
        .map(|identifier| identifier.trim().to_owned())
}

/// The running app bundle, when this executable is inside one.
pub(super) fn installed_bundle() -> Option<PathBuf> {
    let executable = std::env::current_exe().ok()?.canonicalize().ok()?;
    let bundle = executable.parent()?.parent()?.parent()?;
    (bundle
        .extension()
        .is_some_and(|extension| extension == "app")
        && bundle.join("Contents/Info.plist").is_file())
    .then(|| bundle.to_path_buf())
}

/// The swap renames inside the bundle's folder, so the folder and bundle must
/// be writable by this user, and the bundle not an App Translocation copy.
pub(super) fn replaceable(bundle: &Path) -> bool {
    let writable = |path: &Path| {
        CString::new(path.as_os_str().as_bytes()).is_ok_and(|path| {
            // SAFETY: `path` is NUL-terminated and outlives the call; access has no side effects.
            unsafe { libc::access(path.as_ptr(), libc::W_OK) == 0 }
        })
    };
    !bundle
        .components()
        .any(|component| component.as_os_str() == "AppTranslocation")
        && bundle.parent().is_some_and(writable)
        && writable(bundle)
}

/// Unpacks `archive` beside `installed` and verifies the bundle in it.
pub(super) fn stage(archive: &Path, installed: &Path, version: &str) -> Result<Staged, String> {
    let parent = installed
        .parent()
        .ok_or("Spaceman.app has no parent folder")?;
    let staging = parent.join(format!("{PREFIX}{version}"));
    let result = unpack(archive, &staging).and_then(|update| {
        verify_bundle(&update, REQUIREMENT, Some(TEAM_ID))?;
        Ok(update)
    });
    match result {
        Ok(update) => Ok(Staged {
            update,
            installed: installed.to_path_buf(),
        }),
        Err(error) => {
            drop(fs::remove_dir_all(&staging));
            Err(error)
        }
    }
}

/// Unpacks a zip that must hold exactly `Spaceman.app`, as the release builds it.
fn unpack(archive: &Path, staging: &Path) -> Result<PathBuf, String> {
    if staging.exists() {
        fs::remove_dir_all(staging).map_err(|error| error.to_string())?;
    }
    fs::create_dir(staging).map_err(|error| error.to_string())?;
    let status = Command::new("/usr/bin/ditto")
        .args(["-x", "-k"])
        .arg(archive)
        .arg(staging)
        .stdin(Stdio::null())
        .status()
        .map_err(|error| format!("couldn't run ditto: {error}"))?;
    if !status.success() {
        return Err(format!("couldn't unpack the update ({status})"));
    }
    let entries = fs::read_dir(staging)
        .map_err(|error| error.to_string())?
        .flatten()
        .map(|entry| entry.file_name())
        .collect::<Vec<_>>();
    if entries.len() != 1 || entries[0] != APP {
        return Err("the update must hold exactly Spaceman.app".into());
    }
    Ok(staging.join(APP))
}

/// Checks the bundle's identifier, code signature against `requirement`, and
/// signing team before anything runs from it.
fn verify_bundle(bundle: &Path, requirement: &str, team: Option<&str>) -> Result<(), String> {
    let output = Command::new("/usr/libexec/PlistBuddy")
        .args(["-c", "Print :CFBundleIdentifier"])
        .arg(bundle.join("Contents/Info.plist"))
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("couldn't run PlistBuddy: {error}"))?;
    if !output.status.success() || String::from_utf8_lossy(&output.stdout).trim() != BUNDLE_ID {
        return Err(format!("the update is not {BUNDLE_ID}"));
    }
    let output = Command::new("/usr/bin/codesign")
        .args(["--verify", "--deep", "--strict"])
        .arg(format!("-R={}", requirement.trim()))
        .arg(bundle)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| format!("couldn't run codesign: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "the update's code signature is not Anomaly's notarized Developer ID: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    if let Some(team) = team {
        let output = Command::new("/usr/bin/codesign")
            .args(["--display", "--verbose=2"])
            .arg(bundle)
            .stdin(Stdio::null())
            .output()
            .map_err(|error| format!("couldn't run codesign: {error}"))?;
        let expected = format!("TeamIdentifier={team}");
        if !String::from_utf8_lossy(&output.stderr)
            .lines()
            .any(|line| line.trim() == expected)
        {
            return Err(format!("the update is not signed by team {team}"));
        }
    }
    Ok(())
}

/// Checks the staged bundle once more and swaps it in; with `relaunch`, opens
/// it once this process has exited, else it starts at the next launch (a
/// quit). Called only after every parked window is restored.
pub(super) fn install(staged: &Staged, relaunch: bool) -> Result<(), String> {
    verify_bundle(&staged.update, REQUIREMENT, Some(TEAM_ID))?;
    swap(staged)?;
    if !relaunch {
        return Ok(());
    }
    Command::new("/bin/sh")
        .args([
            "-c",
            "while kill -0 \"$SPACEMAN_PID\" 2>/dev/null; do sleep 0.2; done; exec /usr/bin/open \"$SPACEMAN_BUNDLE\"",
        ])
        .env("SPACEMAN_PID", std::process::id().to_string())
        .env("SPACEMAN_BUNDLE", &staged.installed)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        // Its own process group, so nothing that ends Spaceman's group ends it.
        .process_group(0)
        .spawn()
        .map_err(|error| format!("couldn't schedule the relaunch: {error}"))?;
    Ok(())
}

/// Afterwards the installed path holds the update and the staging folder the
/// previous version.
fn swap(staged: &Staged) -> Result<(), String> {
    let from =
        CString::new(staged.update.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    let to =
        CString::new(staged.installed.as_os_str().as_bytes()).map_err(|error| error.to_string())?;
    // SAFETY: Both paths are NUL-terminated and outlive the call.
    if unsafe { libc::renamex_np(from.as_ptr(), to.as_ptr(), libc::RENAME_SWAP) } != 0 {
        return Err(format!(
            "couldn't swap in the updated Spaceman.app: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

/// Removes a staged update Spaceman won't install.
pub(super) fn discard(staged: &Staged) {
    if let Some(folder) = staged.update.parent() {
        drop(fs::remove_dir_all(folder));
    }
}

/// Removes every staging folder beside `bundle` except `keep`: earlier updates,
/// and the bundle an update replaced once the new one runs. Never the bundle
/// itself, and never a symlink.
pub(super) fn remove_beside(bundle: &Path, keep: Option<&Path>) {
    let Some(folder) = bundle
        .parent()
        .and_then(|folder| folder.canonicalize().ok())
    else {
        return;
    };
    let Ok(entries) = fs::read_dir(&folder) else {
        return;
    };
    let keep = keep.and_then(Path::parent).and_then(Path::file_name);
    let running = bundle
        .canonicalize()
        .ok()
        .and_then(|bundle| bundle.parent().map(Path::to_path_buf));
    for entry in entries.flatten() {
        let name = entry.file_name();
        let path = entry.path();
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && name.to_str().is_some_and(|name| name.starts_with(PREFIX))
            && Some(name.as_os_str()) != keep
            && running.as_deref() != Some(path.as_path())
        {
            drop(fs::remove_dir_all(path));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("spaceman-bundle-{name}-{}", std::process::id()));
        drop(fs::remove_dir_all(&path));
        fs::create_dir_all(&path).unwrap();
        path.canonicalize().unwrap()
    }

    /// A minimal ad-hoc signed `Spaceman.app` with `identifier`.
    fn bundle(root: &Path, identifier: &str) -> PathBuf {
        let bundle = root.join(APP);
        fs::create_dir_all(bundle.join("Contents/MacOS")).unwrap();
        fs::copy("/usr/bin/true", bundle.join(EXECUTABLE)).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string>\
                 <key>CFBundleExecutable</key><string>spaceman-agent</string>\
                 </dict></plist>"
            ),
        )
        .unwrap();
        let status = Command::new("/usr/bin/codesign")
            .args(["--force", "--sign", "-", "--identifier", identifier])
            .arg(&bundle)
            .stderr(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
        bundle
    }

    #[test]
    fn the_developer_id_requirement_compiles_and_names_the_team() {
        assert!(REQUIREMENT.contains(&format!("leaf[subject.OU] = \"{TEAM_ID}\"")));
        assert!(REQUIREMENT.contains(&format!("identifier \"{BUNDLE_ID}\"")));
        assert!(REQUIREMENT.trim_end().ends_with("and notarized"));
        let status = Command::new("/usr/bin/csreq")
            .arg(format!("-r={}", REQUIREMENT.trim()))
            .arg("-t")
            .stdout(Stdio::null())
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[test]
    fn only_anomalys_signature_team_and_identity_pass() {
        let root = temporary("verify");
        let app = bundle(&root, BUNDLE_ID);
        let identifier = "identifier \"anomaly.spaceman\"";
        // An ad-hoc signature satisfies a plain identifier requirement…
        verify_bundle(&app, identifier, None).unwrap();
        // …but has no team, and never satisfies Anomaly's Developer ID requirement.
        assert!(verify_bundle(&app, identifier, Some(TEAM_ID)).is_err());
        let error = verify_bundle(&app, REQUIREMENT, None).unwrap_err();
        assert!(error.contains("code signature"), "{error}");
        let other = bundle(&root.join("other"), "com.kitlangton.spaceman");
        let error =
            verify_bundle(&other, "identifier \"com.kitlangton.spaceman\"", None).unwrap_err();
        assert!(error.contains("not anomaly.spaceman"), "{error}");
        // A modified bundle no longer satisfies its signature.
        fs::write(app.join(EXECUTABLE), b"tampered").unwrap();
        assert!(verify_bundle(&app, identifier, None).is_err());
        drop(fs::remove_dir_all(root));
    }

    #[test]
    fn a_zip_must_hold_exactly_spaceman_app() {
        let root = temporary("unpack");
        let app = bundle(&root.join("source"), BUNDLE_ID);
        let zip = |name: &str, extra: bool| {
            let archive = root.join(name);
            let mut command = Command::new("/usr/bin/ditto");
            command.args(["-c", "-k"]);
            if extra {
                fs::write(root.join("source/extra"), "x").unwrap();
                command.arg(root.join("source"));
            } else {
                command.arg("--keepParent").arg(&app);
            }
            assert!(command.arg(&archive).status().unwrap().success());
            archive
        };
        let staging = root.join(format!("{PREFIX}9.9.9"));
        fs::create_dir_all(staging.join("stale")).unwrap();
        let unpacked = unpack(&zip("good.zip", false), &staging).unwrap();
        assert_eq!(unpacked, staging.join(APP));
        assert!(!staging.join("stale").exists());
        verify_bundle(&unpacked, "identifier \"anomaly.spaceman\"", None).unwrap();
        assert!(unpack(&zip("extra.zip", true), &staging).is_err());
        drop(fs::remove_dir_all(root));
    }

    #[test]
    fn the_swap_exchanges_bundles_and_launch_cleanup_removes_the_old_one() {
        let root = temporary("swap");
        let installed = root.join(APP);
        fs::create_dir_all(installed.join("Contents")).unwrap();
        fs::write(installed.join("Contents/version"), "old").unwrap();
        let staging = root.join(format!("{PREFIX}9.9.9"));
        let update = staging.join(APP);
        fs::create_dir_all(update.join("Contents")).unwrap();
        fs::write(update.join("Contents/version"), "new").unwrap();
        let older = root.join(format!("{PREFIX}9.9.8"));
        fs::create_dir_all(older.join(APP)).unwrap();
        fs::create_dir_all(root.join("Other.app")).unwrap();

        let staged = Staged {
            update: update.clone(),
            installed: installed.clone(),
        };
        remove_beside(&installed, Some(&update));
        assert!(!older.exists(), "an earlier update is removed");
        assert!(staging.exists(), "the staged update stays");

        swap(&staged).unwrap();
        assert_eq!(
            fs::read_to_string(installed.join("Contents/version")).unwrap(),
            "new"
        );
        assert_eq!(
            fs::read_to_string(update.join("Contents/version")).unwrap(),
            "old"
        );

        remove_beside(&installed, None);
        assert!(
            !staging.exists(),
            "the replaced bundle goes at the next launch"
        );
        assert!(installed.exists());
        assert!(root.join("Other.app").exists(), "only the prefix is ours");
        drop(fs::remove_dir_all(root));
    }

    #[test]
    fn a_bundle_running_from_a_staging_folder_keeps_that_folder() {
        let root = temporary("inside");
        let staging = root.join(format!("{PREFIX}9.9.9"));
        let bundle = staging.join(APP);
        fs::create_dir_all(bundle.join("Contents")).unwrap();
        remove_beside(&bundle, None);
        assert!(bundle.exists());
        drop(fs::remove_dir_all(root));
    }

    #[test]
    fn translocated_and_read_only_locations_are_not_replaceable() {
        let root = temporary("replaceable");
        let app = root.join(APP);
        fs::create_dir_all(&app).unwrap();
        assert!(replaceable(&app));
        assert!(!replaceable(&root.join("AppTranslocation/x/Spaceman.app")));
        assert!(!replaceable(Path::new("/System/Applications/Spaceman.app")));
        drop(fs::remove_dir_all(root));
    }
}
