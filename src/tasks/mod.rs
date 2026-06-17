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

pub mod checksum;
pub mod cmd_library;
pub mod common;
pub mod fields;
pub mod files;
pub mod logic;
pub mod request;
pub mod response;

pub use crate::connection::command::cmd_info;
pub use crate::handle::handle::{CheckRc, TaskHandle};
pub use crate::playbooks::templar::TemplateMode;
pub use crate::tasks::common::{EvaluatedTask, IsAction, IsTask};
pub use crate::tasks::fields::Field;
pub use crate::tasks::files::{FileAttributesEvaluated, FileAttributesInput};
pub use crate::tasks::logic::{
    PostLogicEvaluated, PostLogicInput, PreLogicEvaluated, PreLogicInput,
};
pub use crate::tasks::request::{TaskRequest, TaskRequestType};
pub use crate::tasks::response::{TaskResponse, TaskStatus};
