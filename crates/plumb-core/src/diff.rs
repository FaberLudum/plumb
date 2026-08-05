// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

/// A file touched by a diff, with the lines the diff adds to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub added_lines: Vec<String>,
}

/// Parses `git diff` output into one entry per changed file.
///
/// Only the post-image path (`+++ b/...`) and added lines are extracted; hunk
/// offsets and removals are deliberately ignored.
pub fn parse_unified_diff(diff: &str) -> Vec<ChangedFile> {
    let mut files: Vec<ChangedFile> = Vec::new();

    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ ") {
            let path = path.strip_prefix("b/").unwrap_or(path);
            if path == "/dev/null" {
                continue;
            }
            files.push(ChangedFile {
                path: path.to_string(),
                added_lines: Vec::new(),
            });
        } else if let Some(added) = line.strip_prefix('+') {
            if line.starts_with("+++") {
                continue;
            }
            if let Some(current) = files.last_mut() {
                current.added_lines.push(added.to_string());
            }
        }
    }

    files
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
diff --git a/requirements.txt b/requirements.txt
index 1234567..89abcde 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1,2 +1,3 @@
 ruff==0.4.0
+requestz==2.31.0
 pytest==8.0.0
diff --git a/src/app.py b/src/app.py
index 2222222..3333333 100644
--- a/src/app.py
+++ b/src/app.py
@@ -1 +1,2 @@
 import os
+import requestz
";

    #[test]
    fn extracts_one_entry_per_changed_file() {
        let files = parse_unified_diff(SAMPLE);
        let paths: Vec<&str> = files.iter().map(|f| f.path.as_str()).collect();
        assert_eq!(paths, vec!["requirements.txt", "src/app.py"]);
    }

    #[test]
    fn collects_added_lines_without_the_marker() {
        let files = parse_unified_diff(SAMPLE);
        assert_eq!(files[0].added_lines, vec!["requestz==2.31.0"]);
        assert_eq!(files[1].added_lines, vec!["import requestz"]);
    }

    #[test]
    fn ignores_the_triple_plus_header() {
        let files = parse_unified_diff(SAMPLE);
        for file in &files {
            for line in &file.added_lines {
                assert!(
                    !line.starts_with("++"),
                    "header leaked into added lines: {line}"
                );
            }
        }
    }

    #[test]
    fn returns_empty_for_an_empty_diff() {
        assert!(parse_unified_diff("").is_empty());
    }

    #[test]
    fn keeps_an_added_line_whose_content_starts_with_a_plus() {
        const DIFF: &str = "\
diff --git a/src/counter.c b/src/counter.c
index 1111111..2222222 100644
--- a/src/counter.c
+++ b/src/counter.c
@@ -1 +1,2 @@
 int total = 0;
++i;
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files[0].added_lines, vec!["+i;"]);
    }

    #[test]
    fn keeps_an_added_line_whose_content_starts_with_two_pluses() {
        const DIFF: &str = "\
diff --git a/src/counter.c b/src/counter.c
index 4444444..5555555 100644
--- a/src/counter.c
+++ b/src/counter.c
@@ -1 +1,2 @@
 int total = 0;
+++i;
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files[0].added_lines, vec!["++i;"]);
    }

    #[test]
    fn skips_a_file_the_diff_deletes() {
        const DIFF: &str = "\
diff --git a/old.py b/old.py
deleted file mode 100644
index 3333333..0000000
--- a/old.py
+++ /dev/null
@@ -1 +0,0 @@
-print('gone')
";
        assert!(parse_unified_diff(DIFF).is_empty());
    }
}
