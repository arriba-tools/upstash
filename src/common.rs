//! What every command shares: where it prints, why it stops, and how it reads a setting.

use std::fmt;
use std::io::Write;
use std::path::Path;
use std::process::Command;
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

/// The value of the setting `name`. It is the environment variable `name`, or else the Actions
/// variable `name` of `repo`, by default the repository gh resolves here.
pub fn setting(name: &str, repo: Option<&str>) -> Result<String, Stop> {
    match std::env::var(name) {
        Ok(v) if !v.trim().is_empty() => Ok(v),
        _ => variable(name, &resolve(name, repo)?),
    }
}

/// `repo`, or the repository gh resolves for the working directory.
fn resolve(name: &str, repo: Option<&str>) -> Result<String, Stop> {
    match repo {
        Some(r) => Ok(r.to_string()),
        None => gh(&[
            "repo",
            "view",
            "--json",
            "nameWithOwner",
            "--jq",
            ".nameWithOwner",
        ])
        .map_err(|e| {
            Stop::Failed(format!(
                "No repository to read {name} from, so pass -R OWNER/REPO: {e}"
            ))
        }),
    }
}

fn variable(name: &str, repo: &str) -> Result<String, Stop> {
    gh(&[
        "api",
        &format!("repos/{repo}/actions/variables/{name}"),
        "--jq",
        ".value",
    ])
    .map_err(|e| {
        Stop::Failed(format!(
            "Reading the Actions variable {name} of {repo} failed: {e}"
        ))
    })
}

fn gh(args: &[&str]) -> Result<String, String> {
    let out = Command::new("gh")
        .args(args)
        .output()
        .map_err(|e| format!("gh: {e}"))?;
    if !out.status.success() {
        return Err(String::from_utf8_lossy(&out.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// The token an Upstash REST URL carries as its _token parameter, as written.
pub fn token(url: &str) -> Option<&str> {
    url.split_once("_token=")
        .map(|(_, t)| t.split('&').next().unwrap_or_default())
        .filter(|t| !t.is_empty())
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
    fn token_is_the_tokens_parameter_as_written() {
        assert_eq!(token("https://h?_token=Ab+c/d=&y=1"), Some("Ab+c/d="));
        assert_eq!(token("https://h?y=1&_token=t"), Some("t"));
        assert_eq!(token("https://h?_token="), None);
        assert_eq!(token("https://h"), None);
    }

    #[test]
    fn encode_keeps_only_unreserved_bytes() {
        assert_eq!(encode("a-b_c.d~e"), "a-b_c.d~e");
        assert_eq!(encode("a/b c"), "a%2Fb%20c");
    }

    #[test]
    fn a_setting_comes_from_the_environment_first() {
        // SAFETY: no other test reads or writes this variable.
        unsafe { std::env::set_var("UPSTASH_ARRIBA_TEST_SETTING", " v ") };
        assert_eq!(
            setting("UPSTASH_ARRIBA_TEST_SETTING", Some("o/r")),
            Ok(" v ".into())
        );
    }

    #[test]
    fn text_reads_the_file_or_else_the_text() {
        assert_eq!(text(None, Some("x")), Ok("x".into()));
        let missing = Path::new("/nonexistent/upstash-arriba-note");
        assert!(matches!(text(Some(missing), None), Err(Stop::Failed(_))));
    }
}
