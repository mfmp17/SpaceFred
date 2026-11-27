//! Spaceman's in-app updater, a client of Anomaly's release service
//! (`https://anoma.ly/release/spaceman/desktop/<channel>.json`). A worker thread
//! checks the channel 10 seconds after launch, then hourly (every 15 minutes on
//! the dev channel) or on "Check for Updates". When the channel's current
//! release isn't the running build it downloads the zip from the release
//! service's own `files/`, checks its size and SHA-256 against the manifest,
//! unpacks it beside the installed bundle and checks its code signature
//! (`bundle`). The update then waits for Install & restart, which restores every
//! parked window first (`ExitIntent::Update`) and only then swaps the bundles.
//! Quitting with an update ready installs it the same way, after the restore,
//! for the next launch. Manifest polls revalidate with `If-None-Match`, and an
//! interrupted download resumes (`transport`).
//!
//! Versions are display text and never compared: a build installed by this
//! updater is known by its target's SHA-256 (`installed.json`, tied to the
//! running executable's hash), any other by its exact version. Each build
//! knows the channel and version it was published as (`stamp`); a channel
//! setting other than the build's own offers a switch the user confirms.
//! A copy Spaceman can't replace (a read-only or translocated location) still
//! checks, and offers the release's download instead of installing it.
//! Manifests are not signed: HTTPS, the manifest's SHA-256 and the Developer ID
//! signature are the integrity checks. Builds from source (no
//! `SPACEMAN_RELEASE_BUILD` stamp, which only the Release workflow's build sets)
//! never check.

#[allow(unsafe_code)]
mod bundle;
mod protocol;
mod stamp;
pub use stamp::build_version;
mod transport;

use std::path::{Path, PathBuf};
use std::sync::{
    Arc, Condvar, Mutex, MutexGuard,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

use crate::app::UiUpdateStatus;
use crate::config::{Config, UpdateChannel};
use protocol::{Decision, Install, Reason, Target};

const APP: &str = "spaceman";
const ARTIFACT: &str = "desktop";
const TARGET: &str = "macos-arm64";
const ORIGIN: &str = "https://anoma.ly/release";
/// Every download must come from the release service's own files.
const FILES: &str = "https://anoma.ly/release/spaceman/files/";
const STARTUP_DELAY: Duration = Duration::from_secs(10);
const CHECK_INTERVAL: Duration = Duration::from_secs(60 * 60);
const DEV_CHECK_INTERVAL: Duration = Duration::from_secs(15 * 60);
const RETRY_INTERVAL: Duration = Duration::from_secs(5 * 60);
const RECEIPT: &str = "installed.json";
const BUCKET: &str = "bucket";

/// How this install takes updates.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum Method {
    /// A `Spaceman.app` this user can replace: updates stage and install.
    Direct,
    /// A read-only or translocated copy: updates are offered as a download.
    DownloadOnly,
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Available {
    /// The channel setting names another channel, whose release waits for
    /// the user to confirm the switch.
    Switch {
        channel: UpdateChannel,
        version: String,
        sha256: String,
    },
    /// A release this copy can't install itself; `url` is its zip.
    Download { version: String, url: String },
}

#[derive(Clone, Debug, Eq, PartialEq)]
enum Phase {
    Disabled(UiUpdateStatus),
    Idle,
    Checking,
    UpToDate,
    Downloading,
    Available(Available),
    Failed,
    /// Swapped in; the process is about to exit, so nothing may change it.
    Installed,
}

/// A verified update waiting for Install & restart.
#[derive(Clone, Debug, Eq, PartialEq)]
struct StagedUpdate {
    channel: UpdateChannel,
    staged: bundle::Staged,
    sha256: String,
    version: String,
    /// The SHA-256 of its `spaceman-agent`, which identifies it once it runs.
    executable: String,
}

struct Core {
    phase: Phase,
    staged: Option<StagedUpdate>,
    /// The channel and SHA-256 of the other channel's release the user agreed
    /// to switch to; the next check on that channel takes it.
    confirmed_switch: Option<(UpdateChannel, String)>,
    check_requested: bool,
    stop: bool,
}

impl Core {
    fn status(&self) -> UiUpdateStatus {
        match (&self.phase, &self.staged) {
            (Phase::Disabled(status), _) => status.clone(),
            (Phase::Installed, _) => UiUpdateStatus::Installing,
            (
                Phase::Available(Available::Switch {
                    channel, version, ..
                }),
                _,
            ) => UiUpdateStatus::Switch {
                channel: channel.as_str().into(),
                version: version.clone(),
            },
            (Phase::Available(Available::Download { version, .. }), _) => {
                UiUpdateStatus::Download {
                    version: version.clone(),
                }
            }
            (_, Some(update)) => UiUpdateStatus::Ready {
                version: update.version.clone(),
            },
            (Phase::Idle, None) => UiUpdateStatus::Idle,
            (Phase::Checking, None) => UiUpdateStatus::Checking,
            (Phase::UpToDate, None) => UiUpdateStatus::UpToDate,
            (Phase::Downloading, None) => UiUpdateStatus::Downloading,
            (Phase::Failed, None) => UiUpdateStatus::Failed,
        }
    }

    fn live(&self) -> bool {
        !matches!(self.phase, Phase::Disabled(_) | Phase::Installed)
    }

    /// Adopts a newly staged update unless one was already installed; returns
    /// what to discard.
    fn adopt(&mut self, update: StagedUpdate) -> Option<StagedUpdate> {
        if !self.live() {
            return Some(update);
        }
        self.staged.replace(update)
    }
}

struct Shared {
    core: Mutex<Core>,
    signal: Condvar,
    /// Bumped on every change the UI shows.
    revision: AtomicU64,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, Core> {
        self.core.lock().unwrap_or_else(|error| error.into_inner())
    }

    fn changed(&self) {
        self.revision.fetch_add(1, Ordering::AcqRel);
    }

    fn set_phase(&self, phase: Phase) {
        let mut core = self.lock();
        if core.live() {
            core.phase = phase;
        }
        drop(core);
        self.changed();
    }
}

pub(crate) struct Updater {
    shared: Arc<Shared>,
}

impl Updater {
    /// Starts the worker, unless this build can't update at all. A copy it
    /// can't replace still checks, to offer the download.
    pub(crate) fn start() -> Self {
        let installed = bundle::installed_bundle();
        let disabled = if option_env!("SPACEMAN_RELEASE_BUILD") != Some("1") {
            Some(UiUpdateStatus::SourceBuild)
        } else if installed
            .as_ref()
            .is_some_and(|bundle| !bundle::is_anomaly_release(bundle))
        {
            // A fork such as SpaceFred must not download Anomaly's Spaceman.app
            // and swap it over itself, even if a release stamp was set.
            Some(UiUpdateStatus::Unavailable)
        } else if std::env::var_os("SPACEMAN_DISABLE_UPDATES").is_some_and(|value| value == "1")
            || installed.is_none()
        {
            Some(UiUpdateStatus::Unavailable)
        } else {
            None
        };
        let updater = Self::with_phase(disabled.clone().map_or(Phase::Idle, Phase::Disabled));
        if let (None, Some(installed)) = (disabled, installed) {
            let method = if bundle::replaceable(&installed) {
                Method::Direct
            } else {
                Method::DownloadOnly
            };
            let shared = Arc::clone(&updater.shared);
            if let Err(error) = std::thread::Builder::new()
                .name("updater".into())
                .spawn(move || run(&shared, &installed, method))
            {
                log(&format!("couldn't start: {error}"));
                updater.shared.set_phase(Phase::Failed);
            }
        }
        updater
    }

    fn with_phase(phase: Phase) -> Self {
        Self {
            shared: Arc::new(Shared {
                core: Mutex::new(Core {
                    phase,
                    staged: None,
                    confirmed_switch: None,
                    check_requested: false,
                    stop: false,
                }),
                signal: Condvar::new(),
                revision: AtomicU64::new(0),
            }),
        }
    }

    pub(crate) fn status(&self) -> UiUpdateStatus {
        self.shared.lock().status()
    }

    /// Changes whenever `status` may have.
    pub(crate) fn revision(&self) -> u64 {
        self.shared.revision.load(Ordering::Acquire)
    }

    pub(crate) fn ready(&self) -> bool {
        let core = self.shared.lock();
        core.live() && core.staged.is_some()
    }

    /// Checks now instead of at the next interval; a check asked for during a
    /// download runs right after it.
    pub(crate) fn check_now(&self) {
        let mut core = self.shared.lock();
        if !core.live() {
            return;
        }
        if matches!(
            core.phase,
            Phase::Idle | Phase::UpToDate | Phase::Failed | Phase::Available(_)
        ) {
            core.phase = Phase::Checking;
        }
        core.check_requested = true;
        drop(core);
        self.shared.changed();
        self.shared.signal.notify_all();
    }

    /// The user agreed to switch to the other channel's release on offer:
    /// the next check, which starts now, downloads and stages it.
    pub(crate) fn confirm_switch(&self) {
        let mut core = self.shared.lock();
        let Phase::Available(Available::Switch {
            channel, sha256, ..
        }) = &core.phase
        else {
            return;
        };
        core.confirmed_switch = Some((*channel, sha256.clone()));
        core.phase = Phase::Checking;
        core.check_requested = true;
        drop(core);
        self.shared.changed();
        self.shared.signal.notify_all();
    }

    /// Opens the release's zip, for a copy Spaceman can't replace itself.
    pub(crate) fn open_download(&self) -> Result<(), String> {
        let Phase::Available(Available::Download { url, .. }) = &self.shared.lock().phase else {
            return Err("no download is on offer".into());
        };
        let url = url.clone();
        std::process::Command::new("/usr/bin/open")
            .arg(&url)
            .stdin(std::process::Stdio::null())
            .spawn()
            .map(drop)
            .map_err(|error| format!("couldn't open the download: {error}"))
    }

    /// Swaps in the staged update and schedules its launch for when this
    /// process exits. Call only once every parked window has been restored.
    pub(crate) fn install(&self) -> Result<String, String> {
        self.apply_for(Some(channel()), updates_dir(), |staged| {
            bundle::install(staged, true)
        })
    }

    /// Quitting with an update ready installs it for the next launch, as
    /// Install & restart does but without relaunching. Call only once every
    /// parked window has been restored. `None` when no update was ready.
    pub(crate) fn install_on_quit(&self) -> Option<Result<String, String>> {
        self.install_on_quit_for(channel(), updates_dir(), |staged| {
            bundle::install(staged, false)
        })
    }

    fn install_on_quit_for(
        &self,
        selected: UpdateChannel,
        updates: Result<PathBuf, String>,
        install: impl FnOnce(&bundle::Staged) -> Result<(), String>,
    ) -> Option<Result<String, String>> {
        let discarded = {
            let mut core = self.shared.lock();
            if !core.live() {
                return None;
            }
            match core.staged.as_ref() {
                None => return None,
                Some(update) if update.channel != selected => core.staged.take(),
                Some(_) => None,
            }
        };
        if let Some(update) = discarded {
            bundle::discard(&update.staged);
            self.shared.changed();
            return None;
        }
        Some(self.apply_for(Some(selected), updates, install))
    }

    fn apply_for(
        &self,
        selected: Option<UpdateChannel>,
        updates: Result<PathBuf, String>,
        install: impl FnOnce(&bundle::Staged) -> Result<(), String>,
    ) -> Result<String, String> {
        let mut core = self.shared.lock();
        if !core.live() {
            return Err("no update is ready".into());
        }
        if let Some(selected) = selected
            && core
                .staged
                .as_ref()
                .is_some_and(|staged| staged.channel != selected)
        {
            let discarded = core.staged.take();
            drop(core);
            if let Some(update) = discarded {
                bundle::discard(&update.staged);
            }
            self.shared.changed();
            return Err("no update is ready".into());
        }
        let update = core.staged.take().ok_or("no update is ready")?;
        let result = updates
            .and_then(|updates| write_receipt(&updates, &update))
            .and_then(|()| install(&update.staged));
        match &result {
            Ok(()) => {
                core.phase = Phase::Installed;
                core.stop = true;
            }
            Err(_) => {
                bundle::discard(&update.staged);
                core.phase = Phase::Failed;
            }
        }
        drop(core);
        self.shared.changed();
        self.shared.signal.notify_all();
        result.map(|()| update.version)
    }
}

impl Drop for Updater {
    fn drop(&mut self) {
        self.shared.lock().stop = true;
        self.shared.signal.notify_all();
    }
}

/// Timestamped like the agent's other log lines; never carries user content.
fn log(message: &str) {
    let now = crate::config::now();
    let line = format!(
        "[{}.{:03}] updater: {message}\n",
        now.as_secs(),
        now.subsec_millis()
    );
    drop(std::io::Write::write_all(
        &mut std::io::stderr().lock(),
        line.as_bytes(),
    ));
}

/// Waits `delay` or until woken; true when the updater should stop.
fn wait(shared: &Shared, delay: Duration) -> bool {
    let deadline = std::time::Instant::now() + delay;
    let mut core = shared.lock();
    while !core.check_requested && !core.stop {
        let now = std::time::Instant::now();
        if now >= deadline {
            break;
        }
        core = shared
            .signal
            .wait_timeout(core, deadline - now)
            .unwrap_or_else(|error| error.into_inner())
            .0;
    }
    core.check_requested = false;
    core.stop
}

fn channel() -> UpdateChannel {
    Config::load().map_or(UpdateChannel::Stable, |config| config.update_channel())
}

fn run(shared: &Shared, installed: &Path, method: Method) {
    // The bundle an update replaced, and any update never installed.
    if method == Method::Direct {
        bundle::remove_beside(installed, None);
    }
    if wait(shared, STARTUP_DELAY) {
        return;
    }
    let mut cache = transport::ManifestCache::default();
    let mut failures = 0;
    loop {
        let channel = channel();
        prepare_check(shared, channel);
        let interval = match channel {
            UpdateChannel::Stable => CHECK_INTERVAL,
            UpdateChannel::Dev => DEV_CHECK_INTERVAL,
        };
        let delay = match check(shared, installed, method, &mut cache, channel) {
            Ok(phase) => {
                failures = 0;
                shared.set_phase(phase);
                interval
            }
            Err(error) => {
                log(&format!("check failed: {error}"));
                failures = (failures + 1).min(4);
                shared.set_phase(Phase::Failed);
                (RETRY_INTERVAL * 2u32.pow(failures - 1)).min(interval)
            }
        };
        if wait(shared, delay) {
            return;
        }
    }
}

/// What a check does with the protocol's decision.
#[derive(Debug, Eq, PartialEq)]
enum Plan {
    Reject(&'static str),
    /// The channel's release runs (or was switched back to): nothing to stage.
    Current,
    /// The staged update is still the channel's release.
    Staged,
    /// Another channel's release waits for the user to confirm the switch.
    Switch {
        version: String,
        sha256: String,
    },
    /// This copy can't replace itself: offer the release's download.
    Download {
        version: String,
        target: Target,
    },
    /// Download, verify and stage the release.
    Stage {
        version: String,
        target: Target,
    },
}

/// `confirmed` is the switch the user agreed to; `staged` the SHA-256 of the
/// update waiting for Install & restart.
fn plan(decision: Decision, method: Method, confirmed: Option<&str>, staged: Option<&str>) -> Plan {
    match decision {
        Decision::Reject { reason } => Plan::Reject(reason),
        Decision::None { .. } | Decision::Notify { .. } => Plan::Current,
        Decision::Install {
            version, target, ..
        } if method == Method::DownloadOnly => Plan::Download { version, target },
        Decision::Install { target, .. } if staged == Some(target.sha256.as_str()) => Plan::Staged,
        Decision::Install {
            reason: Reason::Switch,
            version,
            target,
        } if confirmed != Some(target.sha256.as_str()) => Plan::Switch {
            version,
            sha256: target.sha256,
        },
        Decision::Install {
            version, target, ..
        } => Plan::Stage { version, target },
    }
}

/// Where `target` downloads from, which must be the release service's own files.
fn files_url(manifest_url: &str, target: &Target) -> Result<String, String> {
    protocol::resolve_url(manifest_url, &target.url)
        .filter(|url| url.starts_with(FILES))
        .ok_or_else(|| format!("the update URL {} is not the release service's", target.url))
}

fn discard_staged(shared: &Shared) {
    let discarded = shared.lock().staged.take();
    if let Some(update) = discarded {
        bundle::discard(&update.staged);
    }
}

/// Drops any staged update or pending switch confirmation from another channel
/// before fetching the selected channel's manifest, so an offline check failure
/// cannot leave an abandoned channel switch ready to install on quit.
fn prepare_check(shared: &Shared, selected: UpdateChannel) {
    let discarded = {
        let mut core = shared.lock();
        if !core.live() {
            return;
        }
        if core
            .confirmed_switch
            .as_ref()
            .is_some_and(|(channel, _)| *channel != selected)
        {
            core.confirmed_switch = None;
        }
        if core
            .staged
            .as_ref()
            .is_some_and(|staged| staged.channel != selected)
        {
            core.staged.take()
        } else {
            None
        }
    };
    if let Some(update) = discarded {
        bundle::discard(&update.staged);
    }
    shared.set_phase(Phase::Checking);
}

fn check(
    shared: &Shared,
    installed: &Path,
    method: Method,
    cache: &mut transport::ManifestCache,
    channel: UpdateChannel,
) -> Result<Phase, String> {
    let updates = updates_dir()?;
    let receipt = running_receipt(&updates);
    let identity = stamp::running(receipt.as_ref().map(|receipt| receipt.version.as_str()));
    let manifest_url = format!("{ORIGIN}/{APP}/{ARTIFACT}/{}.json", channel.as_str());
    let user_agent = transport::user_agent(
        identity.channel.as_deref().unwrap_or(channel.as_str()),
        &identity.version,
    );
    let manifest = cache.fetch(&manifest_url, &user_agent)?;
    let install = Install {
        version: identity.version,
        target: TARGET.into(),
        distribution: "direct".into(),
        rollout_bucket: rollout_bucket(&updates)?,
        sha256: receipt.map(|receipt| receipt.sha256),
        channel: identity.channel,
    };
    let decision = protocol::decide(APP, ARTIFACT, channel.as_str(), &manifest, &install);
    let (confirmed, staged) = {
        let mut core = shared.lock();
        let staged = core
            .staged
            .as_ref()
            .filter(|staged| staged.channel == channel)
            .map(|staged| staged.sha256.clone());
        let confirmed = core
            .confirmed_switch
            .take()
            .filter(|(confirmed_channel, _)| *confirmed_channel == channel)
            .map(|(_, sha256)| sha256);
        (confirmed, staged)
    };
    match plan(decision, method, confirmed.as_deref(), staged.as_deref()) {
        Plan::Reject(reason) => Err(format!("the update manifest was rejected: {reason}")),
        Plan::Current => {
            discard_staged(shared);
            transport::prune(&updates, None);
            Ok(Phase::UpToDate)
        }
        Plan::Staged => Ok(Phase::Idle),
        Plan::Switch { version, sha256 } => {
            // The staged update is the old channel's; the switch replaces it.
            discard_staged(shared);
            Ok(Phase::Available(Available::Switch {
                channel,
                version,
                sha256,
            }))
        }
        Plan::Download { version, target } => Ok(Phase::Available(Available::Download {
            url: files_url(&manifest_url, &target)?,
            version,
        })),
        Plan::Stage { version, target } => {
            let url = files_url(&manifest_url, &target)?;
            shared.set_phase(Phase::Downloading);
            transport::prune(&updates, Some(&target.sha256));
            let file = transport::download(&url, &user_agent, &target, &updates)?;
            let name = format!("{version}-{}", &target.sha256[..12]);
            let staged = bundle::stage(&file, installed, &name)?;
            transport::prune(&updates, None);
            let executable = transport::sha256_hex(&staged.update.join(bundle::EXECUTABLE))
                .map_err(|error| error.to_string())?;
            if self::channel() != channel {
                bundle::discard(&staged);
                return Ok(Phase::Idle);
            }
            log(&format!("{version} is ready to install"));
            let update = StagedUpdate {
                channel,
                staged,
                sha256: target.sha256,
                version,
                executable,
            };
            let mut core = shared.lock();
            let kept = update.staged.update.clone();
            match core.adopt(update) {
                Some(discarded) if discarded.staged.update != kept => {
                    bundle::discard(&discarded.staged);
                }
                Some(_) | None => {}
            }
            if core.live() {
                bundle::remove_beside(installed, Some(&kept));
            }
            Ok(Phase::Idle)
        }
    }
}

fn updates_dir() -> Result<PathBuf, String> {
    let updates =
        crate::config::application_support_path("updates").ok_or("HOME is unavailable")?;
    std::fs::create_dir_all(&updates).map_err(|error| error.to_string())?;
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&updates, std::fs::Permissions::from_mode(0o700))
            .map_err(|error| error.to_string())?;
    }
    Ok(updates)
}

/// What this updater last installed: the release's target SHA-256 and
/// version, and its executable's hash, which says whether it is still the
/// build running.
#[derive(Debug, Eq, PartialEq, serde::Deserialize, serde::Serialize)]
struct Receipt {
    sha256: String,
    version: String,
    executable: String,
}

fn write_receipt(updates: &Path, update: &StagedUpdate) -> Result<(), String> {
    let receipt = Receipt {
        sha256: update.sha256.clone(),
        version: update.version.clone(),
        executable: update.executable.clone(),
    };
    let text = serde_json::to_string(&receipt).map_err(|error| error.to_string())?;
    let partial = updates.join(format!("{RECEIPT}.partial"));
    std::fs::write(&partial, text).map_err(|error| error.to_string())?;
    std::fs::rename(&partial, updates.join(RECEIPT)).map_err(|error| error.to_string())
}

fn running_receipt(updates: &Path) -> Option<Receipt> {
    static EXECUTABLE: std::sync::OnceLock<Option<String>> = std::sync::OnceLock::new();
    let running = EXECUTABLE.get_or_init(|| {
        std::env::current_exe()
            .ok()
            .and_then(|path| transport::sha256_hex(&path).ok())
    });
    receipt_for(updates, running.as_deref()?)
}

fn receipt_for(updates: &Path, executable: &str) -> Option<Receipt> {
    let text = std::fs::read_to_string(updates.join(RECEIPT)).ok()?;
    let receipt: Receipt = serde_json::from_str(&text).ok()?;
    (receipt.executable == executable && protocol::is_version_name(&receipt.version))
        .then_some(receipt)
}

/// This install's rollout bucket, 0–99, drawn once from `/dev/urandom`. It never
/// leaves the device.
fn rollout_bucket(updates: &Path) -> Result<u64, String> {
    let path = updates.join(BUCKET);
    if let Some(number) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| text.trim().parse::<u32>().ok())
    {
        return Ok(u64::from(number) % 100);
    }
    let mut bytes = [0; 4];
    std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").map_err(|error| error.to_string())?,
        &mut bytes,
    )
    .map_err(|error| error.to_string())?;
    let number = u32::from_le_bytes(bytes);
    std::fs::write(&path, format!("{number}\n")).map_err(|error| error.to_string())?;
    Ok(u64::from(number) % 100)
}

#[cfg(test)]
mod tests;
