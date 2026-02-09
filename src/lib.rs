// Jetporch
// Copyright (C) 2023 - Michael DeHaan <michael@michaeldehaan.net> + contributors
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// at your option) any later version.
// 
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
// 
// You should have received a copy of the GNU General Public License
// long with this program.  If not, see <http://www.gnu.org/licenses/>.

pub mod cli;
pub mod connection;
pub mod dns;
pub mod handle;
pub mod inventory;
pub mod modules;
pub mod playbooks;
pub mod provisioners;
pub mod registry;
pub mod tasks;
pub mod util;

// New API modules
pub mod api;
pub mod config;
pub mod error;
pub mod output;

// Re-export commonly used types for library users
pub use api::{PlaybookRunner, PlaybookRunnerBuilder, PlaybookResult, run_playbook};
pub use config::{JetpackConfig, ConnectionMode};
pub use error::{JetpackError, Result};
pub use output::{OutputHandler, OutputHandlerRef, TerminalOutputHandler, NullOutputHandler, LogLevel, RecapData};
pub use inventory::inventory::Inventory;
pub use provisioners::ProvisionConfig;