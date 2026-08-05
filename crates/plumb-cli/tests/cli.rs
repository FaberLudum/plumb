// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use assert_cmd::Command;

#[test]
fn reports_its_version() {
    let mut cmd = Command::cargo_bin("plumb").expect("binary builds");
    cmd.arg("--version");
    cmd.assert().success().stdout(predicates::str::contains("plumb 0.1.0"));
}
