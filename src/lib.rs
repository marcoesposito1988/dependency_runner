extern crate thiserror;

mod workqueue;
mod system;

pub mod common;
pub mod context;
pub mod vcx;
pub mod models;
pub use common::{Executable, Executables, LookupError, Query};
pub use common::{readable_canonical_path, path_to_string, osstring_to_string, decanonicalize};
pub use context::Context;

pub fn lookup(query: &Query, context: Context) -> Result<Executables, LookupError> {
    let mut wq = workqueue::Workqueue::new(query, context);
    wq.run()
}
