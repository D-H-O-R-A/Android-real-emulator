use crate::platform::HostReport;
use std::io;
use std::process::{Command, Stdio};

#[derive(Debug, Clone)]
pub struct RecommendedImagePackage {
    pub package_id: String,
    pub api_level: u32,
    pub description: String,
}

/// Returns the recommended Google Play Store system image packages for the current host architecture.
/// - API 34 (Android 14) / API 35 (Android 15): Modern 64-bit apps + `libndk_translation` (`arm64-v8a`).
/// - API 30 (Android 11): Full 32-bit (`armeabi-v7a`) + 64-bit (`arm64-v8a`) dual-ABI translation for older 32-bit apps/games.
pub fn recommended_playstore_packages(host: &HostReport) -> Vec<RecommendedImagePackage> {
    let target_abi = if host.arch == "aarch64" {
        "arm64-v8a"
    } else {
        "x86_64"
    };

    vec![
        RecommendedImagePackage {
            package_id: format!("system-images;android-34;google_apis_playstore;{target_abi}"),
            api_level: 34,
            description: format!(
                "Android 14 (API 34) Google Play Official + Tradução ARM64 ({target_abi})"
            ),
        },
        RecommendedImagePackage {
            package_id: format!("system-images;android-35;google_apis_playstore;{target_abi}"),
            api_level: 35,
            description: format!(
                "Android 15 (API 35) Google Play Official + Tradução ARM64 ({target_abi})"
            ),
        },
        RecommendedImagePackage {
            package_id: format!("system-images;android-33;google_apis_playstore;{target_abi}"),
            api_level: 33,
            description: format!(
                "Android 13 (API 33) Google Play Official + Tradução ARM64 ({target_abi})"
            ),
        },
        RecommendedImagePackage {
            package_id: format!("system-images;android-30;google_apis_playstore;{target_abi}"),
            api_level: 30,
            description: format!(
                "Android 11 (API 30) Google Play Official + Suporte Híbrido 32-bit (armeabi-v7a) e 64-bit (arm64-v8a)"
            ),
        },
    ]
}

/// Installs an official Google Play Store system image using `sdkmanager`.
pub fn install_system_image(host: &HostReport, api_level: u32) -> io::Result<()> {
    let sdkmanager = host.sdkmanager_bin.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "sdkmanager não encontrado no Android SDK.",
        )
    })?;

    let target_abi = if host.arch == "aarch64" {
        "arm64-v8a"
    } else {
        "x86_64"
    };
    let pkg = format!("system-images;android-{api_level};google_apis_playstore;{target_abi}");

    println!("Instalando pacote oficial Google Play: {pkg} ...");
    let status = Command::new(sdkmanager)
        .arg(&pkg)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Falha ao instalar {pkg} via sdkmanager."),
        ));
    }
    Ok(())
}
