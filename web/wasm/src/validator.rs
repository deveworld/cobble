//! The parts of the validator the compiler uses. The rest reads files and
//! carries Minecraft's registries, so it stays out of the browser build.

#[path = "../../../src/validator/snbt.rs"]
pub mod snbt;
#[path = "../../../src/validator/string_reader.rs"]
pub mod string_reader;

pub mod registries {
    /// Tag entries are looked up by `cobble build`.
    pub fn tag_entry_problem(_tag_type: &str, _id: &str) -> Option<String> {
        None
    }

    /// Minecraft's own ids aren't built in, so resources aren't checked
    /// against them.
    pub(crate) fn vanilla_ids(
        _registry: &str,
    ) -> Option<&'static std::collections::HashSet<String>> {
        None
    }
}
