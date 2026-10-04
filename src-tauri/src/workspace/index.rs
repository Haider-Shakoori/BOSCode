use super::{
    ignored_entry, is_sensitive_path, relative_display, WorkspaceEntry, MAX_WORKSPACE_ENTRIES,
};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use walkdir::WalkDir;

const MAX_INDEX_FILE_BYTES: u64 = 256_000;
const MAX_INDEX_TOTAL_BYTES: usize = 32_000_000;

#[derive(Clone)]
pub(super) struct IndexedTextFile {
    pub path: String,
    pub size: u64,
    modified: u64,
    pub content: String,
}

#[derive(Clone)]
pub(super) struct WorkspaceIndex {
    pub root: PathBuf,
    pub entries: Vec<WorkspaceEntry>,
    pub files: Vec<IndexedTextFile>,
    pub file_count: usize,
    pub directory_count: usize,
    pub truncated: bool,
    pub built_at: u64,
}

#[derive(Clone)]
pub(super) struct RankedSnippet {
    pub path: String,
    pub line: usize,
    pub preview: String,
    pub score: usize,
}

fn timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn modified_seconds(path: &Path) -> u64 {
    fs::metadata(path)
        .and_then(|metadata| metadata.modified())
        .ok()
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or_default()
}

fn readable_text(path: &Path, size: u64, budget_remaining: usize) -> Option<String> {
    if size == 0 || size > MAX_INDEX_FILE_BYTES || size as usize > budget_remaining {
        return None;
    }

    let bytes = fs::read(path).ok()?;
    if bytes.iter().take(8_192).any(|byte| *byte == 0) {
        return None;
    }

    String::from_utf8(bytes).ok()
}

impl WorkspaceIndex {
    pub fn build(root: &Path, previous: Option<&WorkspaceIndex>) -> Self {
        let previous_files = previous
            .filter(|index| index.root == root)
            .map(|index| {
                index
                    .files
                    .iter()
                    .map(|file| (file.path.clone(), file))
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();

        let mut entries = Vec::new();
        let mut files = Vec::new();
        let mut file_count = 0usize;
        let mut directory_count = 0usize;
        let mut truncated = false;
        let mut indexed_bytes = 0usize;

        for entry in WalkDir::new(root)
            .follow_links(false)
            .into_iter()
            .filter_entry(|entry| !ignored_entry(entry))
            .filter_map(Result::ok)
            .skip(1)
        {
            if entries.len() >= MAX_WORKSPACE_ENTRIES {
                truncated = true;
                break;
            }

            if entry.file_type().is_symlink() || is_sensitive_path(entry.path()) {
                continue;
            }

            let Ok(relative) = entry.path().strip_prefix(root) else {
                continue;
            };

            let is_dir = entry.file_type().is_dir();
            if is_dir {
                directory_count += 1;
            } else if entry.file_type().is_file() {
                file_count += 1;
            } else {
                continue;
            }

            let path = relative_display(relative);
            let size = if is_dir {
                None
            } else {
                entry.metadata().ok().map(|metadata| metadata.len())
            };

            entries.push(WorkspaceEntry {
                path: path.clone(),
                name: entry.file_name().to_string_lossy().to_string(),
                is_dir,
                depth: relative.components().count().saturating_sub(1),
                size,
            });

            if is_dir {
                continue;
            }

            let Some(size) = size else {
                continue;
            };
            let modified = modified_seconds(entry.path());

            if let Some(previous) = previous_files.get(&path) {
                if previous.size == size && previous.modified == modified {
                    indexed_bytes = indexed_bytes.saturating_add(previous.content.len());
                    files.push((*previous).clone());
                    continue;
                }
            }

            let budget_remaining = MAX_INDEX_TOTAL_BYTES.saturating_sub(indexed_bytes);
            let Some(content) = readable_text(entry.path(), size, budget_remaining) else {
                continue;
            };

            indexed_bytes = indexed_bytes.saturating_add(content.len());
            files.push(IndexedTextFile {
                path,
                size,
                modified,
                content,
            });
        }

        entries.sort_by(|left, right| {
            left.path
                .to_ascii_lowercase()
                .cmp(&right.path.to_ascii_lowercase())
        });
        files.sort_by(|left, right| left.path.cmp(&right.path));

        Self {
            root: root.to_path_buf(),
            entries,
            files,
            file_count,
            directory_count,
            truncated,
            built_at: timestamp(),
        }
    }

    pub fn search(&self, query: &str, limit: usize) -> Vec<super::SearchHit> {
        let needle = query.trim().to_ascii_lowercase();
        if needle.len() < 2 {
            return Vec::new();
        }

        let mut hits = Vec::new();

        'files: for file in &self.files {
            for (index, line) in file.content.lines().enumerate() {
                if line.to_ascii_lowercase().contains(&needle) {
                    hits.push(super::SearchHit {
                        path: file.path.clone(),
                        line: index + 1,
                        preview: line.trim().chars().take(240).collect(),
                    });

                    if hits.len() >= limit {
                        break 'files;
                    }
                }
            }
        }

        hits
    }

    pub fn rank_snippets(&self, tokens: &[String], limit: usize) -> Vec<RankedSnippet> {
        if tokens.is_empty() {
            return Vec::new();
        }

        let mut ranked = Vec::new();

        for file in &self.files {
            let path_lower = file.path.to_ascii_lowercase();
            let content_lower = file.content.to_ascii_lowercase();
            let mut file_score = 0usize;

            for token in tokens {
                if path_lower.contains(token) {
                    file_score += 16;
                }

                let occurrences = content_lower.matches(token).take(4).count();
                file_score += occurrences * 3;
            }

            if file_score == 0 {
                continue;
            }

            let mut emitted_for_file = 0usize;
            for (line_index, line) in file.content.lines().enumerate() {
                let lower = line.to_ascii_lowercase();
                let line_hits = tokens
                    .iter()
                    .filter(|token| lower.contains(token.as_str()))
                    .count();

                if line_hits == 0 {
                    continue;
                }

                ranked.push(RankedSnippet {
                    path: file.path.clone(),
                    line: line_index + 1,
                    preview: line.trim().chars().take(240).collect(),
                    score: file_score + line_hits * 5,
                });
                emitted_for_file += 1;

                if emitted_for_file >= 3 {
                    break;
                }
            }
        }

        ranked.sort_by(|left, right| {
            right
                .score
                .cmp(&left.score)
                .then_with(|| left.path.cmp(&right.path))
                .then_with(|| left.line.cmp(&right.line))
        });
        ranked.truncate(limit);
        ranked
    }

    #[cfg(test)]
    pub fn indexed_bytes(&self) -> usize {
        self.files.iter().map(|file| file.content.len()).sum()
    }
}

#[cfg(test)]
mod tests {
    use super::WorkspaceIndex;
    use std::{
        fs,
        time::{SystemTime, UNIX_EPOCH},
    };

    fn temp_workspace() -> std::path::PathBuf {
        let suffix = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("boscode-index-{suffix}"));
        fs::create_dir_all(root.join("src")).unwrap();
        root
    }

    #[test]
    fn builds_and_searches_cached_text_index() {
        let root = temp_workspace();
        fs::write(
            root.join("src").join("checkout.rs"),
            "pub fn checkout_total() {\n    // rounding logic\n}\n",
        )
        .unwrap();
        fs::write(root.join(".env"), "SECRET=do-not-index").unwrap();

        let index = WorkspaceIndex::build(&root, None);
        let hits = index.search("rounding", 10);

        assert_eq!(index.file_count, 1);
        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].path, "src/checkout.rs");
        assert!(index.indexed_bytes() > 0);

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn ranks_path_and_content_matches() {
        let root = temp_workspace();
        fs::write(
            root.join("src").join("invoice_checkout.rs"),
            "fn total() {\n    // checkout rounding fix\n}\n",
        )
        .unwrap();
        fs::write(root.join("src").join("other.rs"), "fn unrelated() {}\n").unwrap();

        let index = WorkspaceIndex::build(&root, None);
        let snippets = index.rank_snippets(&["checkout".to_string(), "rounding".to_string()], 8);

        assert!(!snippets.is_empty());
        assert_eq!(snippets[0].path, "src/invoice_checkout.rs");

        let _ = fs::remove_dir_all(root);
    }
}
