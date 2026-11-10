use crate::platform::HostReport;
use std::io;
use std::path::Path;
use std::process::Command;

#[derive(Debug, Clone)]
pub struct FeatureCheck {
    pub feature: String,
    pub present: bool,
    pub description: String,
}

#[derive(Debug, Clone)]
pub struct LiveAuditReport {
    pub device_serial: String,
    pub android_version: String,
    pub sdk_level: String,
    pub device_model: String,
    pub cpu_abilist: String,
    pub native_bridge: String,
    pub native_bridge_exec: String,
    pub lcd_density: String,
    pub gl_es_version: String,
    pub play_store_installed: bool,
    pub play_services_installed: bool,
    pub arm64_supported: bool,
    pub arm32_supported: bool,
    pub sim_operator: String,
    pub sim_country: String,
    pub sim_state: String,
    pub network_operator: String,
    pub feature_checks: Vec<FeatureCheck>,
}

const REQUIRED_PLAYSTORE_FEATURES: &[(&str, &str)] = &[
    (
        "android.hardware.camera",
        "Câmera traseira principal (exigida por bancos, QR code, WhatsApp, Instagram)",
    ),
    (
        "android.hardware.camera.front",
        "Câmera frontal (exigida por selfies, videochamadas e biometria facial)",
    ),
    (
        "android.hardware.camera.any",
        "Presença de qualquer câmera no sistema",
    ),
    (
        "android.hardware.wifi",
        "Conectividade Wi-Fi (VirtioWifi - evita bloqueio por falta de rede sem fio)",
    ),
    (
        "android.hardware.bluetooth",
        "Suporte a Bluetooth emulado",
    ),
    (
        "android.hardware.location.gps",
        "Localização GPS por hardware",
    ),
    (
        "android.hardware.touchscreen",
        "Tela sensível ao toque",
    ),
    (
        "android.hardware.touchscreen.multitouch.jazzhand",
        "Multi-touch completo (5+ toques simultâneos para jogos)",
    ),
    (
        "android.hardware.faketouch",
        "Compatibilidade básica de toque",
    ),
    (
        "android.hardware.telephony",
        "Subsistema de telefonia/modem GSM",
    ),
    (
        "android.hardware.microphone",
        "Entrada de áudio / microfone",
    ),
    (
        "android.hardware.sensor.accelerometer",
        "Sensor acelerômetro",
    ),
    (
        "android.hardware.sensor.gyroscope",
        "Sensor giroscópio",
    ),
    (
        "android.hardware.sensor.compass",
        "Bússola / magnetômetro",
    ),
    (
        "android.hardware.vulkan.level",
        "Suporte gráfico Vulkan",
    ),
    (
        "android.hardware.vulkan.version",
        "Versão da API Vulkan",
    ),
];

fn run_adb(adb_bin: &Path, serial: Option<&str>, args: &[&str]) -> io::Result<String> {
    let mut cmd = Command::new(adb_bin);
    if let Some(s) = serial {
        cmd.arg("-s").arg(s);
    }
    cmd.args(args);
    let out = cmd.output()?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("ADB falhou: {}", stderr.trim()),
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub fn list_connected_devices(host: &HostReport) -> io::Result<Vec<String>> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::NotFound,
            "Binário 'adb' não encontrado. Verifique a instalação do Android SDK platform-tools.",
        )
    })?;
    let out = run_adb(adb, None, &["devices"])?;
    let mut devices = Vec::new();
    for line in out.lines().skip(1) {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[1] == "device" {
            devices.push(parts[0].to_string());
        }
    }
    Ok(devices)
}

/// Runs a live compatibility audit against a booted Android instance via ADB.
pub fn audit_running_instance(
    host: &HostReport,
    serial: Option<&str>,
) -> io::Result<LiveAuditReport> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Binário 'adb' não encontrado.")
    })?;

    let devices = list_connected_devices(host)?;
    let target_serial = match serial {
        Some(s) => s.to_string(),
        None => devices.into_iter().next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Nenhuma instância Android em execução detectada via ADB. Inicie com `droidrun start`.",
            )
        })?,
    };
    let s = Some(target_serial.as_str());

    let android_version =
        run_adb(adb, s, &["shell", "getprop", "ro.build.version.release"]).unwrap_or_default();
    let sdk_level =
        run_adb(adb, s, &["shell", "getprop", "ro.build.version.sdk"]).unwrap_or_default();
    let device_model =
        run_adb(adb, s, &["shell", "getprop", "ro.product.model"]).unwrap_or_default();
    let cpu_abilist =
        run_adb(adb, s, &["shell", "getprop", "ro.product.cpu.abilist"]).unwrap_or_default();
    let native_bridge =
        run_adb(adb, s, &["shell", "getprop", "ro.dalvik.vm.native.bridge"]).unwrap_or_default();
    let native_bridge_exec =
        run_adb(adb, s, &["shell", "getprop", "ro.enable.native.bridge.exec"]).unwrap_or_default();
    let mut lcd_density =
        run_adb(adb, s, &["shell", "getprop", "ro.sf.lcd_density"]).unwrap_or_default();
    if lcd_density.is_empty() {
        lcd_density = run_adb(adb, s, &["shell", "wm", "density"])
            .unwrap_or_default()
            .replace("Physical density:", "")
            .trim()
            .to_string();
    }
    let gl_es_version =
        run_adb(adb, s, &["shell", "getprop", "ro.opengles.version"]).unwrap_or_default();

    let packages_out = run_adb(adb, s, &["shell", "pm", "list", "packages"]).unwrap_or_default();
    let play_store_installed = packages_out.lines().any(|l| l.trim() == "package:com.android.vending");
    let play_services_installed = packages_out
        .lines()
        .any(|l| l.trim() == "package:com.google.android.gms");

    let features_out = run_adb(adb, s, &["shell", "pm", "list", "features"]).unwrap_or_default();
    let mut feature_checks = Vec::new();
    for (feat, desc) in REQUIRED_PLAYSTORE_FEATURES {
        let present = features_out.lines().any(|line| {
            let trimmed = line.trim().strip_prefix("feature:").unwrap_or(line.trim());
            trimmed == *feat || trimmed.starts_with(&format!("{feat}="))
        });
        feature_checks.push(FeatureCheck {
            feature: (*feat).to_string(),
            present,
            description: (*desc).to_string(),
        });
    }

    let arm64_supported = cpu_abilist.contains("arm64-v8a");
    let arm32_supported = cpu_abilist.contains("armeabi");

    let sim_operator =
        run_adb(adb, s, &["shell", "getprop", "gsm.sim.operator.numeric"]).unwrap_or_default();
    let sim_country =
        run_adb(adb, s, &["shell", "getprop", "gsm.sim.operator.iso-country"]).unwrap_or_default();
    let sim_state =
        run_adb(adb, s, &["shell", "getprop", "gsm.sim.state"]).unwrap_or_default();
    let network_operator =
        run_adb(adb, s, &["shell", "getprop", "gsm.operator.alpha"]).unwrap_or_default();

    Ok(LiveAuditReport {
        device_serial: target_serial,
        android_version,
        sdk_level,
        device_model,
        cpu_abilist,
        native_bridge,
        native_bridge_exec,
        lcd_density,
        gl_es_version,
        play_store_installed,
        play_services_installed,
        arm64_supported,
        arm32_supported,
        sim_operator,
        sim_country,
        sim_state,
        network_operator,
        feature_checks,
    })
}

/// Resets cached `DeviceConfig` in Google Play Store (`com.android.vending`) on a live device
/// so that newly enabled hardware features (like rear camera, Vulkan, sensors, ABIs) are
/// immediately re-uploaded to Google Play servers without losing the Google account login.
pub fn sync_playstore_catalog(
    host: &HostReport,
    serial: Option<&str>,
) -> io::Result<Vec<String>> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Binário 'adb' não encontrado.")
    })?;
    let devices = list_connected_devices(host)?;
    let target_serial = match serial {
        Some(s) => s.to_string(),
        None => devices.into_iter().next().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                "Nenhuma instância Android conectada via ADB.",
            )
        })?,
    };
    let s = Some(target_serial.as_str());
    let mut steps = Vec::new();

    // 1. Ensure Google Play Store package is enabled
    let _ = run_adb(
        adb,
        s,
        &["shell", "pm", "enable", "com.android.vending"],
    );
    steps.push("Pacote com.android.vending (Google Play Store) habilitado.".to_string());

    // 2. Force-stop and clear local Play Store cache/data so it re-queries PackageManager features
    let _ = run_adb(
        adb,
        s,
        &["shell", "am", "force-stop", "com.android.vending"],
    );
    run_adb(adb, s, &["shell", "pm", "clear", "com.android.vending"])?;
    steps.push(
        "Cache de DeviceConfig da Google Play Store limpo (força reenvio das features de câmera, GPU e ARM64)."
            .to_string(),
    );

    // 3. Re-launch Google Play Store UI
    let _ = run_adb(
        adb,
        s,
        &[
            "shell",
            "monkey",
            "-p",
            "com.android.vending",
            "-c",
            "android.intent.category.LAUNCHER",
            "1",
        ],
    );
    steps.push("Google Play Store reiniciada na tela do Android.".to_string());

    Ok(steps)
}

/// Installs a single `.apk` or a directory of split `.apk` files onto the running Android instance.
pub fn install_apk(
    host: &HostReport,
    serial: Option<&str>,
    target_path: &Path,
) -> io::Result<String> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Binário 'adb' não encontrado.")
    })?;
    if !target_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Arquivo ou pasta não encontrado: {}", target_path.display()),
        ));
    }

    if target_path.is_dir() {
        let mut apk_files = Vec::new();
        for entry in std::fs::read_dir(target_path)?.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("apk") {
                apk_files.push(p);
            }
        }
        if apk_files.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "Nenhum arquivo .apk encontrado no diretório informado.",
            ));
        }
        let mut cmd = Command::new(adb);
        if let Some(s) = serial {
            cmd.arg("-s").arg(s);
        }
        cmd.arg("install-multiple").arg("-r");
        for apk in &apk_files {
            cmd.arg(apk);
        }
        let out = cmd.output()?;
        if !out.status.success() {
            return Err(io::Error::new(
                io::ErrorKind::Other,
                String::from_utf8_lossy(&out.stderr).to_string(),
            ));
        }
        Ok(format!(
            "Split APK instalado com sucesso ({} arquivos): {}",
            apk_files.len(),
            String::from_utf8_lossy(&out.stdout).trim()
        ))
    } else {
        let path_str = target_path.to_string_lossy().to_string();
        run_adb(adb, serial, &["install", "-r", &path_str])
    }
}

/// Sends an SMS directly to the running Android instance via ADB emulator control pipe.
/// Useful for testing 2FA, OTP verification, and receiving verification codes.
pub fn send_sms(
    host: &HostReport,
    serial: Option<&str>,
    sender_number: &str,
    message_text: &str,
) -> io::Result<String> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Binário 'adb' não encontrado.")
    })?;
    run_adb(adb, serial, &["emu", "sms", "send", sender_number, message_text])
}
