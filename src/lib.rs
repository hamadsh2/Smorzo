// SPDX-License-Identifier: GPL-3.0-only
//! Smorzo's audio engine as a library, shared by the window app (`src/main.rs`) and the
//! command-line tool (`src/bin/smorzo-cli.rs`).

pub mod appaudio;
pub mod control;
pub mod denoise;
pub mod devices;
pub mod dfn;
pub mod dsp;
pub mod echo;
pub mod engine;
pub mod eq;
pub mod exclusive;
pub mod instance;
pub mod params;
pub mod playback;
pub mod settings;
pub mod stats;
pub mod tune;
