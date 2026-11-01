// Statement processor modules
// Each module implements methods on the Transpiler struct for processing specific statement types

mod assignment;
mod command_statement;
mod const_processor;
mod entity_processor;
mod execute_processor;
mod function_processor;
mod if_processor;
mod import_processor;
mod loop_processor;
mod match_processor;
mod selector_processor;

pub(in crate::transpiler) use assignment::{out_of_range_numbers, serialize_snbt};
pub(in crate::transpiler) use import_processor::ImportState;
pub(in crate::transpiler) use loop_processor::{calls_user_function, ActiveListLoop, UnrollState};
