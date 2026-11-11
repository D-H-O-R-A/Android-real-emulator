use std::collections::BTreeMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostOs {
    Linux,
    MacOs,
    Windows,
    Other(String),
}

impl HostOs {
    pub fn detect() -> Self {
        match env::consts::OS {
            "linux" => HostOs::Linux,
            "macos" => HostOs::MacOs,
            "windows" => HostOs::Windows,
            other => HostOs::Other(other.to_string()),
        }
    }

    pub fn display_name(&self) -> &str {
        match self {
            HostOs::Linux => "Linux",
            HostOs::MacOs => "macOS",
            HostOs::Windows => "Windows",
            HostOs::Other(s) => s.as_str(),
        }
    }

    pub fn hypervisor_name(&self) -> &str {
        match self {
            HostOs::Linux => "KVM (/dev/kvm)",
            HostOs::MacOs => "Hypervisor.framework (HVF)",
            HostOs::Windows => "Windows Hypervisor Platform (WHPX / AEHD)",
            HostOs::Other(_) => "QEMU TCG/Hardware Virtualization",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SystemImageInfo {
    /// Relative SDK path, e.g. `system-images/android-35/google_apis_playstore/x86_64/`
    pub relative_sysdir: String,
    pub full_path: PathBuf,
    pub api_level: u32,
    pub android_release: String,
    pub tag_id: String,
    pub primary_abi: String,
    pub supported_abis: Vec<String>,
    pub has_play_store: bool,
    pub has_arm64_translation: bool,
    pub has_arm32_translation: bool,
    pub native_bridge: Option<String>,
}

impl SystemImageInfo {
    /// Returns true if rootAVD has patched this image (indicated by ramdisk.img.backup).
    pub fn is_magisk_patched(&self) -> bool {
        self.full_path.join("ramdisk.img.backup").exists()
    }

    /// Score this image for compatibility with the host architecture and Play Store requirements.
    pub fn compatibility_score(&self, host_arch: &str) -> i32 {
        let mut score = 0;
        if self.has_play_store {
            score += 1000;
        }
        // Native CPU match is critical for hardware hypervisor (KVM / HVF / WHPX)
        if self.primary_abi == host_arch
            || (host_arch == "x86_64" && self.primary_abi == "x86")
            || (host_arch == "aarch64" && self.primary_abi == "arm64-v8a")
        {
            score += 500;
        } else {
            // Mismatched primary ABI forces slow software emulation
            score -= 400;
        }
        if self.has_arm64_translation || self.primary_abi == "arm64-v8a" {
            score += 300;
        }
        if self.has_arm32_translation {
            score += 100;
        }
        score += self.api_level as i32;
        score
    }
}

#[derive(Debug, Clone)]
pub struct HostReport {
    pub os: HostOs,
    pub arch: String,
    pub cpu_cores: usize,
    pub hypervisor_available: bool,
    pub hypervisor_detail: String,
    pub sdk_root: Option<PathBuf>,
    pub avd_home: PathBuf,
    pub emulator_bin: Option<PathBuf>,
    pub adb_bin: Option<PathBuf>,
    pub sdkmanager_bin: Option<PathBuf>,
    pub qemu_bin: Option<PathBuf>,
    pub rootavd_dir: Option<PathBuf>,
    pub webcams: Vec<String>,
    pub installed_images: Vec<SystemImageInfo>,
}

impl HostReport {
    pub fn detect() -> Self {
        let os = HostOs::detect();
        let arch = env::consts::ARCH.to_string();
        let cpu_cores = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(4);

        let (hypervisor_available, hypervisor_detail) = detect_hypervisor(&os);
        let sdk_root = detect_sdk_root(&os);
        let avd_home = detect_avd_home();

        let exe_suffix = if os == HostOs::Windows { ".exe" } else { "" };
        let bat_suffix = if os == HostOs::Windows { ".bat" } else { "" };

        let emulator_bin = sdk_root
            .as_ref()
            .map(|r| r.join("emulator").join(format!("emulator{exe_suffix}")))
            .filter(|p| p.exists())
            .or_else(|| which_bin(&format!("emulator{exe_suffix}")));

        let adb_bin = sdk_root
            .as_ref()
            .map(|r| r.join("platform-tools").join(format!("adb{exe_suffix}")))
            .filter(|p| p.exists())
            .or_else(|| which_bin(&format!("adb{exe_suffix}")));

        let sdkmanager_bin = sdk_root
            .as_ref()
            .and_then(|r| {
                let latest = r
                    .join("cmdline-tools")
                    .join("latest")
                    .join("bin")
                    .join(format!("sdkmanager{bat_suffix}"));
                if latest.exists() {
                    return Some(latest);
                }
                let tools = r
                    .join("tools")
                    .join("bin")
                    .join(format!("sdkmanager{bat_suffix}"));
                if tools.exists() {
                    return Some(tools);
                }
                None
            })
            .or_else(|| which_bin(&format!("sdkmanager{bat_suffix}")));

        let qemu_bin_name = if arch == "aarch64" {
            format!("qemu-system-aarch64{exe_suffix}")
        } else {
            format!("qemu-system-x86_64{exe_suffix}")
        };
        let qemu_bin = which_bin(&qemu_bin_name);
        let rootavd_dir = crate::magisk::find_existing_rootavd();

        let webcams = detect_webcams(&os);
        let installed_images = sdk_root
            .as_ref()
            .map(|r| scan_system_images(r))
            .unwrap_or_default();

        HostReport {
            os,
            arch,
            cpu_cores,
            hypervisor_available,
            hypervisor_detail,
            sdk_root,
            avd_home,
            emulator_bin,
            adb_bin,
            sdkmanager_bin,
            qemu_bin,
            rootavd_dir,
            webcams,
            installed_images,
        }
    }

    /// Select the best installed system image for running Play Store apps with hardware acceleration.
    pub fn best_playstore_image(&self) -> Option<&SystemImageInfo> {
        self.installed_images
            .iter()
            .filter(|img| img.has_play_store)
            .max_by_key(|img| img.compatibility_score(&self.arch))
            .or_else(|| {
                self.installed_images
                    .iter()
                    .max_by_key(|img| img.compatibility_score(&self.arch))
            })
    }
}

pub fn home_dir() -> PathBuf {
    if let Ok(home) = env::var("HOME") {
        return PathBuf::from(home);
    }
    if let Ok(userprofile) = env::var("USERPROFILE") {
        return PathBuf::from(userprofile);
    }
    PathBuf::from(".")
}

pub fn detect_avd_home() -> PathBuf {
    if let Ok(avd_home) = env::var("ANDROID_AVD_HOME") {
        let p = PathBuf::from(avd_home);
        if p.exists() {
            return p;
        }
    }
    home_dir().join(".android").join("avd")
}

pub fn detect_sdk_root(os: &HostOs) -> Option<PathBuf> {
    for var in ["ANDROID_HOME", "ANDROID_SDK_ROOT"] {
        if let Ok(val) = env::var(var) {
            let p = PathBuf::from(val);
            if p.exists() {
                return Some(p);
            }
        }
    }

    let home = home_dir();
    let mut candidates = Vec::new();
    match os {
        HostOs::Linux => {
            candidates.push(home.join("Android").join("Sdk"));
            candidates.push(home.join("android-sdk"));
            candidates.push(PathBuf::from("/opt/android-sdk"));
            candidates.push(PathBuf::from("/usr/lib/android-sdk"));
        }
        HostOs::MacOs => {
            candidates.push(home.join("Library").join("Android").join("sdk"));
            candidates.push(home.join("Android").join("Sdk"));
        }
        HostOs::Windows => {
            if let Ok(local_app_data) = env::var("LOCALAPPDATA") {
                candidates.push(PathBuf::from(local_app_data).join("Android").join("Sdk"));
            }
            candidates.push(home.join("AppData").join("Local").join("Android").join("Sdk"));
        }
        HostOs::Other(_) => {
            candidates.push(home.join("Android").join("Sdk"));
        }
    }

    candidates.into_iter().find(|p| p.exists())
}

fn detect_hypervisor(os: &HostOs) -> (bool, String) {
    match os {
        HostOs::Linux => {
            let kvm = Path::new("/dev/kvm");
            if kvm.exists() {
                // Verify read/write access by opening
                match fs::OpenOptions::new().read(true).write(true).open(kvm) {
                    Ok(_) => (
                        true,
                        "KVM ativo e acessível em /dev/kvm (aceleração máxima por hardware)".to_string(),
                    ),
                    Err(e) => (
                        false,
                        format!(
                            "/dev/kvm existe, mas sem permissão de acesso ({e}). Execute: sudo usermod -aG kvm $USER"
                        ),
                    ),
                }
            } else {
                (
                    false,
                    "/dev/kvm não encontrado. Ative Intel VT-x / AMD-V na BIOS.".to_string(),
                )
            }
        }
        HostOs::MacOs => {
            if let Ok(out) = Command::new("sysctl").arg("-n").arg("kern.hv_support").output() {
                let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if s == "1" {
                    return (
                        true,
                        "Apple Hypervisor.framework (HVF) ativo e suportado".to_string(),
                    );
                }
            }
            (
                false,
                "Hypervisor.framework não detectado no macOS".to_string(),
            )
        }
        HostOs::Windows => {
            if let Ok(out) = Command::new("cmd")
                .args(["/C", "sc query hvservice"])
                .output()
            {
                let stdout = String::from_utf8_lossy(&out.stdout);
                if stdout.contains("RUNNING") {
                    return (
                        true,
                        "Windows Hypervisor Platform (WHPX / Hyper-V) ativo".to_string(),
                    );
                }
            }
            (
                true,
                "Windows WHPX / AEHD (verifique 'Plataforma do Hipervisor do Windows' nos Recursos do Windows)".to_string(),
            )
        }
        HostOs::Other(_) => (false, "SO genérico (sem detecção específica de hypervisor)".to_string()),
    }
}

fn detect_webcams(os: &HostOs) -> Vec<String> {
    let mut cams = Vec::new();
    if *os == HostOs::Linux {
        if let Ok(entries) = fs::read_dir("/dev") {
            let mut devs: Vec<String> = entries
                .flatten()
                .filter_map(|e| {
                    let name = e.file_name().to_string_lossy().to_string();
                    if name.starts_with("video") {
                        Some(format!("/dev/{name}"))
                    } else {
                        None
                    }
                })
                .collect();
            devs.sort();
            cams.extend(devs);
        }
    }
    if cams.is_empty() {
        cams.push("webcam0 (virtual/host default)".to_string());
    }
    cams
}

pub fn which_bin(name: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(name);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

pub fn parse_prop_file(path: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let Ok(content) = fs::read_to_string(path) else {
        return map;
    };
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            map.insert(k.trim().to_string(), v.trim().to_string());
        }
    }
    map
}

pub fn scan_system_images(sdk_root: &Path) -> Vec<SystemImageInfo> {
    let mut images = Vec::new();
    let sys_images_dir = sdk_root.join("system-images");
    let Ok(api_dirs) = fs::read_dir(&sys_images_dir) else {
        return images;
    };

    for api_entry in api_dirs.flatten() {
        let api_path = api_entry.path();
        if !api_path.is_dir() {
            continue;
        }
        let Ok(tag_dirs) = fs::read_dir(&api_path) else {
            continue;
        };
        for tag_entry in tag_dirs.flatten() {
            let tag_path = tag_entry.path();
            if !tag_path.is_dir() {
                continue;
            }
            let tag_id = tag_entry.file_name().to_string_lossy().to_string();
            let Ok(abi_dirs) = fs::read_dir(&tag_path) else {
                continue;
            };
            for abi_entry in abi_dirs.flatten() {
                let abi_path = abi_entry.path();
                if !abi_path.is_dir() {
                    continue;
                }
                let build_prop_path = abi_path.join("build.prop");
                if !build_prop_path.exists() && !abi_path.join("system.img").exists() {
                    continue;
                }

                let props = parse_prop_file(&build_prop_path);
                let api_level = props
                    .get("ro.build.version.sdk")
                    .or_else(|| props.get("ro.system.build.version.sdk"))
                    .and_then(|s| s.parse::<u32>().ok())
                    .unwrap_or_else(|| {
                        api_entry
                            .file_name()
                            .to_string_lossy()
                            .strip_prefix("android-")
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(0)
                    });

                let android_release = props
                    .get("ro.build.version.release")
                    .or_else(|| props.get("ro.system.build.version.release"))
                    .cloned()
                    .unwrap_or_else(|| "Unknown".to_string());

                let primary_abi = abi_entry.file_name().to_string_lossy().to_string();

                let abilist_raw = props
                    .get("ro.system.product.cpu.abilist")
                    .or_else(|| props.get("ro.product.cpu.abilist"))
                    .cloned()
                    .unwrap_or_else(|| primary_abi.clone());

                let supported_abis: Vec<String> = abilist_raw
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();

                let native_bridge = props
                    .get("ro.dalvik.vm.native.bridge")
                    .filter(|v| *v != "0" && !v.is_empty())
                    .cloned();

                let adv_features = parse_prop_file(&abi_path.join("advancedFeatures.ini"));
                let has_play_store = tag_id.contains("playstore")
                    || adv_features
                        .get("PlayStoreImage")
                        .map(|v| v == "on")
                        .unwrap_or(false);

                let has_arm64_translation = supported_abis.iter().any(|a| a == "arm64-v8a")
                    && (primary_abi == "arm64-v8a" || native_bridge.is_some());
                let has_arm32_translation = supported_abis.iter().any(|a| a.starts_with("armeabi"));

                let api_folder = api_entry.file_name().to_string_lossy().to_string();
                let relative_sysdir =
                    format!("system-images/{api_folder}/{tag_id}/{primary_abi}/");

                images.push(SystemImageInfo {
                    relative_sysdir,
                    full_path: abi_path,
                    api_level,
                    android_release,
                    tag_id: tag_id.clone(),
                    primary_abi,
                    supported_abis,
                    has_play_store,
                    has_arm64_translation,
                    has_arm32_translation,
                    native_bridge,
                });
            }
        }
    }

    images.sort_by(|a, b| {
        b.has_play_store
            .cmp(&a.has_play_store)
            .then(b.api_level.cmp(&a.api_level))
            .then(a.primary_abi.cmp(&b.primary_abi))
    });
    images
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_host_detection() {
        let report = HostReport::detect();
        assert!(!report.arch.is_empty());
        assert!(report.cpu_cores >= 1);
    }

    #[test]
    fn test_image_compatibility_scoring() {
        let x86_play = SystemImageInfo {
            relative_sysdir: "system-images/android-35/google_apis_playstore/x86_64/".into(),
            full_path: PathBuf::new(),
            api_level: 35,
            android_release: "15".into(),
            tag_id: "google_apis_playstore".into(),
            primary_abi: "x86_64".into(),
            supported_abis: vec!["x86_64".into(), "arm64-v8a".into()],
            has_play_store: true,
            has_arm64_translation: true,
            has_arm32_translation: false,
            native_bridge: Some("libndk_translation.so".into()),
        };

        let arm_play = SystemImageInfo {
            primary_abi: "arm64-v8a".into(),
            ..x86_play.clone()
        };

        // On an x86_64 host, x86_64 + libndk_translation should score higher than pure arm64-v8a
        // because x86_64 runs on hardware KVM/WHPX while translating arm64-v8a apps natively.
        assert!(
            x86_play.compatibility_score("x86_64") > arm_play.compatibility_score("x86_64")
        );
        // On an Apple Silicon / ARM64 host, native arm64-v8a should score higher.
        assert!(
            arm_play.compatibility_score("aarch64") > x86_play.compatibility_score("aarch64")
        );
    }
}
