pub mod ast;
pub mod commands;
pub mod config;
pub mod diagnostics;
pub mod error;
pub(crate) mod fs_safety;
pub mod pack_format;
pub mod parser;
pub mod stdlib;
pub mod transpiler;
pub mod validator;

#[cfg(test)]
pub(crate) mod test_support {
    use std::sync::{Mutex, OnceLock};

    pub(crate) fn cwd_lock() -> &'static Mutex<()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(()))
    }
}
