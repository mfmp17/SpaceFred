//! Writing a CLI edit back into `config.toml` without reformatting the file.
//!
//! The CLI edits a typed copy of the config and renders it fresh, which drops every comment and
//! reorders keys. [`merge_into`] instead applies the difference between the file and that rendering
//! to the file's own syntax tree: keys and entries the edit did not change keep their text,
//! comments, and order; changed values keep the comments around them; removed keys and entries go
//! with the comments attached to them; new ones are added beside their siblings. A comment a
//! blank line sets off from a removed key or entry is not attached to it: it stays, moved onto
//! what follows (the next key, the next table, or the end of the file), as Settings does.

use toml_edit::{
    Array, ArrayOfTables, Decor, DocumentMut, InlineTable, Item, RawString, Table, TableLike, Value,
};

/// Comments set off from removed keys and entries, each with the document position of the table
/// header it sat under (`None` before every header), waiting to move onto the next table.
type Kept = Vec<(Option<isize>, String)>;

/// The `source` file with `rendered`'s content, or `None` when either fails to parse.
pub(crate) fn merge_into(source: &str, rendered: &str) -> Option<String> {
    let mut doc = source.parse::<DocumentMut>().ok()?;
    let target = rendered.parse::<DocumentMut>().ok()?;
    let mut kept = Kept::new();
    merge_table(doc.as_table_mut(), target.as_table(), None, &mut kept);
    restore(&mut doc, kept);
    Some(doc.to_string())
}

/// `source` without the entries of its `key` array at the zero-based `positions`, each with the
/// comments attached to it, or `None` when `key` is not an array or a position is past its end.
///
/// [`merge_into`] matches entries by value, so it cannot tell which of two equal entries an
/// edit removed. An edit that removes entries by position drops them here first; the merge then
/// pairs the rest one to one.
pub(crate) fn remove_entries(source: &str, key: &str, positions: &[usize]) -> Option<String> {
    let mut doc = source.parse::<DocumentMut>().ok()?;
    let mut positions = positions.to_vec();
    positions.sort_unstable();
    positions.dedup();
    let mut kept = Kept::new();
    match doc.as_table_mut().get_mut(key)? {
        Item::ArrayOfTables(tables) => {
            if positions.last().is_some_and(|&last| last >= tables.len()) {
                return None;
            }
            for &position in &positions {
                keep_entry_comment(tables.get(position)?, &mut kept);
            }
            for &position in positions.iter().rev() {
                tables.remove(position);
            }
        }
        Item::Value(Value::Array(values)) => {
            if positions.last().is_some_and(|&last| last >= values.len()) {
                return None;
            }
            let steps = values
                .iter()
                .enumerate()
                .filter(|(index, _)| positions.binary_search(index).is_err())
                .map(|(index, value)| (Some(index), value.clone()))
                .collect();
            rebuild_array(values, steps);
        }
        _ => return None,
    }
    restore(&mut doc, kept);
    Some(doc.to_string())
}

/// `header` is the document position of the table header `old`'s keys print under.
fn merge_table(old: &mut Table, new: &Table, header: Option<isize>, kept: &mut Kept) {
    let stale: Vec<String> = old
        .iter()
        .filter(|(key, _)| new.get(key).is_none_or(Item::is_none))
        .map(|(key, _)| key.to_owned())
        .collect();
    // Each removed value key's set-off comment, grouped by the surviving value key that follows.
    let mut moves: Vec<(Option<String>, String)> = Vec::new();
    for key in &stale {
        let Some(item) = old.get(key) else { continue };
        if !item.is_value() {
            keep_removed(item, header, kept);
            continue;
        }
        let Some(comment) = old
            .key(key)
            .and_then(|name| set_off(name.leaf_decor().prefix()))
        else {
            continue;
        };
        let next = old
            .iter()
            .skip_while(|(name, _)| *name != key.as_str())
            .skip(1)
            .find(|(name, item)| {
                item.is_value() && !stale.iter().any(|gone| gone.as_str() == *name)
            })
            .map(|(name, _)| name.to_owned());
        if let Some((target, text)) = moves.last_mut()
            && *target == next
        {
            push_comment(text, &comment);
        } else {
            moves.push((next, comment));
        }
    }
    for key in &stale {
        old.remove(key);
    }
    for (next, comment) in moves {
        if let Some(next) = next
            && let Some(mut name) = old.key_mut(&next)
        {
            prepend(name.leaf_decor_mut(), &comment);
        } else {
            kept.push((header, comment));
        }
    }
    for (key, item) in new.iter() {
        if item.is_none() {
            continue;
        }
        match old.get_mut(key) {
            Some(existing) => merge_item(existing, item, header, kept),
            None => {
                if item.is_value() && old.is_implicit() && !old.is_dotted() {
                    old.set_implicit(false);
                }
                old.insert(key, detach(item));
            }
        }
    }
}

fn merge_item(old: &mut Item, new: &Item, header: Option<isize>, kept: &mut Kept) {
    if same_item(old, new) {
        return;
    }
    match (old, new) {
        (Item::Table(old), Item::Table(new)) => {
            let header = header_position(old).or(header);
            merge_table(old, new, header, kept);
        }
        (Item::Table(old), Item::Value(Value::InlineTable(new))) => {
            let header = header_position(old).or(header);
            merge_table(old, &new.clone().into_table(), header, kept);
        }
        (Item::ArrayOfTables(old), Item::ArrayOfTables(new)) => {
            merge_tables(old, new, header, kept);
        }
        (Item::Value(old), Item::Value(new)) => merge_value(old, new),
        (Item::Value(old), Item::Table(new)) if old.is_inline_table() => {
            merge_value(old, &Value::InlineTable(new.clone().into_inline_table()));
        }
        (Item::Value(old), Item::ArrayOfTables(new)) if old.is_array() => {
            merge_value(old, &Value::Array(new.clone().into_array()));
        }
        (old, new) => *old = detach(new),
    }
}

fn merge_tables(
    old: &mut ArrayOfTables,
    new: &ArrayOfTables,
    header: Option<isize>,
    kept: &mut Kept,
) {
    let olds: Vec<&Table> = old.iter().collect();
    let news: Vec<&Table> = new.iter().collect();
    let plan = align(olds.len(), news.len(), |i, j| same_tables(olds[i], news[j]));
    let mut merged = ArrayOfTables::new();
    let mut next_old = 0;
    for (from, j) in plan {
        merged.push(match from {
            Some(i) => {
                for gone in &olds[next_old..i] {
                    keep_entry_comment(gone, kept);
                }
                next_old = i + 1;
                let mut table = olds[i].clone();
                let header = header_position(&table).or(header);
                merge_table(&mut table, news[j], header, kept);
                table
            }
            None => detach_table(news[j]),
        });
    }
    for gone in &olds[next_old..] {
        keep_entry_comment(gone, kept);
    }
    *old = merged;
}

fn merge_value(old: &mut Value, new: &Value) {
    if same_value(old, new) {
        return;
    }
    match (old, new) {
        (Value::InlineTable(old), Value::InlineTable(new)) => merge_inline(old, new),
        (Value::Array(old), Value::Array(new)) => merge_array(old, new),
        (old, new) => {
            let decor = old.decor().clone();
            *old = new.clone();
            *old.decor_mut() = decor;
        }
    }
}

fn merge_inline(old: &mut InlineTable, new: &InlineTable) {
    let stale: Vec<String> = old
        .iter()
        .filter(|(key, _)| new.get(key).is_none())
        .map(|(key, _)| key.to_owned())
        .collect();
    for key in stale {
        old.remove(&key);
    }
    for (key, value) in new.iter() {
        match old.get_mut(key) {
            Some(existing) => merge_value(existing, value),
            None => {
                old.insert(key, value.clone());
            }
        }
    }
}

fn merge_array(old: &mut Array, new: &Array) {
    let olds: Vec<&Value> = old.iter().collect();
    let news: Vec<&Value> = new.iter().collect();
    let plan = align(olds.len(), news.len(), |i, j| same_value(olds[i], news[j]));
    let steps = plan
        .into_iter()
        .map(|(from, j)| match from {
            Some(i) => {
                let mut value = olds[i].clone();
                merge_value(&mut value, news[j]);
                (Some(i), value)
            }
            None => (None, news[j].clone()),
        })
        .collect();
    rebuild_array(old, steps);
}

/// Replaces `old`'s elements with `steps`, each the old element it continues (in order), if any,
/// and its value. An old element no step continues leaves the array; a comment set off above it
/// moves onto the next element that stays, or before the closing bracket.
fn rebuild_array(old: &mut Array, steps: Vec<(Option<usize>, Value)>) {
    let mut merged = Array::new();
    let mut pending = String::new();
    let mut next_old = 0;
    for (from, mut value) in steps {
        if let Some(i) = from {
            for gone in old.iter().take(i).skip(next_old) {
                if let Some(comment) = set_off(gone.decor().prefix()) {
                    push_comment(&mut pending, &comment);
                }
            }
            next_old = i + 1;
            if !pending.is_empty() {
                prepend(value.decor_mut(), &pending);
                pending.clear();
            }
        }
        merged.push_formatted(value);
    }
    for gone in old.iter().skip(next_old) {
        if let Some(comment) = set_off(gone.decor().prefix()) {
            push_comment(&mut pending, &comment);
        }
    }
    if pending.is_empty() {
        merged.set_trailing(old.trailing().clone());
    } else {
        let trailing = old.trailing().as_str().unwrap_or_default();
        let comment = if trailing.starts_with(['\r', '\n']) {
            pending.trim_end()
        } else {
            pending.as_str()
        };
        merged.set_trailing(format!("{comment}{trailing}"));
    }
    merged.set_trailing_comma(old.trailing_comma());
    *merged.decor_mut() = old.decor().clone();
    *old = merged;
}

/// The position of the header `table` prints under its own name, if it has one.
fn header_position(table: &Table) -> Option<isize> {
    (!table.is_implicit() && !table.is_dotted())
        .then(|| table.position())
        .flatten()
}

/// Keeps the comments set off inside a removed table or array of tables: above its header, and
/// above each of its keys (a removed `[[entry]]`'s own keys go with it).
fn keep_removed(item: &Item, header: Option<isize>, kept: &mut Kept) {
    match item {
        Item::Table(table) => {
            let position = header_position(table);
            if position.is_some()
                && let Some(comment) = set_off(table.decor().prefix())
            {
                kept.push((position, comment));
            }
            let header = position.or(header);
            for (key, item) in table.iter() {
                if !item.is_value() {
                    keep_removed(item, header, kept);
                } else if let Some(comment) = table
                    .key(key)
                    .and_then(|name| set_off(name.leaf_decor().prefix()))
                {
                    kept.push((header, comment));
                }
            }
        }
        Item::ArrayOfTables(tables) => {
            for table in tables.iter() {
                keep_entry_comment(table, kept);
            }
        }
        _ => {}
    }
}

/// Keeps the comment set off above a removed `[[entry]]` header.
fn keep_entry_comment(table: &Table, kept: &mut Kept) {
    if let Some(comment) = set_off(table.decor().prefix()) {
        kept.push((table.position(), comment));
    }
}

/// Moves each kept comment onto the first table header after the one it sat under, or to the end
/// of the file, in document order.
fn restore(doc: &mut DocumentMut, mut kept: Kept) {
    kept.sort_by_key(|(position, _)| position.unwrap_or(isize::MIN));
    for (position, comment) in kept.into_iter().rev() {
        if let Some(table) = next_table(doc.as_table_mut(), position.unwrap_or(isize::MIN)) {
            prepend(table.decor_mut(), &comment);
        } else {
            let trailing = doc.trailing().as_str().unwrap_or_default();
            let trailing = format!("{comment}{trailing}");
            doc.set_trailing(trailing);
        }
    }
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
                match header_position(sub) {
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

/// The comment lines in a removed item's leading whitespace that a blank line sets off from it,
/// ending in a newline; the comment directly above the item is attached to it and goes with it.
fn set_off(prefix: Option<&RawString>) -> Option<String> {
    let text = prefix?.as_str()?;
    let mut blank_end = None;
    let mut offset = 0;
    for line in text.split_inclusive('\n') {
        offset += line.len();
        // A whole line only: the indent before the item itself isn't a blank line.
        if line.ends_with('\n') && line.trim().is_empty() {
            blank_end = Some(offset);
        }
    }
    let kept = text[..blank_end?].trim_end();
    kept.contains('#').then(|| format!("{kept}\n"))
}

fn push_comment(text: &mut String, comment: &str) {
    if !text.is_empty() {
        text.push('\n');
    }
    text.push_str(comment);
}

/// Puts `comment` above what `decor` leads, a blank line still setting it off.
fn prepend(decor: &mut Decor, comment: &str) {
    let existing = decor
        .prefix()
        .and_then(RawString::as_str)
        .unwrap_or_default();
    let prefix = if existing.starts_with(['\r', '\n']) {
        format!("{comment}{existing}")
    } else {
        format!("{comment}\n{existing}")
    };
    decor.set_prefix(prefix);
}

/// For each new element in order, the old element it continues, if any. Equal lengths pair
/// elements by position (an edit in place); otherwise a longest common subsequence of equal
/// elements keeps the untouched entries and their comments around an insertion or removal.
fn align(
    old_len: usize,
    new_len: usize,
    same: impl Fn(usize, usize) -> bool,
) -> Vec<(Option<usize>, usize)> {
    if old_len == new_len {
        return (0..new_len).map(|j| (Some(j), j)).collect();
    }
    let width = new_len + 1;
    let mut longest = vec![0_usize; (old_len + 1) * width];
    for i in (0..old_len).rev() {
        for j in (0..new_len).rev() {
            longest[i * width + j] = if same(i, j) {
                longest[(i + 1) * width + j + 1] + 1
            } else {
                longest[(i + 1) * width + j].max(longest[i * width + j + 1])
            };
        }
    }
    let (mut i, mut j) = (0, 0);
    let mut plan = Vec::with_capacity(new_len);
    while j < new_len {
        if i < old_len && same(i, j) {
            plan.push((Some(i), j));
            i += 1;
            j += 1;
        } else if i < old_len && longest[(i + 1) * width + j] >= longest[i * width + j + 1] {
            i += 1;
        } else {
            plan.push((None, j));
            j += 1;
        }
    }
    plan
}

/// A copy of `item` whose tables carry no position from the document they came from, so
/// they print beside their siblings in the document they join.
fn detach(item: &Item) -> Item {
    match item {
        Item::Table(table) => Item::Table(detach_table(table)),
        Item::ArrayOfTables(tables) => {
            let mut out = ArrayOfTables::new();
            for table in tables.iter() {
                out.push(detach_table(table));
            }
            Item::ArrayOfTables(out)
        }
        other => other.clone(),
    }
}

fn detach_table(table: &Table) -> Table {
    let mut out = Table::new();
    out.set_implicit(table.is_implicit());
    out.set_dotted(table.is_dotted());
    for (key, item) in table.iter() {
        out.insert(key, detach(item));
    }
    out
}

fn same_item(a: &Item, b: &Item) -> bool {
    match (a, b) {
        (Item::None, Item::None) => true,
        (Item::Value(a), Item::Value(b)) => same_value(a, b),
        _ => match (a.as_table_like(), b.as_table_like()) {
            (Some(a), Some(b)) => same_tables(a, b),
            _ => match (table_entries(a), table_entries(b)) {
                (Some(a), Some(b)) => {
                    a.len() == b.len() && a.iter().zip(&b).all(|(a, b)| same_tables(*a, *b))
                }
                _ => false,
            },
        },
    }
}

fn table_entries(item: &Item) -> Option<Vec<&dyn TableLike>> {
    match item {
        Item::ArrayOfTables(tables) => {
            Some(tables.iter().map(|table| table as &dyn TableLike).collect())
        }
        Item::Value(Value::Array(values)) => values
            .iter()
            .map(|value| value.as_inline_table().map(|table| table as &dyn TableLike))
            .collect(),
        _ => None,
    }
}

fn same_tables(a: &dyn TableLike, b: &dyn TableLike) -> bool {
    a.len() == b.len()
        && a.iter()
            .all(|(key, a)| b.get(key).is_some_and(|b| same_item(a, b)))
}

fn same_value(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::String(a), Value::String(b)) => a.value() == b.value(),
        (Value::Integer(a), Value::Integer(b)) => a.value() == b.value(),
        (Value::Float(a), Value::Float(b)) => a.value().to_bits() == b.value().to_bits(),
        (Value::Boolean(a), Value::Boolean(b)) => a.value() == b.value(),
        (Value::Datetime(a), Value::Datetime(b)) => a.value() == b.value(),
        (Value::Array(a), Value::Array(b)) => {
            a.len() == b.len() && a.iter().zip(b.iter()).all(|(a, b)| same_value(a, b))
        }
        (Value::InlineTable(a), Value::InlineTable(b)) => same_tables(a, b),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::merge_into;

    #[test]
    fn keeps_comments_and_order_around_a_removed_key() {
        let source = "# Top.\nversion = 1\n\n[settings]\n# Columns.\nlayout = \"columns\"\nhud_position = \"top\"\n\n# Disabled:\n# [[rules]]\n";
        let rendered = "version = 1\n\n[settings]\nlayout = \"columns\"\n";
        assert_eq!(
            merge_into(source, rendered).unwrap(),
            "# Top.\nversion = 1\n\n[settings]\n# Columns.\nlayout = \"columns\"\n\n# Disabled:\n# [[rules]]\n"
        );
    }

    #[test]
    fn changes_a_value_in_place_and_leaves_an_unchanged_file_alone() {
        let source = "version = 1\n[settings]\nlayout = 'dwindle' # mine\nhud = false\n";
        let rendered = "version = 1\n\n[settings]\nhud = false\nlayout = \"columns\"\n";
        assert_eq!(
            merge_into(source, rendered).unwrap(),
            "version = 1\n[settings]\nlayout = \"columns\" # mine\nhud = false\n"
        );
        let same = "version = 1\n\n[settings]\nhud = false\nlayout = 'dwindle'\n";
        assert_eq!(merge_into(source, same).unwrap(), source);
    }

    #[test]
    fn removes_and_adds_array_entries_without_touching_the_others() {
        let source = "version = 1\n\n# One.\n[[rules]]\nbundle_id = \"a\"\naction = \"float\"\n\n# Two.\n[[rules]]\nbundle_id = \"b\"\naction = \"float\"\n\n# Trailing note.\n";
        let removed = "version = 1\n\n[[rules]]\nbundle_id = \"b\"\naction = \"float\"\n";
        assert_eq!(
            merge_into(source, removed).unwrap(),
            "version = 1\n\n# Two.\n[[rules]]\nbundle_id = \"b\"\naction = \"float\"\n\n# Trailing note.\n"
        );
        let added = "version = 1\n\n[[rules]]\nbundle_id = \"a\"\naction = \"float\"\n\n[[rules]]\nbundle_id = \"b\"\naction = \"float\"\n\n[[rules]]\nbundle_id = \"c\"\naction = \"float\"\n";
        let merged = merge_into(source, added).unwrap();
        assert!(
            merged.contains("# One.\n[[rules]]\nbundle_id = \"a\""),
            "{merged}"
        );
        assert!(
            merged.contains("# Two.\n[[rules]]\nbundle_id = \"b\""),
            "{merged}"
        );
        assert!(merged.contains("# Trailing note."), "{merged}");
        assert!(
            merged.find("bundle_id = \"b\"") < merged.find("bundle_id = \"c\""),
            "{merged}"
        );
    }
}
