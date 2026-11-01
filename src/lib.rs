//! Rust Base Template Library.

/// Returns the default greeting message.
#[must_use]
pub fn greeting() -> &'static str {
    "Hello World!"
}

/// Main entry point logic for the CLI application.
///
/// # Errors
///
/// Returns an error if standard output operations fail.
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    println!("{}", greeting());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_greeting() {
        assert_eq!(greeting(), "Hello World!");
    }

    #[test]
    fn test_run() {
        assert!(run().is_ok());
    }
}
