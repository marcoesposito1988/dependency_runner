extern crate thiserror;

mod apiset;
mod runner;
mod system;

pub mod common;
mod executable;
pub mod lookup_path;
mod pe;
mod query;
pub mod vcx;

pub use common::{
    decanonicalize, osstring_to_string, path_to_string, readable_canonical_path, LookupError,
};
pub use executable::{Executable, Executables};
pub use lookup_path::LookupPath;
pub use pe::demangle_symbol;
pub use query::LookupQuery;
pub use system::WindowsSystem;

pub fn lookup(query: &LookupQuery, context: LookupPath) -> Result<Executables, LookupError> {
    let mut wq = runner::Runner::new(query, context);
    wq.run()
}
