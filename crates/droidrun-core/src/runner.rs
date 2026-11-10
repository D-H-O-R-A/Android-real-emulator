use crate::platform::{HostOs, HostReport};
use crate::profile::{ensure_user_advanced_features, AvdProfile};
use std::io;
use std::path::Path;
use std::process::{Child, Command, Stdio};

#[derive(Debug, Clone)]
pub struct LaunchOptions {
    pub profile_name: String,
    pub cold_boot: bool,
    pub wipe_data: bool,
    pub gpu_mode: String,
    pub camera_back: String,
    pub camera_front: String,
    pub memory_mb: Option<u32>,
    pub cores: Option<u32>,
    pub detach: bool,
}

impl Default for LaunchOptions {
    fn default() -> Self {
        Self {
            profile_name: "Pixel_7_PlayStore".to_string(),
            cold_boot: true,
            wipe_data: false,
            gpu_mode: "host".to_string(),
            camera_back: "webcam0".to_string(),
            camera_front: "webcam0".to_string(),
            memory_mb: None,
            cores: None,
            detach: true,
        }
    }
}

/// Builds the full command-line arguments for launching the Android instance with Play Store,
/// Vulkan/GLES 3.1+, VirtioWifi, and dual cameras enabled.
pub fn build_emulator_args(opts: &LaunchOptions, profile: Option<&AvdProfile>) -> Vec<String> {
    let mut args = vec![
        format!("@{}", opts.profile_name),
        "-gpu".to_string(),
        opts.gpu_mode.clone(),
        "-feature".to_string(),
        "PlayStoreImage,GLESDynamicVersion,VirtioWifi,BluetoothEmulation".to_string(),
    ];

    let cam_back = profile
        .and_then(|p| p.properties.get("hw.camera.back"))
        .filter(|v| *v != "none" && !v.is_empty())
        .cloned()
        .unwrap_or_else(|| opts.camera_back.clone());

    let cam_front = profile
        .and_then(|p| p.properties.get("hw.camera.front"))
        .filter(|v| *v != "none" && !v.is_empty())
        .cloned()
        .unwrap_or_else(|| opts.camera_front.clone());

    args.push("-camera-back".to_string());
    args.push(cam_back);
    args.push("-camera-front".to_string());
    args.push(cam_front);

    if let Some(mem) = opts.memory_mb {
        args.push("-memory".to_string());
        args.push(mem.to_string());
    }
    if let Some(cores) = opts.cores {
        args.push("-cores".to_string());
        args.push(cores.to_string());
    }
    if opts.cold_boot {
        args.push("-no-snapshot-load".to_string());
    }
    if opts.wipe_data {
        args.push("-wipe-data".to_string());
    }

    args
}

/// Launches the configured Android profile using the SDK emulator engine with KVM/HVF/WHPX acceleration.
pub fn launch_profile(
    host: &HostReport,
    opts: &LaunchOptions,
    profile: Option<&AvdProfile>,
) -> io::Result<Child> {
    let emulator_bin = host.emulator_bin.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Binário 'emulator' não encontrado no Android SDK. Execute `droidrun setup-sdk`.",
        )
    })?;

    ensure_user_advanced_features()?;
    let args = build_emulator_args(opts, profile);

    let mut cmd = Command::new(emulator_bin);
    cmd.args(&args);

    if let Some(sdk_root) = &host.sdk_root {
        cmd.env("ANDROID_SDK_ROOT", sdk_root);
        cmd.env("ANDROID_HOME", sdk_root);
    }
    cmd.env("ANDROID_AVD_HOME", &host.avd_home);

    // On Linux laptops with hybrid Intel + NVIDIA GPUs (Optimus), Vulkan defaults to the discrete
    // NVIDIA GPU while X11 OpenGL defaults to Mesa Intel, causing `glInteropSupported: false` and crashes.
    // Enabling NVIDIA PRIME Render Offload aligns both OpenGL and Vulkan on the NVIDIA GPU (`glInteropSupported: true`).
    if host.os == HostOs::Linux && Path::new("/proc/driver/nvidia/version").exists() {
        cmd.env("__NV_PRIME_RENDER_OFFLOAD", "1");
        cmd.env("__GLX_VENDOR_LIBRARY_NAME", "nvidia");
    }

    if opts.detach {
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
    }

    cmd.spawn()
}

/// Builds a native cross-platform QEMU command line (KVM on Linux, HVF on macOS, WHPX on Windows)
/// for booting standalone Android AOSP / GSI / ISO images directly.
pub fn build_standalone_qemu_args(
    host: &HostReport,
    disk_or_iso: &Path,
    ram_mb: u32,
    cores: u32,
    adb_port: u16,
) -> Vec<String> {
    let accel = match host.os {
        HostOs::Linux => "kvm",
        HostOs::MacOs => "hvf",
        HostOs::Windows => "whpx",
        HostOs::Other(_) => "tcg",
    };

    let mut args = vec![
        "-accel".to_string(),
        accel.to_string(),
        "-cpu".to_string(),
        "host".to_string(),
        "-smp".to_string(),
        cores.to_string(),
        "-m".to_string(),
        ram_mb.to_string(),
        "-device".to_string(),
        "virtio-gpu-gl-pci".to_string(),
        "-display".to_string(),
        "gtk,gl=on".to_string(),
        "-device".to_string(),
        "virtio-mouse-pci".to_string(),
        "-device".to_string(),
        "virtio-keyboard-pci".to_string(),
        "-netdev".to_string(),
        format!("user,id=net0,hostfwd=tcp::{adb_port}-:5555"),
        "-device".to_string(),
        "virtio-net-pci,netdev=net0".to_string(),
    ];

    let is_iso = disk_or_iso
        .extension()
        .and_then(|e| e.to_str())
        .map(|s| s.eq_ignore_ascii_case("iso"))
        .unwrap_or(false);

    if is_iso {
        args.push("-cdrom".to_string());
        args.push(disk_or_iso.to_string_lossy().to_string());
    } else {
        args.push("-drive".to_string());
        args.push(format!(
            "file={},if=virtio,format=raw",
            disk_or_iso.display()
        ));
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_build_emulator_args_includes_playstore_and_cameras() {
        let opts = LaunchOptions::default();
        let args = build_emulator_args(&opts, None);
        assert!(args.contains(&"@Pixel_7_PlayStore".to_string()));
        assert!(args.contains(&"host".to_string()));
        assert!(args.contains(&"-camera-back".to_string()));
        assert!(args.contains(&"webcam0".to_string()));
        assert!(args.contains(&"-no-snapshot-load".to_string()));
    }
}
