// SPDX-License-Identifier: Apache-2.0
// Copyright 2026 The Plumb Authors

//! Pure logic for Plumb's gates. Nothing in this crate touches the network,
//! the filesystem, or subprocesses; that all lives in `plumb-cli`.

pub mod adapters;
pub mod deps;
pub mod diff;
pub mod finding;
pub mod layer1;
pub mod manifest;
pub mod report;
