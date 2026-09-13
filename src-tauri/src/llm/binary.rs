//! Finding llama-server, and which device to give it.
//!
//! A bundled copy ships with the app so a user never learns what llama-server
//! is; an explicit path still wins, so one who does can use their own build.

use std::path::{Path, PathBuf};
use std::process::Command;

const EXE: &str = if cfg!(windows) {
    "llama-server.exe"
} else {
    "llama-server"
};

/// One entry from `--list-devices`.
#[derive(Debug, Clone, PartialEq)]
pub struct Device {
    /// What `--device` expects, e.g. `Vulkan1`.
    pub id: String,
    pub name: String,
    pub total_mib: u64,
    pub free_mib: u64,
}

/// Explicit setting, then the bundled copy, then beside the executable. PATH is
/// `on_path` and deliberately separate, so this answer does not depend on a
/// machine's environment.
pub fn resolve(explicit: Option<&Path>, bundled_dir: Option<&Path>) -> Option<PathBuf> {
    [
        explicit.map(|p| p.to_path_buf()),
        bundled_dir.map(|d| d.join(EXE)),
        std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join(EXE))),
    ]
    .into_iter()
    .flatten()
    .find(|p| p.is_file())
}

/// Last resort, for a development machine with llama.cpp already installed.
pub fn on_path() -> Option<PathBuf> {
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|d| d.join(EXE))
        .find(|p| p.is_file())
}

/// Parses `--list-devices`. Unknown lines are skipped rather than failing: the
/// format is a human-readable listing and not a contract.
pub fn parse_devices(output: &str) -> Vec<Device> {
    output
        .lines()
        .filter_map(|line| {
            let (id, rest) = line.trim().split_once(": ")?;
            if id.is_empty() || id.contains(' ') {
                return None;
            }
            // Last paren: a name can contain one, as "Radeon(TM) Graphics" does.
            let open = rest.rfind('(')?;
            let inside = rest[open + 1..].strip_suffix(')')?;
            let (total, free) = inside.split_once(',')?;
            Some(Device {
                id: id.to_string(),
                name: rest[..open].trim().to_string(),
                total_mib: total.trim().strip_suffix(" MiB")?.parse().ok()?,
                free_mib: free.trim().strip_suffix(" MiB free")?.parse().ok()?,
            })
        })
        .collect()
}

/// By name, because no backend reports the distinction. Wrong on an unknown
/// integrated part, which costs speed rather than correctness.
fn integrated(name: &str) -> bool {
    let n = name.to_lowercase();
    n.contains("radeon(tm) graphics")
        || n.contains("iris")
        || n.contains("uhd graphics")
        || n.contains("hd graphics")
        || n.contains("integrated")
}

/// The device worth offloading to: most free memory wins, but an integrated GPU
/// loses to a discrete one of any size, because its memory is shared system RAM
/// at system-RAM speed.
pub fn best_device(devices: &[Device]) -> Option<&Device> {
    let discrete: Vec<&Device> = devices.iter().filter(|d| !integrated(&d.name)).collect();
    if discrete.is_empty() {
        return devices.iter().max_by_key(|d| d.free_mib);
    }
    discrete.into_iter().max_by_key(|d| d.free_mib)
}

pub fn devices(binary: &Path) -> Vec<Device> {
    // Short-lived, but still a console flash on every device probe.
    super::without_a_console(Command::new(binary).arg("--list-devices"))
        .output()
        .ok()
        .map(|o| {
            let mut text = String::from_utf8_lossy(&o.stdout).to_string();
            text.push_str(&String::from_utf8_lossy(&o.stderr));
            parse_devices(&text)
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Verbatim from build 10897 on the development machine.
    const LISTING: &str = "Available devices:\n  \
        Vulkan0: AMD Radeon(TM) Graphics (8886 MiB, 8441 MiB free)\n  \
        Vulkan1: NVIDIA GeForce RTX 3050 Laptop GPU (3965 MiB, 3370 MiB free)\n";

    fn temp_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("parallax-bin-{tag}-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn fake_exe(dir: &Path) -> PathBuf {
        let at = dir.join(EXE);
        std::fs::write(&at, b"not really a binary").unwrap();
        at
    }

    #[test]
    fn the_listing_parses() {
        let found = parse_devices(LISTING);
        assert_eq!(found.len(), 2);
        assert_eq!(found[0].id, "Vulkan0");
        assert_eq!(found[0].name, "AMD Radeon(TM) Graphics");
        assert_eq!(found[0].total_mib, 8886);
        assert_eq!(found[0].free_mib, 8441);
        assert_eq!(found[1].id, "Vulkan1");
        assert_eq!(found[1].name, "NVIDIA GeForce RTX 3050 Laptop GPU");
        assert_eq!(found[1].free_mib, 3370);
    }

    #[test]
    fn a_listing_with_no_devices_is_empty() {
        assert!(parse_devices("Available devices:\n").is_empty());
        assert!(parse_devices("").is_empty());
    }

    /// The whole reason device selection exists: the integrated GPU reports more
    /// free memory than the discrete card and is much slower.
    #[test]
    fn a_discrete_card_beats_a_larger_integrated_one() {
        let found = parse_devices(LISTING);
        let best = best_device(&found).unwrap();
        assert_eq!(
            best.id, "Vulkan1",
            "offloading to shared system RAM is not offloading"
        );
    }

    #[test]
    fn the_larger_of_two_discrete_cards_wins() {
        let devices = vec![
            Device {
                id: "Vulkan0".into(),
                name: "NVIDIA GeForce RTX 3050 Laptop GPU".into(),
                total_mib: 3965,
                free_mib: 3370,
            },
            Device {
                id: "Vulkan1".into(),
                name: "NVIDIA GeForce RTX 4090".into(),
                total_mib: 24564,
                free_mib: 24000,
            },
        ];
        assert_eq!(best_device(&devices).unwrap().id, "Vulkan1");
    }

    #[test]
    fn no_devices_means_no_choice() {
        assert!(best_device(&[]).is_none());
    }

    #[test]
    fn an_explicit_path_wins() {
        let chosen = temp_dir("explicit");
        let bundled = temp_dir("bundled");
        let at = fake_exe(&chosen);
        fake_exe(&bundled);
        assert_eq!(resolve(Some(&at), Some(&bundled)), Some(at));
    }

    #[test]
    fn the_bundled_copy_is_used_when_nothing_is_set() {
        let bundled = temp_dir("only-bundled");
        let at = fake_exe(&bundled);
        assert_eq!(resolve(None, Some(&bundled)), Some(at));
    }

    /// A stale setting must not hide the copy that ships with the app.
    #[test]
    fn an_explicit_path_that_is_gone_falls_through() {
        let bundled = temp_dir("fallthrough");
        let at = fake_exe(&bundled);
        let missing = bundled.join("nowhere").join(EXE);
        assert_eq!(resolve(Some(&missing), Some(&bundled)), Some(at));
    }

    #[test]
    fn nothing_anywhere_is_none() {
        let empty = temp_dir("empty");
        assert_eq!(resolve(None, Some(&empty)), None);
    }
}
