extern crate thiserror;

mod runner;
mod system;

pub mod common;
pub mod lookup_path;
pub mod vcx;
pub mod models;
mod executable;
mod query;

pub use executable::{Executable, Executables};
pub use query::LookupQuery;
pub use common::{LookupError, readable_canonical_path, path_to_string, osstring_to_string, decanonicalize};
pub use lookup_path::LookupPath;

pub fn lookup(query: &LookupQuery, context: LookupPath) -> Result<Executables, LookupError> {
    let mut wq = runner::Runner::new(query, context);
    wq.run()
}
