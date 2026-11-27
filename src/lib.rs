mod host;
mod model;
mod package;
mod parser;

pub use host::{Host as AppInstance, run};
pub use package::MAX_BYTES as MAX_PACKAGE_BYTES;

pub fn check_source(source: &str) -> Result<(), String> {
    parser::compile(source).map(|_| ())
}

pub fn compile_source(source: &str) -> Result<Vec<u8>, String> {
    package::build(&parser::compile(source)?)
}

#[cfg(test)]
mod tests;
