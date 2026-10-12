// fontelle-asio — hosts an ASIO driver for Fontelle in a separate process.
// Copyright (C) 2026 Fopull LLC
//
// This program is free software: you can redistribute it and/or modify it
// under the terms of the GNU General Public License as published by the Free
// Software Foundation, version 3.
//
// This program is distributed in the hope that it will be useful, but
// WITHOUT ANY WARRANTY; without even the implied warranty of MERCHANTABILITY
// or FITNESS FOR A PARTICULAR PURPOSE. See the GNU General Public License for
// more details. You should have received a copy of the GNU General Public
// License along with this program (`LICENSE` beside `Cargo.toml`). If not,
// see <https://www.gnu.org/licenses/>.
//
// ASIO is a registered trademark of Steinberg Media Technologies GmbH.

//! `fontelle-asio`: a driver host of its own, so Fontelle can play through
//! ASIO® without Steinberg's GPLv3 SDK, or any closed add-on, ever sharing a
//! process with the other.
//!
//! The program does four things (`docs/asio.md` has the whole protocol):
//!
//! - `fontelle-asio --version`: says its protocol version.
//! - `fontelle-asio list`: names the installed ASIO drivers.
//! - `fontelle-asio panel --driver <name>`: opens a driver's own settings.
//! - `fontelle-asio run …` ([`fontelle_audio_ipc::RunArgs`]): opens a
//!   driver at Fontelle's rate and buffer, and on each of the driver's
//!   buffers asks Fontelle for audio through the shared block
//!   ([`pump::Pump`]), until Fontelle says stop, closes its end, or exits.
//!
//! Everything but the driver itself ([`asio`], Windows only) is plain Rust
//! and runs anywhere: the [`device::ClockDevice`] stands in for a driver in
//! tests.

pub mod convert;
pub mod device;
pub mod pump;
pub mod run;
pub mod sizes;

#[cfg(windows)]
pub mod asio;
