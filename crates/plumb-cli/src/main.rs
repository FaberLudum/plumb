// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

use clap::Parser;

mod git;

#[derive(Parser)]
#[command(
    name = "plumb",
    version,
    about = "Structural gates for AI-generated code"
)]
struct Cli {}

fn main() {
    let _cli = Cli::parse();
}
