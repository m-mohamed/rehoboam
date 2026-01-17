//! Git diff parsing and structured representation
//!
//! Parses raw `git diff` output into structured data for enhanced TUI display.
//! Supports file navigation, line numbers, and collapsible hunks.

/// Structured representation of a complete git diff
#[derive(Debug, Clone, Default)]
pub struct ParsedDiff {
    /// List of files with changes
    pub files: Vec<FileDiff>,
    /// Summary statistics
    pub summary: DiffSummary,
}

/// Summary statistics for a diff
#[derive(Debug, Clone, Default)]
pub struct DiffSummary {
    /// Number of files changed
    pub files_changed: usize,
    /// Total lines added
    pub insertions: usize,
    /// Total lines deleted
    pub deletions: usize,
}

/// A single file's diff content
#[derive(Debug, Clone)]
pub struct FileDiff {
    /// File path (new path if renamed)
    pub path: String,
    /// Old path (for renames)
    pub old_path: Option<String>,
    /// List of hunks (change sections)
    pub hunks: Vec<Hunk>,
    /// Lines added in this file
    pub insertions: usize,
    /// Lines deleted in this file
    pub deletions: usize,
    /// Whether this is a binary file
    pub is_binary: bool,
}

/// A hunk (section of changes within a file)
#[derive(Debug, Clone)]
pub struct Hunk {
    /// The @@ header line (e.g., "@@ -10,5 +10,7 @@")
    pub header: String,
    /// Starting line in old file.
    /// Reserved for Phase 1: Line navigation and jump-to-line features.
    #[allow(dead_code)]
    pub old_start: u32,
    /// Number of lines in old file.
    /// Reserved for Phase 1: Hunk size indicators.
    #[allow(dead_code)]
    pub old_count: u32,
    /// Starting line in new file.
    /// Reserved for Phase 1: Line navigation and jump-to-line features.
    #[allow(dead_code)]
    pub new_start: u32,
    /// Number of lines in new file.
    /// Reserved for Phase 1: Hunk size indicators.
    #[allow(dead_code)]
    pub new_count: u32,
    /// Lines in this hunk
    pub lines: Vec<DiffLine>,
    /// Whether this hunk is collapsed in the UI.
    /// Reserved for Phase 1: Collapsible hunks for large diffs.
    #[allow(dead_code)]
    pub collapsed: bool,
}

/// A single line in a diff
#[derive(Debug, Clone)]
pub struct DiffLine {
    /// Type of line (context, addition, deletion)
    pub kind: LineKind,
    /// The actual content (without the +/- prefix)
    pub content: String,
    /// Line number in old file (None for additions)
    pub old_line_no: Option<u32>,
    /// Line number in new file (None for deletions)
    pub new_line_no: Option<u32>,
}

/// Type of diff line
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LineKind {
    /// Unchanged context line
    Context,
    /// Added line (+)
    Addition,
    /// Deleted line (-)
    Deletion,
}

impl ParsedDiff {
    /// Create an empty diff
    pub fn empty() -> Self {
        Self::default()
    }

    /// Check if the diff has any changes
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    /// Format the summary as a string
    pub fn summary_string(&self) -> String {
        let files = self.summary.files_changed;
        let file_word = if files == 1 { "file" } else { "files" };
        format!(
            "{} {} changed, +{} -{}",
            files, file_word, self.summary.insertions, self.summary.deletions
        )
    }
}

/// Parse raw git diff output into structured form
pub fn parse_diff(raw: &str) -> ParsedDiff {
    if raw.trim().is_empty() {
        return ParsedDiff::empty();
    }

    let mut files = Vec::new();
    let mut current_file: Option<FileDiff> = None;
    let mut current_hunk: Option<Hunk> = None;
    let mut old_line_no: u32 = 0;
    let mut new_line_no: u32 = 0;

    for line in raw.lines() {
        // File header: "diff --git a/path b/path"
        if line.starts_with("diff --git ") {
            // Save previous hunk and file
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }
            if let Some(file) = current_file.take() {
                files.push(file);
            }

            // Extract path from "diff --git a/path b/path"
            let path = extract_path_from_diff_header(line);
            current_file = Some(FileDiff {
                path,
                old_path: None,
                hunks: Vec::new(),
                insertions: 0,
                deletions: 0,
                is_binary: false,
            });
            continue;
        }

        // Binary file marker
        if line.starts_with("Binary files ") {
            if let Some(ref mut file) = current_file {
                file.is_binary = true;
            }
            continue;
        }

        // Rename from: "rename from path"
        if let Some(old_path) = line.strip_prefix("rename from ") {
            if let Some(ref mut file) = current_file {
                file.old_path = Some(old_path.to_string());
            }
            continue;
        }

        // Hunk header: "@@ -start,count +start,count @@"
        if line.starts_with("@@ ") {
            // Save previous hunk
            if let Some(hunk) = current_hunk.take() {
                if let Some(ref mut file) = current_file {
                    file.hunks.push(hunk);
                }
            }

            // Parse hunk header
            if let Some((old_start, old_count, new_start, new_count)) = parse_hunk_header(line) {
                old_line_no = old_start;
                new_line_no = new_start;
                current_hunk = Some(Hunk {
                    header: line.to_string(),
                    old_start,
                    old_count,
                    new_start,
                    new_count,
                    lines: Vec::new(),
                    collapsed: false,
                });
            }
            continue;
        }

        // Skip other metadata lines
        if line.starts_with("index ")
            || line.starts_with("--- ")
            || line.starts_with("+++ ")
            || line.starts_with("new file mode")
            || line.starts_with("deleted file mode")
            || line.starts_with("old mode")
            || line.starts_with("new mode")
            || line.starts_with("similarity index")
            || line.starts_with("rename to")
        {
            continue;
        }

        // Content lines: +, -, or space (context)
        if let Some(ref mut hunk) = current_hunk {
            let (kind, content) = if let Some(stripped) = line.strip_prefix('+') {
                (LineKind::Addition, stripped)
            } else if let Some(stripped) = line.strip_prefix('-') {
                (LineKind::Deletion, stripped)
            } else if let Some(stripped) = line.strip_prefix(' ') {
                (LineKind::Context, stripped)
            } else if line.is_empty() {
                (LineKind::Context, "")
            } else {
                // Unknown line type, treat as context
                (LineKind::Context, line)
            };

            let (old_no, new_no) = match kind {
                LineKind::Context => {
                    let old = old_line_no;
                    let new = new_line_no;
                    old_line_no += 1;
                    new_line_no += 1;
                    (Some(old), Some(new))
                }
                LineKind::Addition => {
                    let new = new_line_no;
                    new_line_no += 1;
                    if let Some(ref mut file) = current_file {
                        file.insertions += 1;
                    }
                    (None, Some(new))
                }
                LineKind::Deletion => {
                    let old = old_line_no;
                    old_line_no += 1;
                    if let Some(ref mut file) = current_file {
                        file.deletions += 1;
                    }
                    (Some(old), None)
                }
            };

            hunk.lines.push(DiffLine {
                kind,
                content: content.to_string(),
                old_line_no: old_no,
                new_line_no: new_no,
            });
        }
    }

    // Save final hunk and file
    if let Some(hunk) = current_hunk {
        if let Some(ref mut file) = current_file {
            file.hunks.push(hunk);
        }
    }
    if let Some(file) = current_file {
        files.push(file);
    }

    // Calculate summary
    let summary = DiffSummary {
        files_changed: files.len(),
        insertions: files.iter().map(|f| f.insertions).sum(),
        deletions: files.iter().map(|f| f.deletions).sum(),
    };

    ParsedDiff { files, summary }
}

/// Extract file path from "diff --git a/path b/path" line
fn extract_path_from_diff_header(line: &str) -> String {
    // Format: "diff --git a/path b/path"
    // We want the b/path part (new path)
    if let Some(b_idx) = line.rfind(" b/") {
        return line[b_idx + 3..].to_string();
    }
    // Fallback: try to extract from a/ part
    if let Some(a_idx) = line.find(" a/") {
        let rest = &line[a_idx + 3..];
        if let Some(space_idx) = rest.find(' ') {
            return rest[..space_idx].to_string();
        }
    }
    // Last resort: return everything after "diff --git "
    line.strip_prefix("diff --git ").unwrap_or(line).to_string()
}

/// Parse hunk header "@@ -old_start,old_count +new_start,new_count @@"
fn parse_hunk_header(line: &str) -> Option<(u32, u32, u32, u32)> {
    // Format: "@@ -10,5 +10,7 @@" or "@@ -10 +10 @@" (count defaults to 1)
    let line = line.strip_prefix("@@ ")?;
    let line = line.split(" @@").next()?;

    let parts: Vec<&str> = line.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }

    let (old_start, old_count) = parse_range(parts[0].strip_prefix('-')?)?;
    let (new_start, new_count) = parse_range(parts[1].strip_prefix('+')?)?;

    Some((old_start, old_count, new_start, new_count))
}

/// Parse "start,count" or just "start" (count defaults to 1)
fn parse_range(s: &str) -> Option<(u32, u32)> {
    if let Some((start, count)) = s.split_once(',') {
        Some((start.parse().ok()?, count.parse().ok()?))
    } else {
        Some((s.parse().ok()?, 1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_empty_diff() {
        let diff = parse_diff("");
        assert!(diff.is_empty());
        assert_eq!(diff.summary.files_changed, 0);
    }

    #[test]
    fn test_parse_simple_diff() {
        let raw = r#"diff --git a/src/main.rs b/src/main.rs
index abc123..def456 100644
--- a/src/main.rs
+++ b/src/main.rs
@@ -1,3 +1,4 @@
 fn main() {
+    println!("Hello");
     let x = 1;
 }
"#;
        let diff = parse_diff(raw);
        assert_eq!(diff.files.len(), 1);
        assert_eq!(diff.files[0].path, "src/main.rs");
        assert_eq!(diff.files[0].insertions, 1);
        assert_eq!(diff.files[0].deletions, 0);
        assert_eq!(diff.summary.insertions, 1);
        assert_eq!(diff.summary.deletions, 0);
    }

    #[test]
    fn test_parse_multi_file_diff() {
        let raw = r#"diff --git a/file1.rs b/file1.rs
--- a/file1.rs
+++ b/file1.rs
@@ -1,2 +1,3 @@
 line1
+added
 line2
diff --git a/file2.rs b/file2.rs
--- a/file2.rs
+++ b/file2.rs
@@ -1,3 +1,2 @@
 line1
-removed
 line2
"#;
        let diff = parse_diff(raw);
        assert_eq!(diff.files.len(), 2);
        assert_eq!(diff.summary.files_changed, 2);
        assert_eq!(diff.summary.insertions, 1);
        assert_eq!(diff.summary.deletions, 1);
    }

    #[test]
    fn test_parse_hunk_header() {
        assert_eq!(parse_hunk_header("@@ -10,5 +10,7 @@"), Some((10, 5, 10, 7)));
        assert_eq!(parse_hunk_header("@@ -1 +1 @@"), Some((1, 1, 1, 1)));
        assert_eq!(
            parse_hunk_header("@@ -100,20 +105,25 @@ fn main()"),
            Some((100, 20, 105, 25))
        );
    }

    #[test]
    fn test_line_numbers() {
        let raw = r#"diff --git a/test.rs b/test.rs
@@ -5,4 +5,5 @@
 context line
+added line
 another context
-removed line
 final context
"#;
        let diff = parse_diff(raw);
        let hunk = &diff.files[0].hunks[0];

        // Context line at old:5, new:5
        assert_eq!(hunk.lines[0].kind, LineKind::Context);
        assert_eq!(hunk.lines[0].old_line_no, Some(5));
        assert_eq!(hunk.lines[0].new_line_no, Some(5));

        // Added line at new:6 only
        assert_eq!(hunk.lines[1].kind, LineKind::Addition);
        assert_eq!(hunk.lines[1].old_line_no, None);
        assert_eq!(hunk.lines[1].new_line_no, Some(6));

        // Deleted line at old:7 only
        assert_eq!(hunk.lines[3].kind, LineKind::Deletion);
        assert_eq!(hunk.lines[3].old_line_no, Some(7));
        assert_eq!(hunk.lines[3].new_line_no, None);
    }

    #[test]
    fn test_summary_string() {
        let mut diff = ParsedDiff::empty();
        diff.summary = DiffSummary {
            files_changed: 3,
            insertions: 45,
            deletions: 12,
        };
        assert_eq!(diff.summary_string(), "3 files changed, +45 -12");

        diff.summary.files_changed = 1;
        assert_eq!(diff.summary_string(), "1 file changed, +45 -12");
    }
}
