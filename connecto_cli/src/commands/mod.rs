//! CLI command implementations

pub mod keygen;
pub mod keys;
pub mod listen;
pub mod pair;
pub mod scan;

use colored::Colorize;

/// Print a success message
pub fn success(msg: &str) {
    println!("{} {}", "✓".green().bold(), msg);
}

/// Print an error message
pub fn error(msg: &str) {
    eprintln!("{} {}", "✗".red().bold(), msg);
}

/// Print an info message
pub fn info(msg: &str) {
    println!("{} {}", "→".cyan().bold(), msg);
}

/// Print a warning message
pub fn warn(msg: &str) {
    println!("{} {}", "!".yellow().bold(), msg);
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_module_exists() {
        // Just verify the module compiles
        assert!(true);
    }
}
