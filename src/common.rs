//! What every command shares: where it prints, why it stops, and how it reads a setting.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

/// Where a run writes what it prints.
pub struct Io<'a> {
    pub out: &'a mut dyn Write,
    pub err: &'a mut dyn Write,
}

/// Print a line, ignoring a closed stream.
macro_rules! say {
    ($w:expr, $($arg:tt)*) => {{
        let _ = writeln!($w, $($arg)*);
    }};
}
pub(crate) use say;

/// Why a run stopped short.
#[derive(Clone, Debug, PartialEq)]
pub enum Stop {
    /// The service refused a request, or the run failed before sending one. Exit status 1.
    Failed(String),
    /// The service could not be reached. Exit status 4.
    Unreachable(String),
}

impl Stop {
    pub fn code(&self) -> i32 {
        match self {
            Stop::Failed(_) => 1,
            Stop::Unreachable(_) => 4,
        }
    }
}

impl fmt::Display for Stop {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Stop::Failed(m) | Stop::Unreachable(m) => f.write_str(m),
        }
    }
}

/// The value of the environment variable `name`, trimmed, or why there is none.
pub fn env(name: &str) -> Result<String, Stop> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v.trim().to_string()),
        _ => Err(Stop::Failed(format!("{name} is unset"))),
    }
}

/// `s` with every byte but the unreserved ones percent-encoded, as a URL's path segment must be.
pub fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-._~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

pub fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_millis() as u64)
}

/// The text of a --file, or else of a --text.
pub fn text(file: Option<&Path>, text: Option<&str>) -> Result<String, Stop> {
    match file {
        Some(path) => std::fs::read_to_string(path)
            .map_err(|e| Stop::Failed(format!("Failed to read {}: {e}", path.display()))),
        None => Ok(text.unwrap_or_default().to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_keeps_only_unreserved_bytes() {
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn env_trims_the_value_and_refuses_an_unset_one() {
        // SAFETY: no other test reads or writes these variables.
        unsafe { std::env::set_var("UPSTASH_ARRIBA_TEST_SET", " v ") };
        assert_eq!(env("UPSTASH_ARRIBA_TEST_SET"), Ok("v".into()));
        let unset = Stop::Failed("UPSTASH_ARRIBA_TEST_UNSET is unset".into());
        assert_eq!(env("UPSTASH_ARRIBA_TEST_UNSET"), Err(unset));
    }

    #[test]
    fn text_reads_the_file_or_else_the_text() {
        assert_eq!(text(None, Some("x")), Ok("x".into()));
        let missing = Path::new("/nonexistent/upstash-arriba-note");
        assert!(matches!(text(Some(missing), None), Err(Stop::Failed(_))));
    }
}
