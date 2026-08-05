// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

#[cfg(test)]
mod tests {
    use super::*;

    fn set(items: &[&str]) -> BTreeSet<String> {
        items.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn requirements_strips_version_specifiers() {
        let text = "ruff==0.4.0\npytest>=8.0\nhttpx\n";
        assert_eq!(parse_requirements(text), set(&["httpx", "pytest", "ruff"]));
    }

    #[test]
    fn requirements_ignores_comments_blanks_and_flags() {
        let text = "# pinned\n\nruff==0.4.0\n-r other.txt\n-e .\n--index-url https://example.com\n";
        assert_eq!(parse_requirements(text), set(&["ruff"]));
    }

    #[test]
    fn requirements_strips_extras_and_markers() {
        let text = "celery[redis]==5.3.0\nuvicorn ; python_version >= '3.9'\n";
        assert_eq!(parse_requirements(text), set(&["celery", "uvicorn"]));
    }

    #[test]
    fn requirements_rejects_lines_that_cannot_be_package_names() {
        let text = "<<<<<<< HEAD\nruff==0.4.0\n=======\nhttpx==0.27.0\n>>>>>>> feature\n";
        assert_eq!(parse_requirements(text), set(&["httpx", "ruff"]));
    }

    #[test]
    fn names_are_normalised_per_pep_503() {
        let text = "Flask_SQLAlchemy==3.0\nzope.interface==6.0\n";
        assert_eq!(
            parse_requirements(text),
            set(&["flask-sqlalchemy", "zope-interface"])
        );
    }

    #[test]
    fn pyproject_reads_project_dependencies() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff==0.4.0", "httpx>=0.27"]
"#;
        assert_eq!(parse_pyproject(text), set(&["httpx", "ruff"]));
    }

    #[test]
    fn pyproject_reads_optional_dependency_groups() {
        let text = r#"
[project]
name = "demo"
dependencies = ["ruff"]

[project.optional-dependencies]
dev = ["pytest>=8.0"]
docs = ["mkdocs"]
"#;
        assert_eq!(parse_pyproject(text), set(&["mkdocs", "pytest", "ruff"]));
    }

    #[test]
    fn pyproject_returns_empty_when_malformed() {
        assert!(parse_pyproject("this is not toml {{{").is_empty());
    }

    #[test]
    fn parse_dispatches_on_filename_and_ignores_other_files() {
        assert!(parse("requirements.txt", "ruff\n").is_some());
        assert!(parse("requirements-dev.txt", "ruff\n").is_some());
        assert!(parse("pyproject.toml", "[project]\ndependencies = []\n").is_some());
        assert!(parse("src/app.py", "import os\n").is_none());
    }
}
