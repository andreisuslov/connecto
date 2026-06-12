//! Device name sanitization
//!
//! Turns arbitrary device names (mDNS instance names, hostnames, user-chosen
//! labels) into safe SSH host aliases. This is the single sanitization policy
//! shared by the CLI and GUI, replacing the divergent ad-hoc sanitizers that
//! previously lived in each frontend.

/// Sanitize a device name into a safe SSH host alias
///
/// The policy is deliberately the most defensive of the historical variants:
///
/// - lowercase the input
/// - map any character that is not `[a-z0-9-]` to `-`
/// - collapse consecutive `-`
/// - trim leading and trailing `-`
/// - cap the result at 40 characters
/// - return `"device"` if nothing remains
pub fn sanitize_device_name(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;

    for c in name.to_lowercase().chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c);
            prev_dash = false;
        } else if !prev_dash && !out.is_empty() {
            // Collapse runs of disallowed characters (and '-') into a single
            // dash, and never start with one.
            out.push('-');
            prev_dash = true;
        }
    }

    while out.ends_with('-') {
        out.pop();
    }

    // The string is pure ASCII at this point, so byte truncation is safe.
    out.truncate(40);
    while out.ends_with('-') {
        out.pop();
    }

    if out.is_empty() {
        "device".to_string()
    } else {
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_spaces_become_dashes() {
        assert_eq!(sanitize_device_name("My Device"), "my-device");
        assert_eq!(
            sanitize_device_name("Andrei's MacBook Pro"),
            "andrei-s-macbook-pro"
        );
    }

    #[test]
    fn test_dots_become_dashes() {
        assert_eq!(sanitize_device_name("host.local"), "host-local");
    }

    #[test]
    fn test_parens_collapse() {
        assert_eq!(sanitize_device_name("Host (123)"), "host-123");
    }

    #[test]
    fn test_underscores_become_dashes() {
        assert_eq!(sanitize_device_name("my_device_1"), "my-device-1");
    }

    #[test]
    fn test_existing_dashes_kept() {
        assert_eq!(sanitize_device_name("test-host"), "test-host");
    }

    #[test]
    fn test_consecutive_separators_collapse() {
        assert_eq!(sanitize_device_name("--a--b--"), "a-b");
        assert_eq!(sanitize_device_name("a ... b"), "a-b");
    }

    #[test]
    fn test_unicode_maps_to_dashes() {
        assert_eq!(sanitize_device_name("Štěpán's Mac"), "t-p-n-s-mac");
        assert_eq!(sanitize_device_name("日本語"), "device");
    }

    #[test]
    fn test_empty_and_symbol_only_fall_back() {
        assert_eq!(sanitize_device_name(""), "device");
        assert_eq!(sanitize_device_name("!!!"), "device");
        assert_eq!(sanitize_device_name("---"), "device");
    }

    #[test]
    fn test_capped_at_40_chars() {
        let long = "a".repeat(60);
        assert_eq!(sanitize_device_name(&long).len(), 40);

        // A dash landing on the cap boundary is trimmed.
        let name = format!("{}-{}", "a".repeat(39), "b".repeat(10));
        let sanitized = sanitize_device_name(&name);
        assert_eq!(sanitized, "a".repeat(39));
        assert!(!sanitized.ends_with('-'));
    }

    #[test]
    fn test_uppercase_is_lowered() {
        assert_eq!(sanitize_device_name("MACBOOK"), "macbook");
    }
}
