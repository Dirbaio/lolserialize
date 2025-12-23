mod checker;
mod codegen;
mod codegen_go;
mod parser;
mod schema;

pub use checker::{CheckError, check, print_errors as print_check_errors};
pub use codegen::generate;
pub use codegen_go::generate as generate_go;
pub use parser::{parse, print_errors as print_parse_errors};
pub use schema::*;
