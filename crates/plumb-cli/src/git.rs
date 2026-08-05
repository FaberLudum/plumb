// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process::Command;

    fn init_repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let run = |args: &[&str]| {
            Command::new("git")
                .args(args)
                .current_dir(dir.path())
                .output()
                .expect("git runs");
        };
        run(&["init", "-b", "main"]);
        run(&["config", "user.email", "test@example.com"]);
        run(&["config", "user.name", "Test"]);
        fs::write(dir.path().join("requirements.txt"), "ruff==0.4.0\n").expect("write");
        run(&["add", "-A"]);
        run(&["commit", "-m", "initial"]);
        dir
    }

    #[test]
    fn working_diff_reports_uncommitted_changes() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let diff = working_diff(dir.path()).expect("diff succeeds");

        assert!(diff.contains("+requestz==2.31.0"), "diff was: {diff}");
    }

    #[test]
    fn show_at_head_returns_the_committed_contents() {
        let dir = init_repo();
        fs::write(
            dir.path().join("requirements.txt"),
            "ruff==0.4.0\nrequestz==2.31.0\n",
        )
        .expect("write");

        let head = show_at_head(dir.path(), "requirements.txt").expect("show succeeds");

        assert_eq!(head.as_deref(), Some("ruff==0.4.0\n"));
    }

    #[test]
    fn show_at_head_returns_none_for_a_new_file() {
        let dir = init_repo();
        fs::write(dir.path().join("pyproject.toml"), "[project]\n").expect("write");

        let head = show_at_head(dir.path(), "pyproject.toml").expect("show succeeds");

        assert_eq!(head, None);
    }
}
