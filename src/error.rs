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

use std::fmt;
use std::error::Error as StdError;
use std::io;

/// Main error type for JetPack operations
#[derive(Debug)]
pub enum JetpackError {
    /// Configuration errors
    Config(String),
    
    /// Inventory loading errors
    Inventory(String),
    
    /// Playbook parsing errors
    PlaybookParse(String),
    
    /// Task execution errors
    TaskExecution(String),
    
    /// Connection errors
    Connection(String),
    
    /// Module errors
    Module(String),
    
    /// Template errors
    Template(String),
    
    /// IO errors
    Io(io::Error),
    
    /// YAML parsing errors
    Yaml(serde_yaml::Error),
    
    /// SSH errors
    Ssh(String),
    
    /// Variable errors
    Variable(String),
    
    /// Authentication errors
    Auth(String),
    
    /// Other errors
    Other(String),
}

impl fmt::Display for JetpackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            JetpackError::Config(msg) => write!(f, "Configuration error: {}", msg),
            JetpackError::Inventory(msg) => write!(f, "Inventory error: {}", msg),
            JetpackError::PlaybookParse(msg) => write!(f, "Playbook parsing error: {}", msg),
            JetpackError::TaskExecution(msg) => write!(f, "Task execution error: {}", msg),
            JetpackError::Connection(msg) => write!(f, "Connection error: {}", msg),
            JetpackError::Module(msg) => write!(f, "Module error: {}", msg),
            JetpackError::Template(msg) => write!(f, "Template error: {}", msg),
            JetpackError::Io(err) => write!(f, "IO error: {}", err),
            JetpackError::Yaml(err) => write!(f, "YAML error: {}", err),
            JetpackError::Ssh(msg) => write!(f, "SSH error: {}", msg),
            JetpackError::Variable(msg) => write!(f, "Variable error: {}", msg),
            JetpackError::Auth(msg) => write!(f, "Authentication error: {}", msg),
            JetpackError::Other(msg) => write!(f, "Error: {}", msg),
        }
    }
}

impl StdError for JetpackError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            JetpackError::Io(err) => Some(err),
            JetpackError::Yaml(err) => Some(err),
            _ => None,
        }
    }
}

impl From<io::Error> for JetpackError {
    fn from(err: io::Error) -> Self {
        JetpackError::Io(err)
    }
}

impl From<serde_yaml::Error> for JetpackError {
    fn from(err: serde_yaml::Error) -> Self {
        JetpackError::Yaml(err)
    }
}

impl From<String> for JetpackError {
    fn from(err: String) -> Self {
        JetpackError::Other(err)
    }
}

impl From<&str> for JetpackError {
    fn from(err: &str) -> Self {
        JetpackError::Other(err.to_string())
    }
}

/// Result type alias for JetPack operations
pub type Result<T> = std::result::Result<T, JetpackError>;

/// Helper trait to convert old String errors to JetpackError
pub trait ErrorContext<T> {
    fn context(self, context: &str) -> Result<T>;
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String;
}

impl<T> ErrorContext<T> for std::result::Result<T, String> {
    fn context(self, context: &str) -> Result<T> {
        self.map_err(|e| JetpackError::Other(format!("{}: {}", context, e)))
    }
    
    fn with_context<F>(self, f: F) -> Result<T>
    where
        F: FnOnce() -> String,
    {
        self.map_err(|e| JetpackError::Other(format!("{}: {}", f(), e)))
    }
}