extern crate thiserror;

pub use crate::common::{Executable, Executables, LookupError, Query};
pub use crate::context::Context;

mod common;
pub mod external;
pub mod system;
mod workqueue; // TODO make private

pub mod context;
pub mod models;

pub fn lookup(query: Query) -> Result<Executables, LookupError> {
    let mut wq = workqueue::Workqueue::new(query);
    wq.run()
}
