// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

/// A file touched by a diff, with the lines the diff adds to it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedFile {
    pub path: String,
    pub added_lines: Vec<String>,
}

/// Returns the content of `line` when it is an added line, given a marker
/// column `marker_width` wide.
///
/// A combined diff from a merge carries one marker column per parent, so a
/// line added relative to both parents reads `++foo` and one added relative to
/// a single parent reads ` +foo`.
fn added_content(line: &str, marker_width: usize) -> Option<&str> {
    if !line.is_char_boundary(marker_width) {
        return None;
    }
    let (markers, content) = line.split_at(marker_width);
    if !markers.contains('+') {
        return None;
    }
    if !markers.chars().all(|c| matches!(c, '+' | '-' | ' ')) {
        return None;
    }
    Some(content)
}

/// Parses `git diff` output into one entry per changed file.
///
/// Headers are only recognised inside a file section's header block — between
/// a `diff --` line and the section's first `@@`. Inside a hunk body every line
/// carries a marker column, so an unprefixed line at column zero is always
/// structural and content can never be mistaken for a header.
///
/// Combined diffs from a merge — `diff --cc`, emitted by plain `git diff` on a
/// conflicted tree and by `git log -p --cc` — carry one marker column per
/// parent and open their hunks with `@@@`. The marker width is read from the
/// hunk header so their content is sliced correctly.
///
/// Only the post-image path and added lines are extracted; hunk offsets,
/// removals and renames are deliberately ignored. Input is assumed to be git
/// output: a bare `diff -u` patch, which has no `diff --` opener, yields
/// nothing.
pub fn parse_unified_diff(diff: &str) -> Vec<ChangedFile> {
    let mut files: Vec<ChangedFile> = Vec::new();
    let mut in_file_header = false;
    let mut marker_width = 1usize;

    for line in diff.lines() {
        if line.starts_with("diff --") {
            in_file_header = true;
            marker_width = 1;
        } else if let Some(rest) = line.strip_prefix("@@") {
            in_file_header = false;
            marker_width = 1 + rest.chars().take_while(|c| *c == '@').count();
        } else if in_file_header {
            // Do not collapse this into a let-chain on the arm above. The
            // nested form swallows every other header-block line; a let-chain
            // would let `index …`, `new file mode …` and the `--- ` line fall
            // through to the added-line branch below.
            if let Some(path) = line.strip_prefix("+++ ") {
                let path = path.strip_prefix("b/").unwrap_or(path);
                if path != "/dev/null" {
                    files.push(ChangedFile {
                        path: path.to_string(),
                        added_lines: Vec::new(),
                    });
                }
            }
        } else if let Some(added) = added_content(line, marker_width)
            && let Some(current) = files.last_mut()
        {
            current.added_lines.push(added.to_string());
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
    fn does_not_emit_the_post_image_header_as_an_added_line() {
        let files = parse_unified_diff(SAMPLE);
        for file in &files {
            for line in &file.added_lines {
                assert!(
                    !line.starts_with("++ b/"),
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
        assert_eq!(files.len(), 1);
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
        assert_eq!(files.len(), 1);
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

    #[test]
    fn attributes_a_header_shaped_added_line_to_the_enclosing_file() {
        const DIFF: &str = "\
diff --git a/notes.md b/notes.md
index 1111111..2222222 100644
--- a/notes.md
+++ b/notes.md
@@ -1 +1,3 @@
 changelog
+++ b/phantom.txt
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "notes.md");
        assert_eq!(
            files[0].added_lines,
            vec!["++ b/phantom.txt", "requestz==2.31.0"]
        );
    }

    #[test]
    fn attributes_added_lines_correctly_after_a_deletion() {
        const DIFF: &str = "\
diff --git a/old.py b/old.py
deleted file mode 100644
index 3333333..0000000
--- a/old.py
+++ /dev/null
@@ -1 +0,0 @@
-print('gone')
diff --git a/requirements.txt b/requirements.txt
index 4444444..5555555 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1 +1,2 @@
 ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(files[0].added_lines, vec!["requestz==2.31.0"]);
    }

    #[test]
    fn ignores_header_shaped_lines_inside_a_hunk_body() {
        const DIFF: &str = "\
diff --git a/fixtures/sample.patch b/fixtures/sample.patch
index 1111111..2222222 100644
--- a/fixtures/sample.patch
+++ b/fixtures/sample.patch
@@ -1,2 +1,2 @@
--- old
+++ b/phantom.txt
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "fixtures/sample.patch");
        assert_eq!(files[0].added_lines, vec!["++ b/phantom.txt"]);
    }

    /// A tab ends the filename field of a `---`/`+++` line. git appends one
    /// whenever the path contains a space, and other tools put a timestamp
    /// after it.
    ///
    /// Keeping the terminator makes the path match nothing: `manifest::parse`
    /// dispatches on the file name, and `requirements.txt\t` is not
    /// `requirements.txt`. So a manifest under `my project/` is not reported
    /// as unreadable, it is silently never checked — and a directory with a
    /// space in it is ordinary, not exotic.
    #[test]
    fn strips_the_tab_that_terminates_a_path_containing_a_space() {
        const DIFF: &str = "\
diff --git a/my project/requirements.txt b/my project/requirements.txt
index 1111111..2222222 100644
--- a/my project/requirements.txt\t
+++ b/my project/requirements.txt\t
@@ -1 +1,2 @@
 ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "my project/requirements.txt");
        assert_eq!(files[0].added_lines, vec!["requestz==2.31.0"]);
    }

    /// The same field rule, with the timestamp a POSIX unified diff carries.
    /// Only the terminator and what follows it are dropped, never part of a
    /// name — a path with no tab must survive untouched, which every other
    /// test here covers.
    #[test]
    fn drops_a_timestamp_following_the_path() {
        const DIFF: &str = "\
diff --git a/requirements.txt b/requirements.txt
--- a/requirements.txt\t2026-08-05 10:00:00.000000000 +0200
+++ b/requirements.txt\t2026-08-05 10:05:00.000000000 +0200
@@ -1 +1,2 @@
 ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "requirements.txt");
    }

    #[test]
    fn records_a_newly_added_file() {
        const DIFF: &str = "\
diff --git a/requirements.txt b/requirements.txt
new file mode 100644
index 0000000..6666666
--- /dev/null
+++ b/requirements.txt
@@ -0,0 +1,2 @@
+ruff==0.4.0
+requestz==2.31.0
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(
            files[0].added_lines,
            vec!["ruff==0.4.0", "requestz==2.31.0"]
        );
    }

    #[test]
    fn records_a_combined_diff_section_from_a_merge() {
        const DIFF: &str = "\
diff --git a/requirements.txt b/requirements.txt
index 1111111..2222222 100644
--- a/requirements.txt
+++ b/requirements.txt
@@ -1 +1,2 @@
 ruff==0.4.0
+httpx==0.27.0
diff --cc setup.py
index 3333333,4444444..5555555
--- a/setup.py
+++ b/setup.py
@@@ -1,1 -1,1 +1,3 @@@
  import setuptools
++import requestz
 +from_one_parent
";
        let files = parse_unified_diff(DIFF);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].path, "requirements.txt");
        assert_eq!(files[0].added_lines, vec!["httpx==0.27.0"]);
        assert_eq!(files[1].path, "setup.py");
        assert_eq!(
            files[1].added_lines,
            vec!["import requestz", "from_one_parent"]
        );
    }
}
