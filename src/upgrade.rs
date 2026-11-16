//! Replaces the running binary with the one the latest release built for this platform.

use self_update::{VersionStatus, backends::github::Update};

use crate::common::{Io, Stop, say};

/// The platform suffix the release workflow gives this machine's asset, as in
/// `upstash-arriba-darwin-arm64`, or None when no release is built for it.
pub fn platform(os: &str, arch: &str) -> Option<&'static str> {
    match (os, arch) {
        ("macos", "aarch64") => Some("darwin-arm64"),
        ("macos", "x86_64") => Some("darwin-amd64"),
        ("linux", "x86_64") => Some("linux-amd64"),
        ("linux", "aarch64") => Some("linux-arm64"),
        ("windows", "x86_64") => Some("windows-amd64"),
        _ => None,
    }
}

pub fn upgrade(io: &mut Io) -> Result<i32, Stop> {
    let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
    let target = platform(os, arch)
        .ok_or_else(|| Stop::Failed(format!("No release is built for {os} on {arch}")))?;
    let failed = |e: self_update::errors::Error| Stop::Failed(format!("Upgrade failed: {e}"));
    let status = Update::configure()
        .repo_owner("arriba-tools")
        .repo_name("upstash")
        .bin_name("upstash-arriba")
        // The asset is the one whose name contains the platform suffix.
        .target(target)
        .current_version(env!("CARGO_PKG_VERSION"))
        .show_output(false)
        .show_download_progress(false)
        .no_confirm(true)
        .build()
        .map_err(failed)?
        .update()
        .map_err(failed)?;
    let v = status.version();
    if matches!(status, VersionStatus::Updated(_)) {
        say!(io.out, "Upgraded upstash-arriba to {v}");
    } else {
        say!(io.out, "upstash-arriba {v} is the latest release");
    }
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn platform_names_each_released_asset() {
        assert_eq!(platform("macos", "aarch64"), Some("darwin-arm64"));
        assert_eq!(platform("macos", "x86_64"), Some("darwin-amd64"));
        assert_eq!(platform("linux", "x86_64"), Some("linux-amd64"));
        assert_eq!(platform("linux", "aarch64"), Some("linux-arm64"));
        assert_eq!(platform("windows", "x86_64"), Some("windows-amd64"));
        assert_eq!(platform("windows", "aarch64"), None);
    }

    #[test]
    fn this_platform_has_a_release() {
        let (os, arch) = (std::env::consts::OS, std::env::consts::ARCH);
        assert!(platform(os, arch).is_some(), "{os} on {arch}");
    }
}
