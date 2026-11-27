//! Read-modify-write edits to `config.toml`: locking, symlink-following
//! atomic saves, and the workspace display assignment editor.
use std::path::{Path, PathBuf};

use toml_edit::{ArrayOfTables, DocumentMut, InlineTable, Item, RawString, Table, Value, value};

use super::{Config, parse_path};
use crate::{DisplaySelector, WorkspaceId, Workspaces};

/// Exclusive advisory lock guarding read-modify-write updates to a config file.
///
/// Locks the resolved target's parent directory via `flock(2)` so atomic renames
/// of the config file never swap the locked inode and no sidecar lock file is
/// left behind (for example inside a dotfiles repository).
pub struct ConfigLock {
    _dir: std::fs::File,
}

/// Resolves symbolic links at `path` so config locking and atomic saves operate
/// on the target file instead of replacing the symbolic link itself.
pub fn resolve_write_target(path: &Path) -> std::io::Result<PathBuf> {
    const MAX_SYMLINK_DEPTH: usize = 40;
    match std::fs::symlink_metadata(path) {
        Ok(meta) if !meta.file_type().is_symlink() => return Ok(path.to_path_buf()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            return Ok(path.to_path_buf());
        }
        Err(error) => return Err(error),
        Ok(_) => {}
    }
    match std::fs::canonicalize(path) {
        Ok(resolved) => return Ok(resolved),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let mut current = path.to_path_buf();
    for _ in 0..MAX_SYMLINK_DEPTH {
        match std::fs::symlink_metadata(&current) {
            Ok(meta) if meta.file_type().is_symlink() => {
                let target = std::fs::read_link(&current)?;
                current = if target.is_absolute() {
                    target
                } else {
                    let parent = current
                        .parent()
                        .filter(|parent| !parent.as_os_str().is_empty())
                        .unwrap_or_else(|| Path::new("."));
                    let base =
                        std::fs::canonicalize(parent).unwrap_or_else(|_| parent.to_path_buf());
                    base.join(target)
                };
            }
            Ok(_) => return Ok(current),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(current),
            Err(error) => return Err(error),
        }
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!(
            "too many levels of symbolic links while resolving {}",
            path.display()
        ),
    ))
}

/// Acquires an exclusive advisory lock for editing the config file at `path`.
pub fn lock_config(path: &Path) -> std::io::Result<ConfigLock> {
    let target = resolve_write_target(path)?;
    let parent = target
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    std::fs::create_dir_all(parent)?;
    let dir = std::fs::File::open(parent)?;
    dir.lock()?;
    Ok(ConfigLock { _dir: dir })
}

/// Atomically replaces the config file at `path`, following symbolic links so a
/// symlinked `config.toml` stays linked to its updated target.
pub fn write_config_atomic(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let target = resolve_write_target(path)?;
    crate::state_file::write_atomic(&target, bytes)
}

/// Atomically updates `workspace`'s display assignment in `path`, clearing the
/// `[[workspaces]]` override when it returns to the built-in default (while
/// keeping named workspaces that have no route rule declared). Edits the file in
/// place, so comments and formatting elsewhere survive.
pub fn save_workspace_monitor_at(
    path: &Path,
    workspace: WorkspaceId,
    selector: &DisplaySelector,
) -> Result<Config, String> {
    let _lock =
        lock_config(path).map_err(|error| format!("failed to lock {}: {error}", path.display()))?;
    let source = match std::fs::read_to_string(path) {
        Ok(source) => {
            parse_path(path, &source)?;
            source
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => "version = 1\n".to_owned(),
        Err(error) => return Err(format!("failed to read {}: {error}", path.display())),
    };
    let mut doc = source
        .parse::<DocumentMut>()
        .map_err(|error| format!("{}: {error}", path.display()))?;
    let clear = *selector == Workspaces::default_assignment(workspace)
        && (workspace.number().is_some() || routes_to(&doc, workspace));
    set_workspace_monitor(&mut doc, workspace, (!clear).then(|| selector.as_str()));
    let rendered = doc.to_string();
    let parsed = Config::parse(&rendered)
        .map_err(|error| format!("refusing to write invalid config.toml: {error}"))?;
    write_config_atomic(path, rendered.as_bytes())
        .map_err(|error| format!("failed to write {}: {error}", path.display()))?;
    Ok(parsed)
}

/// Whether any `[[rules]]` entry names `workspace`.
fn routes_to(doc: &DocumentMut, workspace: WorkspaceId) -> bool {
    match doc.get("rules") {
        Some(Item::ArrayOfTables(rules)) => rules.iter().any(|rule| {
            rule.get("workspace")
                .and_then(Item::as_value)
                .and_then(workspace_of)
                == Some(workspace)
        }),
        Some(Item::Value(Value::Array(rules))) => rules.iter().any(|rule| {
            rule.as_inline_table()
                .and_then(|rule| rule.get("workspace"))
                .and_then(workspace_of)
                == Some(workspace)
        }),
        _ => false,
    }
}

fn workspace_of(item: &Value) -> Option<WorkspaceId> {
    match item {
        Value::Integer(number) => WorkspaceId::new(u8::try_from(*number.value()).ok()?).ok(),
        Value::String(name) => WorkspaceId::parse(name.value()).ok(),
        _ => None,
    }
}

fn workspace_value(workspace: WorkspaceId) -> Value {
    match workspace.number() {
        Some(number) => Value::from(i64::from(number)),
        None => Value::from(workspace.to_string()),
    }
}

/// Sets (`Some`) or removes (`None`) `workspace`'s entry in `workspaces`, in
/// either `[[workspaces]]` or inline `workspaces = [...]` form.
fn set_workspace_monitor(doc: &mut DocumentMut, workspace: WorkspaceId, monitor: Option<&str>) {
    if let Some(Item::Value(Value::Array(entries))) = doc.get_mut("workspaces") {
        let found = entries.iter().position(|entry| {
            entry
                .as_inline_table()
                .and_then(|table| table.get("workspace"))
                .and_then(workspace_of)
                == Some(workspace)
        });
        match (found, monitor) {
            (Some(index), Some(monitor)) => {
                if let Some(table) = entries.get_mut(index).and_then(Value::as_inline_table_mut) {
                    set_value(table, monitor);
                }
            }
            (Some(index), None) => {
                let comment =
                    comment_lines(entries.get(index).and_then(|entry| entry.decor().prefix()));
                entries.remove(index);
                if let Some(comment) = comment {
                    match entries.get_mut(index) {
                        Some(next) => prepend(next.decor_mut(), &comment),
                        None => {
                            let trailing = entries.trailing().as_str().unwrap_or_default();
                            entries.set_trailing(format!("{comment}{trailing}"));
                        }
                    }
                }
                if entries.is_empty() {
                    remove_inline_workspaces(doc);
                }
            }
            (None, Some(monitor)) => {
                let mut table = InlineTable::new();
                table.insert("workspace", workspace_value(workspace));
                table.insert("monitor", Value::from(monitor));
                table.fmt();
                entries.push(table);
            }
            (None, None) => {}
        }
        return;
    }
    if !doc.get("workspaces").is_some_and(Item::is_array_of_tables) {
        if monitor.is_none() {
            return;
        }
        doc.insert("workspaces", Item::ArrayOfTables(ArrayOfTables::new()));
    }
    let Some(Item::ArrayOfTables(tables)) = doc.get_mut("workspaces") else {
        return;
    };
    let found = tables.iter().position(|table| {
        table
            .get("workspace")
            .and_then(Item::as_value)
            .and_then(workspace_of)
            == Some(workspace)
    });
    match (found, monitor) {
        (Some(index), Some(monitor)) => {
            if let Some(table) = tables.get_mut(index) {
                set_value(table, monitor);
            }
        }
        (Some(index), None) => {
            let removed = tables
                .get(index)
                .map(|table| (table.position(), comment_lines(table.decor().prefix())));
            tables.remove(index);
            if tables.is_empty() {
                doc.remove("workspaces");
            }
            if let Some((position, Some(comment))) = removed {
                keep_comment(doc, position, &comment);
            }
        }
        (None, Some(monitor)) => {
            let mut table = Table::new();
            table.insert("workspace", value(workspace_value(workspace)));
            table.insert("monitor", value(monitor));
            tables.push(table);
        }
        (None, None) => {}
    }
}

/// Drops an emptied inline `workspaces = []`, moving the comments it still holds (a comment set
/// off above the key, and any left inside the brackets) to the next root key, the first table
/// header, or the end of the file, as Settings does.
fn remove_inline_workspaces(doc: &mut DocumentMut) {
    let above = comment_lines(
        doc.key("workspaces")
            .and_then(|key| key.leaf_decor().prefix()),
    );
    let inside = match doc.get("workspaces") {
        Some(Item::Value(Value::Array(entries))) => entries
            .trailing()
            .as_str()
            .filter(|text| text.contains('#'))
            .map(str::to_owned),
        _ => None,
    };
    let next_key = {
        let mut keys = doc
            .as_table()
            .iter()
            .map(|(key, item)| (key, item.is_value()));
        keys.by_ref().find(|(key, _)| *key == "workspaces");
        keys.find(|(_, is_value)| *is_value)
            .map(|(key, _)| key.to_owned())
    };
    doc.remove("workspaces");
    let comment = [above, inside]
        .into_iter()
        .flatten()
        .flat_map(|text| {
            text.lines()
                .map(|line| line.trim().to_owned())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>()
        .join("\n");
    let comment = comment.trim_matches(['\r', '\n']);
    if !comment.contains('#') {
        return;
    }
    let comment = format!("{comment}\n");
    if let Some(next) = next_key
        && let Some(mut key) = doc.key_mut(&next)
    {
        prepend(key.leaf_decor_mut(), &comment);
    } else if let Some(table) = next_table(doc.as_table_mut(), isize::MIN) {
        prepend(table.decor_mut(), &format!("\n{comment}"));
    } else {
        keep_comment(doc, None, &comment);
    }
}

/// Replaces `monitor`'s value, keeping the whitespace and trailing comment around it.
fn set_value(table: &mut dyn toml_edit::TableLike, monitor: &str) {
    let mut new = Value::from(monitor);
    match table.get_mut("monitor") {
        Some(Item::Value(old)) => {
            *new.decor_mut() = old.decor().clone();
            *old = new;
        }
        _ => {
            table.insert("monitor", Item::Value(new));
        }
    }
}

/// The comments in a removed entry's leading whitespace that a blank line sets off from it, as
/// Settings keeps them; a comment directly above the entry goes with it.
fn comment_lines(prefix: Option<&RawString>) -> Option<String> {
    let text = prefix?.as_str()?;
    let mut blank_end = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        // A whole line only: the indent before the entry itself isn't a blank line.
        if line.ends_with('\n') && line.trim().is_empty() {
            blank_end = Some(offset);
        }
    }
    let kept = text[..blank_end?].trim_end();
    kept.contains('#').then(|| format!("{kept}\n"))
}

fn prepend(decor: &mut toml_edit::Decor, comment: &str) {
    let existing = decor
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or_default();
    decor.set_prefix(format!("{comment}{existing}"));
}

/// Moves the comments above a removed `[[workspaces]]` table onto the table that
/// followed it, or to the end of the file.
fn keep_comment(doc: &mut DocumentMut, removed: Option<isize>, comment: &str) {
    if let Some(after) = removed
        && let Some(next) = next_table(doc.as_table_mut(), after)
    {
        prepend(next.decor_mut(), comment);
        return;
    }
    let trailing = doc.trailing().as_str().unwrap_or_default().to_owned();
    let comment = comment.trim_start_matches(['\r', '\n']);
    doc.set_trailing(format!("\n{comment}{trailing}"));
}

/// The header table that comes first after document position `after`.
fn next_table(table: &mut Table, after: isize) -> Option<&mut Table> {
    fn walk<'a>(table: &'a mut Table, after: isize, best: &mut Option<(isize, &'a mut Table)>) {
        for (_, item) in table.iter_mut() {
            let subs: Vec<&mut Table> = match item {
                Item::Table(sub) => vec![sub],
                Item::ArrayOfTables(array) => array.iter_mut().collect(),
                _ => continue,
            };
            for sub in subs {
                let position = (!sub.is_implicit()).then(|| sub.position()).flatten();
                match position {
                    Some(position)
                        if position > after
                            && best.as_ref().is_none_or(|(current, _)| position < *current) =>
                    {
                        *best = Some((position, sub));
                    }
                    _ => walk(sub, after, best),
                }
            }
        }
    }
    let mut best = None;
    walk(table, after, &mut best);
    best.map(|(_, table)| table)
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMENTED: &str = "# My Spaceman config.\nversion = 1\n\n[settings]\n# Columns on this Mac.\nlayout = \"columns\"\n\n# Disabled for now:\n# [[rules]]\n# bundle_id = \"com.apple.Safari\"\n# action = \"float\"\n";

    fn save(name: &str, source: &str, steps: &[(WorkspaceId, DisplaySelector)]) -> Vec<String> {
        let dir = std::env::temp_dir().join(format!(
            "spaceman-write-{name}-{}-{}",
            std::process::id(),
            super::super::now().as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(&path, source).unwrap();
        let saved = steps
            .iter()
            .map(|(workspace, selector)| {
                save_workspace_monitor_at(&path, *workspace, selector).unwrap();
                std::fs::read_to_string(&path).unwrap()
            })
            .collect();
        drop(std::fs::remove_dir_all(dir));
        saved
    }

    fn lost_comments<'a>(source: &'a str, saved: &str) -> Vec<&'a str> {
        source
            .lines()
            .filter(|line| line.trim_start().starts_with('#') && !saved.contains(line.trim()))
            .collect()
    }

    #[test]
    fn moving_a_workspace_and_back_keeps_comments() {
        let saved = save(
            "comments",
            COMMENTED,
            &[
                (WorkspaceId::ONE, DisplaySelector::Secondary),
                (WorkspaceId::ONE, DisplaySelector::Main),
            ],
        );
        assert!(
            lost_comments(COMMENTED, &saved[0]).is_empty(),
            "{}",
            saved[0]
        );
        assert!(
            saved[0].contains("[[workspaces]]\nworkspace = 1\nmonitor = \"secondary\"\n"),
            "{}",
            saved[0]
        );
        assert_eq!(saved[1], COMMENTED, "{:?}", saved[1]);
    }

    #[test]
    fn editing_an_existing_entry_keeps_its_comments() {
        let source = "version = 1\n\n# Office layout.\n\n# Left desk.\n[[workspaces]]\n# Left screen.\nworkspace = 2\nmonitor = \"secondary\" # for now\n\n# Notes.\n[[workspaces]]\nworkspace = 3\nmonitor = \"secondary\"\n";
        let saved = save(
            "existing",
            source,
            &[(WorkspaceId::TWO, DisplaySelector::parse("left"))],
        );
        assert!(
            saved[0].contains("monitor = \"left\" # for now"),
            "{}",
            saved[0]
        );
        assert!(lost_comments(source, &saved[0]).is_empty(), "{}", saved[0]);
        let saved = save(
            "removed",
            source,
            &[
                (WorkspaceId::TWO, DisplaySelector::Main),
                (WorkspaceId::THREE, DisplaySelector::Main),
            ],
        );
        // As in Settings: a comment set off by a blank line stays; one directly above a removed
        // entry, or inside it, goes with it.
        for line in ["# Office layout.", "# Notes."] {
            assert!(saved[0].contains(line), "{}", saved[0]);
        }
        for line in ["# Left desk.", "# Left screen."] {
            assert!(!saved[0].contains(line), "{}", saved[0]);
        }
        Config::parse(&saved[0]).unwrap();
        assert_eq!(
            saved[1], "version = 1\n\n# Office layout.\n",
            "{:?}",
            saved[1]
        );
    }

    #[test]
    fn inline_workspaces_keep_their_form() {
        let source = "version = 1\nworkspaces = [\n  # desks\n\n  # left desk\n  { workspace = 2, monitor = \"secondary\" },\n]\n";
        let saved = save(
            "inline",
            source,
            &[
                (WorkspaceId::THREE, DisplaySelector::Secondary),
                (WorkspaceId::TWO, DisplaySelector::Main),
            ],
        );
        assert!(
            saved[0].contains("{ workspace = 3, monitor = \"secondary\" }"),
            "{}",
            saved[0]
        );
        assert!(lost_comments(source, &saved[0]).is_empty(), "{}", saved[0]);
        assert!(!saved[1].contains("workspace = 2"), "{}", saved[1]);
        assert!(saved[1].contains("# desks"), "{}", saved[1]);
        assert!(!saved[1].contains("# left desk"), "{}", saved[1]);
        for saved in &saved {
            Config::parse(saved).unwrap();
        }
    }

    #[test]
    fn removing_the_last_inline_entry_keeps_comments_set_off_from_it() {
        let source = "version = 1\n\n# Display note.\n\nworkspaces = [\n  { workspace = 1, monitor = \"secondary\" },\n]\n";
        let saved = save(
            "inline-last",
            source,
            &[(WorkspaceId::ONE, DisplaySelector::Main)],
        );
        assert_eq!(
            saved[0], "version = 1\n\n# Display note.\n",
            "{:?}",
            saved[0]
        );
        Config::parse(&saved[0]).unwrap();

        // Comments left inside the brackets move out too, above the next table; one directly
        // above the removed entry goes with it.
        let source = "version = 1\n\n# Display note.\n\nworkspaces = [\n  # spare\n\n  # desk\n  { workspace = 1, monitor = \"secondary\" },\n  # later\n]\n\n[settings]\nlayout = \"columns\"\n";
        let saved = save(
            "inline-table",
            source,
            &[(WorkspaceId::ONE, DisplaySelector::Main)],
        );
        assert!(!saved[0].contains("workspaces"), "{}", saved[0]);
        assert!(!saved[0].contains("# desk"), "{}", saved[0]);
        for line in ["# Display note.", "# spare", "# later"] {
            assert!(saved[0].contains(&format!("{line}\n")), "{}", saved[0]);
        }
        assert!(
            saved[0].find("# later") < saved[0].find("[settings]"),
            "{}",
            saved[0]
        );
        let config = Config::parse(&saved[0]).unwrap();
        assert_eq!(config.layout(), crate::config::LayoutMode::Columns);
    }

    #[test]
    fn a_named_workspace_without_a_route_keeps_its_entry() {
        let a = WorkspaceId::parse("A").unwrap();
        let source = "version = 1\n\n[[workspaces]]\nworkspace = \"A\"\nmonitor = \"secondary\"\n";
        let saved = save("named", source, &[(a, DisplaySelector::Main)]);
        assert!(
            saved[0].contains("workspace = \"A\"\nmonitor = \"main\""),
            "{}",
            saved[0]
        );
        let routed = format!(
            "{source}\n[[rules]]\nbundle_id = \"x\"\naction = \"route\"\nworkspace = \"A\"\n"
        );
        let saved = save("routed", &routed, &[(a, DisplaySelector::Main)]);
        assert!(!saved[0].contains("[[workspaces]]"), "{}", saved[0]);
    }

    #[test]
    fn saving_a_workspace_monitor_keeps_title_contains_rules() {
        let nanos = crate::config::now().as_nanos();
        let dir = std::env::temp_dir().join(format!(
            "spaceman-config-title-contains-{}-{nanos}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        std::fs::write(
            &path,
            "version = 1\n[[rules]]\nbundle_id = \"com.apple.Safari\"\ntitle_contains = \"Work\"\naction = \"float\"\n",
        )
        .unwrap();
        save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("title_contains = \"Work\""));
        assert!(saved.contains("monitor = \"secondary\""));

        std::fs::write(&path, include_str!("../../config.example.toml")).unwrap();
        save_workspace_monitor_at(&path, WorkspaceId::ONE, &DisplaySelector::Secondary).unwrap();
        let saved = std::fs::read_to_string(&path).unwrap();
        assert!(saved.contains("title_contains = \"Work —\""));
        drop(std::fs::remove_dir_all(dir));
    }

    #[test]
    fn config_example_round_trips_keeping_every_comment_and_commented_rules() {
        let example = include_str!("../../config.example.toml");
        let saved = save(
            "example-round-trip",
            example,
            &[
                (WorkspaceId::ONE, DisplaySelector::Secondary),
                (WorkspaceId::ONE, DisplaySelector::Main),
            ],
        );
        for text in &saved {
            assert!(lost_comments(example, text).is_empty(), "{text}");
            assert!(text.contains("# [shortcuts]"), "{text}");
            assert!(text.contains("# [labs]"), "{text}");
            Config::parse(text).unwrap();
        }
        assert!(
            saved[0].contains("workspace = 1\nmonitor = \"secondary\"\n"),
            "{}",
            saved[0]
        );
        assert!(
            !saved[1].contains("workspace = 1\nmonitor = \"secondary\""),
            "{}",
            saved[1]
        );
        assert_eq!(saved[1], example);
    }
}
