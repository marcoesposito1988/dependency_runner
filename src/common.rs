use thiserror::Error;

use serde::Serialize;
use std::collections::hash_map::Values;

#[derive(Error, Debug)]
pub enum LookupError {
    #[error("Read error")]
    CouldNotOpenFile { source: std::io::Error },

    #[error("PE file parse error")]
    ProcessingError { source: pelite::Error },

    #[error("Visual Studio User settings file parse error")]
    ParseError(String),

    #[error("Lookup context building error")]
    ContextDeductionError(String),

    #[error(transparent)]
    RegexError(#[from] regex::Error),
    #[error(transparent)]
    IOError(#[from] std::io::Error),
    #[error(transparent)]
    PEError(#[from] pelite::Error),
}

#[derive(Debug)]
pub struct LookupQuery {
    pub name: String,
    pub depth: usize,
}

#[derive(Debug, Clone, Serialize)]
pub struct ExecutableDetails {
    pub is_system: bool,
    pub folder: String,
    pub dependencies: Option<Vec<String>>,
}

#[derive(Debug, Clone, Serialize)]
pub struct Executable {
    pub name: String,
    pub depth_first_appearance: usize,
    pub found: bool,
    pub details: Option<ExecutableDetails>,
}

impl Executable {
    pub fn full_path(&self) -> String {
        if let Some(details) = &self.details {
            details.folder.clone() + &self.name
        } else {
            self.name.clone()
        }
    }
}

#[derive(Debug, Clone)]
pub struct LookupResult {
    index: std::collections::HashMap<String, Executable>,
}

impl LookupResult {
    pub fn new() -> Self {
        Self {
            index: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, lr: Executable) {
        self.index.insert(name.to_lowercase(), lr);
    }

    pub fn get(&self, name: &str) -> Option<&Executable> {
        self.index.get(&name.to_lowercase())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_lowercase())
    }

    pub fn values(&self) -> Values<'_, String, Executable> {
        self.index.values()
    }
}
