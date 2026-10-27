//! Native updater identity and permission policy. Release tooling consumes the same target catalog.

use serde::{Deserialize, Serialize};
use tauri::{ipc::CapabilityBuilder, Manager, State};

// Tauri reports App on macOS even for a raw executable. That fallback alone
// cannot authorize replacing an application bundle.
#[cfg(any(target_os = "macos", test))]
fn is_app_bundle(executable: &std::path::Path) -> bool {
    let Some(macos) = executable.parent() else {
        return false;
    };
    let Some(contents) = macos.parent() else {
        return false;
    };
    let Some(app) = contents.parent() else {
        return false;
    };
    macos.file_name() == Some(std::ffi::OsStr::new("MacOS"))
        && contents.file_name() == Some(std::ffi::OsStr::new("Contents"))
        && app.extension() == Some(std::ffi::OsStr::new("app"))
        && executable.is_file()
        && contents.join("Info.plist").is_file()
        && contents.join("Resources").is_dir()
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReleaseTarget {
    os: String,
    arch: String,
    bundle: String,
    target: String,
    query_default: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
enum UpdaterMode {
    Installed,
    QueryOnly,
    Unavailable,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct UpdaterPolicy {
    mode: UpdaterMode,
    target: Option<String>,
}

impl UpdaterPolicy {
    fn resolve(debug: bool, os: &str, arch: &str, bundle: Option<&str>) -> Self {
        let targets: Vec<ReleaseTarget> =
            serde_json::from_str(include_str!("../updater-targets.json"))
                .expect("invalid canonical updater targets");
        let supported = |entry: &&ReleaseTarget| entry.os == os && entry.arch == arch;
        if !debug {
            if let Some(entry) = targets
                .iter()
                .filter(supported)
                .find(|entry| Some(entry.bundle.as_str()) == bundle)
            {
                return Self {
                    mode: UpdaterMode::Installed,
                    target: Some(entry.target.clone()),
                };
            }
        }
        match targets
            .iter()
            .filter(supported)
            .find(|entry| entry.query_default)
        {
            Some(entry) => Self {
                mode: UpdaterMode::QueryOnly,
                target: Some(entry.target.clone()),
            },
            None => Self {
                mode: UpdaterMode::Unavailable,
                target: None,
            },
        }
    }

    pub(crate) fn configure<R: tauri::Runtime>(app: &tauri::App<R>) -> tauri::Result<()> {
        use tauri::utils::config::BundleType;
        let bundle = match tauri::utils::platform::bundle_type() {
            Some(BundleType::Nsis) => Some("nsis"),
            Some(BundleType::Deb) => Some("deb"),
            Some(BundleType::Rpm) => Some("rpm"),
            Some(BundleType::AppImage) => Some("appimage"),
            #[cfg(target_os = "macos")]
            Some(BundleType::App)
                if tauri::utils::platform::current_exe()
                    .map(|path| is_app_bundle(path))
                    .unwrap_or(false) =>
            {
                Some("app")
            }
            _ => None,
        };
        let policy = Self::resolve(
            cfg!(debug_assertions),
            std::env::consts::OS,
            std::env::consts::ARCH,
            bundle,
        );
        // Static capabilities allow only check. Never grant combined download/install.
        if policy.mode == UpdaterMode::Installed {
            app.add_capability(
                CapabilityBuilder::new("installed-updater")
                    .window("main")
                    .permission("updater:allow-download")
                    .permission("updater:allow-install"),
            )?;
        }
        app.manage(policy);
        Ok(())
    }
}

#[tauri::command]
pub(crate) fn get_updater_policy(policy: State<'_, UpdaterPolicy>) -> UpdaterPolicy {
    policy.inner().clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_builds_never_receive_install_permissions() {
        let targets: Vec<ReleaseTarget> =
            serde_json::from_str(include_str!("../updater-targets.json")).unwrap();
        for entry in &targets {
            let debug = UpdaterPolicy::resolve(true, &entry.os, &entry.arch, Some(&entry.bundle));
            assert_eq!(debug.mode, UpdaterMode::QueryOnly);
            let installed =
                UpdaterPolicy::resolve(false, &entry.os, &entry.arch, Some(&entry.bundle));
            assert_eq!(installed.mode, UpdaterMode::Installed);
            assert_eq!(installed.target.as_deref(), Some(entry.target.as_str()));
        }
    }

    #[test]
    fn unknown_bundle_is_query_only_and_unsupported_platform_is_unavailable() {
        let policy = UpdaterPolicy::resolve(false, "windows", "x86_64", None);
        assert_eq!(policy.mode, UpdaterMode::QueryOnly);
        assert_eq!(policy.target.as_deref(), Some("windows-x86_64-nsis"));
        let unsupported = UpdaterPolicy::resolve(false, "windows", "aarch64", Some("nsis"));
        assert_eq!(unsupported.mode, UpdaterMode::Unavailable);
        assert!(unsupported.target.is_none());
    }

    #[test]
    fn macos_requires_real_bundle_structure_not_the_platform_fallback() {
        let temp = tempfile::tempdir().unwrap();
        for directory in [
            "TauTerm.app/Contents/MacOS",
            "target/release",
            "NotAnApp/Contents/MacOS",
            "TauTerm.app/Contents/MacOS/nested",
            "Misleading.app/FakeContents/MacOS",
        ] {
            let executable = temp.path().join(directory).join("tauterm");
            std::fs::create_dir_all(executable.parent().unwrap()).unwrap();
            std::fs::write(&executable, []).unwrap();
            assert!(!is_app_bundle(&executable));
        }
        let contents = temp.path().join("TauTerm.app/Contents");
        std::fs::write(contents.join("Info.plist"), []).unwrap();
        std::fs::create_dir(contents.join("Resources")).unwrap();
        assert!(is_app_bundle(&contents.join("MacOS/tauterm")));
        assert!(!is_app_bundle(&contents.join("MacOS/nested/tauterm")));
    }
}
