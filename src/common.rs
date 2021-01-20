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
pub struct LookupResult {
    pub name: String,
    pub depth_first_appearance: usize,
    pub is_system: Option<bool>,
    pub folder: Option<String>,
    pub dependencies: Option<Vec<String>>,
}

#[derive(Debug, Clone)]
pub struct Executables {
    index: std::collections::HashMap<String, LookupResult>,
}

impl Executables {
    pub fn new() -> Self {
        Self {
            index: std::collections::HashMap::new(),
        }
    }

    pub fn insert(&mut self, name: &str, lr: LookupResult) {
        self.index.insert(name.to_lowercase(), lr);
    }

    pub fn get(&self, name: &str) -> Option<&LookupResult> {
        self.index.get(&name.to_lowercase())
    }

    pub fn contains(&self, name: &str) -> bool {
        self.index.contains_key(&name.to_lowercase())
    }

    pub fn values(&self) -> Values<'_, String, LookupResult> {
        self.index.values()
    }
}
