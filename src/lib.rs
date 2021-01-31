extern crate thiserror;

mod runner;
mod system;

pub mod common;
pub mod lookup_path;
pub mod vcx;
mod pe;
mod executable;
mod query;

pub use executable::{Executable, Executables};
pub use query::LookupQuery;
pub use common::{LookupError, readable_canonical_path, path_to_string, osstring_to_string, decanonicalize};
pub use pe::demangle_symbol;
pub use lookup_path::LookupPath;

pub fn lookup(query: &LookupQuery, context: LookupPath) -> Result<Executables, LookupError> {
    let mut wq = runner::Runner::new(query, context);
    wq.run()
}
