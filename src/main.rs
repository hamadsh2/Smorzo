// SPDX-License-Identifier: GPL-3.0-only
//! Smorzo — just your voice. The window app (tray + main window).
//!
//! The command-line tool for testing and measurements is `smorzo-cli`.

// A window app: no console window behind it.
#![windows_subsystem = "windows"]

mod ui;

fn main() {
    ui::run();
}
