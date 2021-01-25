extern crate thiserror;

mod runner;
mod system;

pub mod common;
pub mod lookup_path;
pub mod vcx;
pub mod models;
pub use common::{Executable, Executables, LookupError, LookupQuery};
pub use common::{readable_canonical_path, path_to_string, osstring_to_string, decanonicalize};
pub use lookup_path::LookupPath;

pub fn lookup(query: &LookupQuery, context: LookupPath) -> Result<Executables, LookupError> {
    let mut wq = runner::Runner::new(query, context);
    wq.run()
}
