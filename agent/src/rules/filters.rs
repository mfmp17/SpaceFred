//! Rules for some of an app's windows: adding a rule with a `title_contains` filter, changing
//! or clearing one, and keeping an app's filtered rules ahead of its every-window rule. The first
//! matching rule wins, so a filtered rule written after the every-window one would never apply.
//!
//! A filter is only ever the text someone typed in Settings or in config.toml. Nothing here
//! reads a live window title.

use std::cmp::Ordering;

use toml_edit::{InlineTable, Item, Key, Table, Value, value};

use super::{Doc, Place, Rule, push_inline_table, set_place};

const CHANGED_EXTERNALLY: &str =
    "config.toml changed externally while editing this rule; reloaded current settings";

impl Doc {
    /// Sends `bundle_id`'s windows to `place`: every window, or with `filter` only those whose title
    /// contains it. A rule with the same app and filter is moved instead of doubled, since only
    /// the first of the two could ever match.
    pub fn add_rule(&mut self, bundle_id: &str, filter: Option<&str>, place: Place) {
        if let Some(existing) = self.rule_matching(bundle_id, filter, None) {
            self.place(existing.index, place);
            return;
        }
        if let Some(Item::Value(Value::Array(rules))) = self.doc.get_mut("rules") {
            let mut table = InlineTable::new();
            table.insert("bundle_id", Value::from(bundle_id));
            if let Some(filter) = filter {
                table.insert("title_contains", Value::from(filter));
            }
            set_place(&mut table, place);
            push_inline_table(rules, table);
        } else {
            let mut table = Table::new();
            table.insert("bundle_id", value(bundle_id));
            if let Some(filter) = filter {
                table.insert("title_contains", value(filter));
            }
            set_place(&mut table, place);
            self.rule_tables().push(table);
        }
        self.settle_order(bundle_id);
    }

    /// `bundle_id`'s rule with exactly this title filter (`None`: its every-window rule), other
    /// than the rule at `except`. Filters compare as they match: without regard to case.
    pub fn rule_matching(
        &self,
        bundle_id: &str,
        filter: Option<&str>,
        except: Option<usize>,
    ) -> Option<Rule> {
        self.rules().into_iter().find(|rule| {
            Some(rule.index) != except
                && rule.bundle_id.as_deref() == Some(bundle_id)
                && rule.title.is_none()
                && same_filter(rule.title_contains.as_deref(), filter)
        })
    }

    /// Sets or clears the title filter of the rule matching `expected`, or reports a conflict
    /// if an external edit modified, removed, or ambiguously shifted it.
    pub fn set_title_contains_matching(
        &mut self,
        expected: &Rule,
        filter: Option<&str>,
    ) -> Result<(), String> {
        let index = self
            .resolve_rule_index(expected)
            .ok_or_else(|| CHANGED_EXTERNALLY.to_owned())?;
        match self.doc.get_mut("rules") {
            Some(Item::ArrayOfTables(rules)) => {
                if let Some(table) = rules.get_mut(index) {
                    match filter {
                        Some(filter) => {
                            table.insert("title_contains", value(filter));
                            table.sort_values_by(|a, _, b, _| key_order(a, b));
                        }
                        None => drop(table.remove("title_contains")),
                    }
                }
            }
            Some(Item::Value(Value::Array(rules))) => {
                if let Some(table) = rules.get_mut(index).and_then(Value::as_inline_table_mut) {
                    match filter {
                        Some(filter) => {
                            table.insert("title_contains", Value::from(filter));
                            table.sort_values_by(|a, _, b, _| key_order(a, b));
                            table.fmt();
                        }
                        None => drop(table.remove("title_contains")),
                    }
                }
            }
            _ => {}
        }
        if let Some(bundle_id) = &expected.bundle_id {
            self.settle_order(bundle_id);
        }
        Ok(())
    }

    /// Moves `bundle_id`'s every-window rule behind its last filtered one, when it sits ahead.
    fn settle_order(&mut self, bundle_id: &str) {
        // Each pass moves one every-window rule, so the rule count bounds the passes.
        for _ in 0..self.rules().len() {
            let rules: Vec<Rule> = self
                .rules()
                .into_iter()
                .filter(|rule| rule.bundle_id.as_deref() == Some(bundle_id))
                .collect();
            let Some(last_filtered) = rules.iter().rev().find(|rule| !rule.app_wide()) else {
                return;
            };
            let Some(wide) = rules
                .iter()
                .find(|rule| rule.app_wide() && rule.index < last_filtered.index)
            else {
                return;
            };
            self.move_rule_after(wide.index, last_filtered.index);
        }
    }

    /// Moves the rule at `from` to just behind the later rule at `after`, with its comments.
    fn move_rule_after(&mut self, from: usize, after: usize) {
        if from >= after {
            return;
        }
        match self.doc.get_mut("rules") {
            Some(Item::ArrayOfTables(rules)) if after < rules.len() => {
                // An array of tables has no insert: its tail comes off and goes back in the new
                // order. The moved table gives up its place in the file, so it is written
                // straight after the table now ahead of it.
                let mut tail: Vec<Table> = (from..rules.len())
                    .rev()
                    .map(|index| rules.remove(index))
                    .collect();
                tail.reverse();
                let moved = unpositioned(&tail.remove(0));
                tail.insert(after - from, moved);
                for table in tail {
                    rules.push(table);
                }
            }
            Some(Item::Value(Value::Array(rules))) if after < rules.len() => {
                let moved = rules.remove(from);
                rules.insert_formatted(after, moved);
            }
            _ => {}
        }
    }
}

fn same_filter(left: Option<&str>, right: Option<&str>) -> bool {
    match (left, right) {
        (None, None) => true,
        (Some(left), Some(right)) => left.to_lowercase() == right.to_lowercase(),
        _ => false,
    }
}

/// A rule's keys in the order config.toml documents them; any other key keeps its place after.
fn key_order(left: &Key, right: &Key) -> Ordering {
    let rank = |key: &Key| {
        [
            "bundle_id",
            "title",
            "title_contains",
            "action",
            "workspace",
        ]
        .iter()
        .position(|name| *name == key.get())
        .unwrap_or(usize::MAX)
    };
    rank(left).cmp(&rank(right))
}

/// `table` with its keys, values and comments but no place in the file, so it is written after
/// whichever table precedes it in its array.
fn unpositioned(table: &Table) -> Table {
    let mut fresh = Table::new();
    *fresh.decor_mut() = table.decor().clone();
    for (name, _) in table.iter() {
        if let Some((key, item)) = table.get_key_value(name) {
            fresh.insert_formatted(key, item.clone());
        }
    }
    fresh
}

#[cfg(test)]
#[path = "filters_tests.rs"]
mod tests;
