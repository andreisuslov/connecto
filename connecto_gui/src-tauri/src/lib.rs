//! Connecto GUI Library - Tauri backend

pub mod commands;
pub mod state;

pub use commands::*;
pub use state::*;

#[cfg(test)]
mod tests {
    #[test]
    fn test_lib_compiles() {
        assert!(true);
    }
}
