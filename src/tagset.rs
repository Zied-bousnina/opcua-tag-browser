//! An indexed collection of scanned tags.

use crate::tag::PlcTag;
use std::collections::HashMap;
use std::ops::Deref;

/// A scanned tag list, indexed for lookup by browse path and display name.
///
/// A large server yields thousands of tags, and a linear scan for each lookup
/// adds up. This builds the indexes once.
///
/// Dereferences to `[PlcTag]`, so iteration and slicing work as on a `Vec`.
///
/// ```
/// use opcua_tag_browser::{PlcTag, TagSet};
///
/// let tags = TagSet::new(vec![
///     PlcTag::new("Speed", "Speed", "ns=2;s=speed", "Variable", "Machine/Axis1/Speed"),
///     PlcTag::new("Position", "Position", "ns=2;s=pos", "Variable", "Machine/Axis1/Position"),
/// ]);
///
/// assert_eq!(tags.len(), 2);
/// assert!(tags.find("Machine/Axis1/Speed").is_some());
/// assert!(tags.find("Speed").is_some());
/// assert_eq!(tags.matching("Machine/*/Speed").len(), 1);
/// ```
#[derive(Debug, Clone, Default)]
pub struct TagSet {
    tags: Vec<PlcTag>,
    by_path: HashMap<String, usize>,
    by_name: HashMap<String, usize>,
}

impl TagSet {
    /// Builds an indexed set from a tag list.
    ///
    /// Where several tags share a display name, the first wins for name lookup;
    /// browse paths are unique by construction.
    pub fn new(tags: Vec<PlcTag>) -> Self {
        let mut by_path = HashMap::with_capacity(tags.len());
        let mut by_name = HashMap::with_capacity(tags.len());

        for (index, tag) in tags.iter().enumerate() {
            by_path.insert(tag.path.clone(), index);
            by_name.entry(tag.display_name.clone()).or_insert(index);
        }

        Self {
            tags,
            by_path,
            by_name,
        }
    }

    /// Finds a tag by exact browse path, falling back to display name.
    pub fn find(&self, path_or_name: &str) -> Option<&PlcTag> {
        self.by_path
            .get(path_or_name)
            .or_else(|| self.by_name.get(path_or_name))
            .map(|&i| &self.tags[i])
    }

    /// Finds a tag by its node ID.
    pub fn find_by_node_id(&self, node_id: &str) -> Option<&PlcTag> {
        self.tags.iter().find(|t| t.node_id == node_id)
    }

    /// Every tag whose browse path matches a glob pattern.
    ///
    /// `*` matches any run of characters including `/`, and `?` matches one.
    ///
    /// ```
    /// # use opcua_tag_browser::{PlcTag, TagSet};
    /// # let tags = TagSet::new(vec![
    /// #     PlcTag::new("Speed", "Speed", "ns=2;s=a", "Variable", "Machine/Axis1/Speed"),
    /// #     PlcTag::new("Speed", "Speed", "ns=2;s=b", "Variable", "Machine/Axis2/Speed"),
    /// #     PlcTag::new("Temp", "Temp", "ns=2;s=c", "Variable", "Oven/Temp"),
    /// # ]);
    /// assert_eq!(tags.matching("Machine/*").len(), 2);
    /// assert_eq!(tags.matching("*/Speed").len(), 2);
    /// ```
    pub fn matching(&self, pattern: &str) -> Vec<&PlcTag> {
        self.tags
            .iter()
            .filter(|t| glob_match(pattern, &t.path))
            .collect()
    }

    /// Every tag directly beneath a browse path.
    pub fn children_of(&self, path: &str) -> Vec<&PlcTag> {
        let prefix = format!("{}/", path.trim_end_matches('/'));
        self.tags
            .iter()
            .filter(|t| {
                t.path
                    .strip_prefix(&prefix)
                    .is_some_and(|rest| !rest.contains('/'))
            })
            .collect()
    }

    /// Number of tags in the set.
    pub fn len(&self) -> usize {
        self.tags.len()
    }

    /// Whether the set is empty.
    pub fn is_empty(&self) -> bool {
        self.tags.is_empty()
    }

    /// Consumes the set and returns the underlying list.
    pub fn into_vec(self) -> Vec<PlcTag> {
        self.tags
    }

    /// Borrows the underlying list.
    pub fn as_slice(&self) -> &[PlcTag] {
        &self.tags
    }
}

impl Deref for TagSet {
    type Target = [PlcTag];

    fn deref(&self) -> &Self::Target {
        &self.tags
    }
}

impl From<Vec<PlcTag>> for TagSet {
    fn from(tags: Vec<PlcTag>) -> Self {
        Self::new(tags)
    }
}

impl FromIterator<PlcTag> for TagSet {
    fn from_iter<I: IntoIterator<Item = PlcTag>>(iter: I) -> Self {
        Self::new(iter.into_iter().collect())
    }
}

/// Matches `text` against a glob with `*` and `?` wildcards.
///
/// Iterative with backtracking rather than recursive, so a pathological pattern
/// cannot blow the stack.
pub(crate) fn glob_match(pattern: &str, text: &str) -> bool {
    let pattern: Vec<char> = pattern.chars().collect();
    let text: Vec<char> = text.chars().collect();

    let (mut p, mut t) = (0usize, 0usize);
    let (mut star, mut resume) = (usize::MAX, 0usize);

    while t < text.len() {
        if p < pattern.len() && (pattern[p] == '?' || pattern[p] == text[t]) {
            p += 1;
            t += 1;
        } else if p < pattern.len() && pattern[p] == '*' {
            star = p;
            resume = t;
            p += 1;
        } else if star != usize::MAX {
            // Backtrack: let the last `*` swallow one more character.
            p = star + 1;
            resume += 1;
            t = resume;
        } else {
            return false;
        }
    }

    while p < pattern.len() && pattern[p] == '*' {
        p += 1;
    }

    p == pattern.len()
}
