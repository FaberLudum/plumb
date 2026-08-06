// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Layer 1 artefact rendering: bootstrap answers to the four `.plumb/` files.

pub struct Bootstrap {
    pub project_name: String,
    pub modules: Vec<String>,
    pub error_handling: String,
    pub test_shape: String,
}

pub struct Artefacts {
    pub architecture: String,
    pub conventions: String,
    pub invariants: String,
    pub index: String,
}

pub fn greenfield(bootstrap: &Bootstrap) -> Artefacts {
    let _ = bootstrap;
    todo!()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn demo() -> Bootstrap {
        Bootstrap {
            project_name: "demo".to_string(),
            modules: vec!["api".to_string(), "storage".to_string()],
            error_handling: "errors propagate; no swallowed exceptions".to_string(),
            test_shape: "one behaviour per test, named as a sentence".to_string(),
        }
    }

    #[test]
    fn architecture_names_every_module_with_a_boundary_line() {
        let artefacts = greenfield(&demo());
        assert!(artefacts.architecture.contains("## api"));
        assert!(artefacts.architecture.contains("## storage"));
        assert!(
            artefacts.architecture.contains("May depend on:"),
            "every module section carries an explicit dependency line:\n{}",
            artefacts.architecture
        );
    }

    #[test]
    fn conventions_freeze_the_answers_verbatim() {
        let artefacts = greenfield(&demo());
        assert!(
            artefacts
                .conventions
                .contains("errors propagate; no swallowed exceptions")
        );
        assert!(
            artefacts
                .conventions
                .contains("one behaviour per test, named as a sentence")
        );
    }

    #[test]
    fn index_says_it_is_generated_and_starts_empty() {
        let artefacts = greenfield(&demo());
        assert!(artefacts.index.contains("generated"));
        assert!(
            artefacts.index.contains("plumb index"),
            "the empty index must name the command that fills it:\n{}",
            artefacts.index
        );
    }

    #[test]
    fn rendering_is_deterministic() {
        let a = greenfield(&demo());
        let b = greenfield(&demo());
        assert_eq!(a.architecture, b.architecture);
        assert_eq!(a.conventions, b.conventions);
        assert_eq!(a.invariants, b.invariants);
        assert_eq!(a.index, b.index);
    }
}
