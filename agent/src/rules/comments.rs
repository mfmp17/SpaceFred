//! Removing `[[rules]]` (or inline `rules = [...]`) entries without losing unrelated comments
//! set off by blank lines, such as section headings or commented-out rules. Comments placed
//! directly above a removed rule with no blank line stay attached to that rule and go with it.

use toml_edit::{Decor, Item, RawString, Table, Value};

use super::Doc;

impl Doc {
    pub fn remove(&mut self, index: usize) {
        self.remove_entry("rules", index);
    }

    pub(super) fn remove_entry(&mut self, section: &str, index: usize) {
        enum Removed {
            None,
            Table {
                position: Option<isize>,
                comment: Option<String>,
                emptied: bool,
            },
            Inline {
                comment: Option<String>,
                emptied: bool,
            },
        }
        let removed = match self.doc.get_mut(section) {
            Some(Item::ArrayOfTables(rules)) if index < rules.len() => {
                let (position, comment) = rules
                    .get(index)
                    .map(|table| {
                        (
                            table.position(),
                            table
                                .decor()
                                .prefix()
                                .and_then(RawString::as_str)
                                .and_then(detached_comments),
                        )
                    })
                    .unwrap_or((None, None));
                rules.remove(index);
                Removed::Table {
                    position,
                    comment,
                    emptied: rules.is_empty(),
                }
            }
            Some(Item::Value(Value::Array(rules))) if index < rules.len() => {
                let comment = rules
                    .get(index)
                    .and_then(|value| value.decor().prefix())
                    .and_then(RawString::as_str)
                    .and_then(detached_comments);
                rules.remove(index);
                if rules.is_empty() {
                    let trailing = rules
                        .trailing()
                        .as_str()
                        .map(trim_trailing_blank_lines)
                        .filter(|text| text.contains('#') && !text.is_empty())
                        .map(|text| {
                            if text.ends_with('\n') {
                                text.to_owned()
                            } else {
                                format!("{text}\n")
                            }
                        });
                    let combined = match (comment, trailing) {
                        (Some(a), Some(b)) => Some(join_comment_trailing(&a, &b)),
                        (Some(a), None) => Some(a),
                        (None, Some(b)) => Some(b),
                        (None, None) => None,
                    };
                    Removed::Inline {
                        comment: combined,
                        emptied: true,
                    }
                } else {
                    if let Some(comment) = comment {
                        if let Some(next) = rules.get_mut(index) {
                            prepend_decor_comment(next.decor_mut(), &comment);
                        } else {
                            let trailing = rules.trailing().as_str().unwrap_or_default().to_owned();
                            rules.set_trailing(join_comment_trailing(&comment, &trailing));
                        }
                    }
                    Removed::Inline {
                        comment: None,
                        emptied: false,
                    }
                }
            }
            _ => Removed::None,
        };
        match removed {
            Removed::Table {
                position,
                comment,
                emptied,
            } => {
                if emptied {
                    self.doc.remove(section);
                }
                if let Some(comment) = comment {
                    self.splice_table_comment(section, position, index, &comment);
                }
            }
            Removed::Inline {
                comment,
                emptied: true,
            } => {
                let key_comment = self
                    .doc
                    .key(section)
                    .and_then(|key| key.leaf_decor().prefix())
                    .and_then(RawString::as_str)
                    .and_then(detached_comments);
                let next_root_key = next_value_key_after(self.doc.as_table(), section);
                self.doc.remove(section);
                let combined = match (key_comment, comment) {
                    (Some(a), Some(b)) => Some(join_comment_prefix(&a, &b)),
                    (Some(a), None) => Some(a),
                    (None, Some(b)) => Some(b),
                    (None, None) => None,
                };
                if let Some(comment) = combined {
                    if let Some(next_key) = next_root_key
                        && let Some(mut key) = self.doc.key_mut(&next_key)
                    {
                        prepend_decor_comment(key.leaf_decor_mut(), &comment);
                    } else if let Some(first_pos) = min_table_position(self.doc.as_table(), None)
                        && let Some(target) =
                            find_table_at_position_mut(self.doc.as_table_mut(), first_pos)
                    {
                        prepend_decor_comment(target.decor_mut(), &comment);
                    } else {
                        let trailing = self.doc.trailing().as_str().unwrap_or_default().to_owned();
                        self.doc
                            .set_trailing(join_comment_trailing(&comment, &trailing));
                    }
                }
            }
            Removed::Inline { emptied: false, .. } | Removed::None => {}
        }
    }

    /// Removes top-level table `key` (and any subtables inside it), preserving detached
    /// comments set off by a blank line above any removed table header or scalar key.
    pub(super) fn remove_table(&mut self, key: &str) {
        let mut comments = Vec::new();
        let mut inline_comment = None;
        let next_root_key = next_value_key_after(self.doc.as_table(), key);
        match self.doc.get(key) {
            Some(Item::Table(table)) => {
                if table.is_dotted() {
                    inline_comment = self
                        .doc
                        .key(key)
                        .and_then(|k| k.leaf_decor().prefix())
                        .and_then(RawString::as_str)
                        .and_then(detached_comments);
                }
                collect_detached_table_comments(table, &mut comments);
            }
            Some(Item::Value(_)) => {
                inline_comment = self
                    .doc
                    .key(key)
                    .and_then(|k| k.leaf_decor().prefix())
                    .and_then(RawString::as_str)
                    .and_then(detached_comments);
            }
            _ => {}
        }
        self.doc.remove(key);
        self.splice_removed_table_comments(comments, inline_comment, next_root_key);
    }

    /// Removes scalar key `key_name` from the table at `path` (`["settings", "gaps"]` for
    /// `[settings.gaps]`), preserving any detached comment separated by a blank line above that
    /// key. Inline tables have no such comments to keep and just lose the key.
    pub(super) fn remove_key_in_table(&mut self, path: &[&str], key_name: &str) {
        let Some((first, rest)) = path.split_first() else {
            return;
        };
        let Some(mut item) = self.doc.as_table_mut().get_mut(first) else {
            return;
        };
        for name in rest {
            let Some(next) = item
                .as_table_like_mut()
                .and_then(|table| table.get_mut(name))
            else {
                return;
            };
            item = next;
        }
        let removed = match item.as_table_mut() {
            Some(table) => {
                let pos = table.position();
                remove_table_key(table, key_name).map(|comment| (pos, comment))
            }
            None => {
                if let Some(table) = item.as_table_like_mut() {
                    table.remove(key_name);
                }
                None
            }
        };
        if let Some((pos, comment)) = removed {
            self.splice_detached_after_table(pos, comment);
        }
    }

    /// Splices a detached comment that belonged to the end of the table at `table_position`
    /// onto the following table header, or into the document's trailing decor.
    pub(super) fn splice_detached_after_table(
        &mut self,
        table_position: Option<isize>,
        comment: String,
    ) {
        self.splice_removed_table_comments(vec![(table_position, comment)], None, None);
    }

    /// Removes nested table `child` from top-level table `parent`, preserving any detached
    /// comment heading set off by a blank line above the removed subtable header.
    pub(super) fn remove_subtable(&mut self, parent: &str, child: &str) {
        let mut comments = Vec::new();
        if let Some(table) = self.doc.get(parent).and_then(Item::as_table)
            && let Some(Item::Table(sub)) = table.get(child)
        {
            collect_detached_table_comments(sub, &mut comments);
        }
        if let Some(table) = self.doc.get_mut(parent).and_then(Item::as_table_like_mut) {
            table.remove(child);
        }
        self.splice_removed_table_comments(comments, None, None);
    }

    fn splice_removed_table_comments(
        &mut self,
        mut comments: Vec<(Option<isize>, String)>,
        inline_comment: Option<String>,
        next_root_key: Option<String>,
    ) {
        if let Some(comment) = inline_comment {
            if let Some(next_key) = next_root_key
                && let Some(mut key) = self.doc.key_mut(&next_key)
            {
                prepend_decor_comment(key.leaf_decor_mut(), &comment);
            } else if let Some(first_pos) = min_table_position(self.doc.as_table(), None)
                && let Some(target) = find_table_at_position_mut(self.doc.as_table_mut(), first_pos)
            {
                prepend_decor_comment(target.decor_mut(), &comment);
            } else {
                let trailing = self.doc.trailing().as_str().unwrap_or_default().to_owned();
                self.doc
                    .set_trailing(join_comment_trailing(&comment, &trailing));
            }
        }
        comments.sort_by_key(|(pos, _)| *pos);
        // Splice in reverse order so multiple detached headings prepended onto the
        // same following table keep their original top-to-bottom order.
        for (pos, comment) in comments.into_iter().rev() {
            if let Some(pos) = pos
                && let Some(target_pos) = min_table_position(self.doc.as_table(), Some(pos))
                && let Some(target) =
                    find_table_at_position_mut(self.doc.as_table_mut(), target_pos)
            {
                prepend_decor_comment(target.decor_mut(), &comment);
            } else {
                let trailing = self.doc.trailing().as_str().unwrap_or_default().to_owned();
                self.doc
                    .set_trailing(join_comment_trailing(&comment, &trailing));
            }
        }
    }

    fn splice_table_comment(
        &mut self,
        section: &str,
        removed_position: Option<isize>,
        index: usize,
        comment: &str,
    ) {
        if let Some(pos) = removed_position
            && let Some(target_pos) = min_table_position(self.doc.as_table(), Some(pos))
            && let Some(target) = find_table_at_position_mut(self.doc.as_table_mut(), target_pos)
        {
            prepend_decor_comment(target.decor_mut(), comment);
            return;
        }
        if let Some(Item::ArrayOfTables(entries)) = self.doc.get_mut(section)
            && let Some(next) = entries.get_mut(index)
        {
            prepend_decor_comment(next.decor_mut(), comment);
            return;
        }
        let trailing = self.doc.trailing().as_str().unwrap_or_default().to_owned();
        self.doc
            .set_trailing(join_comment_trailing(comment, &trailing));
    }
}

/// Removes `key_name` from `table`, preserving any detached comment set off by a blank line
/// above that key. If a following value key exists in `table`, the comment is prepended onto
/// it immediately; otherwise it is returned so the caller can attach it to a newly inserted key
/// or splice it after `table`.
pub(super) fn remove_table_key(table: &mut Table, key_name: &str) -> Option<String> {
    let detached = table
        .key(key_name)
        .and_then(|key| key.leaf_decor().prefix())
        .and_then(RawString::as_str)
        .and_then(detached_comments);
    let next_key = detached
        .as_ref()
        .and_then(|_| next_value_key_after(table, key_name));
    table.remove(key_name);
    let comment = detached?;
    if let Some(next_key) = next_key
        && let Some(mut key) = table.key_mut(&next_key)
    {
        prepend_decor_comment(key.leaf_decor_mut(), &comment);
        None
    } else {
        Some(comment)
    }
}

pub(super) fn merge_detached_comments(
    first: Option<String>,
    second: Option<String>,
) -> Option<String> {
    match (first, second) {
        (Some(a), Some(b)) => Some(join_comment_prefix(&a, &b)),
        (Some(a), None) => Some(a),
        (None, Some(b)) => Some(b),
        (None, None) => None,
    }
}

pub(super) fn prepend_key_comment(table: &mut Table, key_name: &str, comment: &str) {
    if let Some(mut key) = table.key_mut(key_name) {
        prepend_decor_comment(key.leaf_decor_mut(), comment);
    }
}

fn collect_detached_table_comments(table: &Table, out: &mut Vec<(Option<isize>, String)>) {
    if !table.is_implicit()
        && let Some(comment) = table
            .decor()
            .prefix()
            .and_then(RawString::as_str)
            .and_then(detached_comments)
    {
        out.push((table.position(), comment));
    }
    for (key, item) in table.iter() {
        match item {
            Item::Table(sub) => collect_detached_table_comments(sub, out),
            Item::ArrayOfTables(array) => {
                for sub in array.iter() {
                    collect_detached_table_comments(sub, out);
                }
            }
            Item::Value(_) => {
                if let Some(comment) = table
                    .key(key)
                    .and_then(|k| k.leaf_decor().prefix())
                    .and_then(RawString::as_str)
                    .and_then(detached_comments)
                {
                    out.push((table.position(), comment));
                }
            }
            Item::None => {}
        }
    }
}

/// Returns comments in `prefix` separated from the following item by at least one blank line.
/// Comments directly above the removed item (no blank line) are treated as attached to it.
fn detached_comments(prefix: &str) -> Option<String> {
    if !prefix.contains('#') {
        return None;
    }
    let mut last_blank_end = None;
    let mut offset = 0;
    for line in prefix.split_inclusive('\n') {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content.trim().is_empty() {
            last_blank_end = Some(offset + line.len());
        }
        offset += line.len();
    }
    let kept = trim_trailing_blank_lines(&prefix[..last_blank_end?]);
    (kept.contains('#') && !kept.is_empty()).then(|| kept.to_owned())
}

fn trim_trailing_blank_lines(s: &str) -> &str {
    let mut end = s.len();
    for line in s.split_inclusive('\n').rev() {
        let content = line.strip_suffix('\n').unwrap_or(line);
        let content = content.strip_suffix('\r').unwrap_or(content);
        if content.trim().is_empty() {
            end -= line.len();
        } else {
            break;
        }
    }
    &s[..end]
}

fn prepend_decor_comment(decor: &mut Decor, comment: &str) {
    let existing = decor
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or_default()
        .to_owned();
    decor.set_prefix(join_comment_prefix(comment, &existing));
}

fn join_comment_prefix(comment: &str, existing: &str) -> String {
    if existing.is_empty() {
        format!("{comment}\n")
    } else if existing.starts_with('\n') || existing.starts_with("\r\n") {
        format!("{comment}{existing}")
    } else {
        format!("{comment}\n{existing}")
    }
}

fn join_comment_trailing(comment: &str, existing: &str) -> String {
    if existing.is_empty() {
        comment.to_owned()
    } else if existing.starts_with('\n') || existing.starts_with("\r\n") {
        format!("{comment}{existing}")
    } else {
        format!("{comment}\n{existing}")
    }
}

fn min_table_position(table: &Table, min_exclusive: Option<isize>) -> Option<isize> {
    let mut best: Option<isize> = None;
    let mut consider = |candidate: Option<isize>| {
        if let Some(pos) = candidate
            && min_exclusive.is_none_or(|after| pos > after)
            && best.is_none_or(|current| pos < current)
        {
            best = Some(pos);
        }
    };
    for (_, item) in table.iter() {
        match item {
            Item::Table(sub) => {
                if !sub.is_implicit() {
                    consider(sub.position());
                }
                consider(min_table_position(sub, min_exclusive));
            }
            Item::ArrayOfTables(array) => {
                for sub in array.iter() {
                    consider(sub.position());
                    consider(min_table_position(sub, min_exclusive));
                }
            }
            _ => {}
        }
    }
    best
}

fn find_table_at_position_mut(table: &mut Table, target: isize) -> Option<&mut Table> {
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(sub) => {
                if !sub.is_implicit() && sub.position() == Some(target) {
                    return Some(sub);
                }
                if let Some(found) = find_table_at_position_mut(sub, target) {
                    return Some(found);
                }
            }
            Item::ArrayOfTables(array) => {
                for sub in array.iter_mut() {
                    if sub.position() == Some(target) {
                        return Some(sub);
                    }
                    if let Some(found) = find_table_at_position_mut(sub, target) {
                        return Some(found);
                    }
                }
            }
            _ => {}
        }
    }
    None
}

fn next_value_key_after(table: &Table, key_name: &str) -> Option<String> {
    let mut saw_key = false;
    for (key, item) in table.iter() {
        if saw_key && item.is_value() {
            return Some(key.to_owned());
        }
        if key == key_name {
            saw_key = true;
        }
    }
    None
}
