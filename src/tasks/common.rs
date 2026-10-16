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

use crate::handle::handle::TaskHandle;
use crate::tasks::TemplateMode;
use crate::tasks::logic::{PostLogicEvaluated, PreLogicEvaluated, PreLogicInput};
use crate::tasks::request::TaskRequest;
use crate::tasks::response::TaskResponse;
use std::sync::Arc;

pub struct EvaluatedTask {
    pub action: Arc<dyn IsAction>,
    pub with: Arc<Option<PreLogicEvaluated>>,
    pub and: Arc<Option<PostLogicEvaluated>>,
}

pub trait IsTask: Send + Sync {
    fn get_module(&self) -> String;
    fn get_name(&self) -> Option<String>;
    fn get_with(&self) -> Option<PreLogicInput>;

    fn evaluate(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
        tm: TemplateMode,
    ) -> Result<EvaluatedTask, Arc<TaskResponse>>;

    /// Modules describe only their operation and public resource fields.
    fn generated_name(&self, _context: &crate::tasks::naming::NamingContext<'_>) -> Option<String> {
        None
    }

    /// Values potentially written by this module cannot describe later async
    /// declarations from a snapshot taken before those writes happen.
    fn name_variable_writes(&self) -> crate::tasks::naming::NameVariableWrites {
        crate::tasks::naming::NameVariableWrites::Unknown
    }

    fn get_display_name(&self) -> String {
        if let Some(name) = self.get_name() {
            return name;
        }
        let variables = serde_yaml::Mapping::new();
        let with = self.get_with();
        let context = crate::tasks::naming::NamingContext::new(
            &variables,
            with.as_ref().and_then(|with| with.items.as_ref()),
        );
        let label = self
            .generated_name(&context)
            .unwrap_or_else(|| self.get_module());
        crate::tasks::naming::bounded(&label)
    }
}

pub trait IsAction: Send + Sync {
    /// Target-side working directory for this evaluated invocation. Modules
    /// opt in by exposing a top-level chdir field and overriding this getter.
    fn chdir(&self) -> Option<&str> {
        None
    }

    fn dispatch(
        &self,
        handle: &Arc<TaskHandle>,
        request: &Arc<TaskRequest>,
    ) -> Result<Arc<TaskResponse>, Arc<TaskResponse>>;
}
