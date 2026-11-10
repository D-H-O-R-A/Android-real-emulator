use crate::platform::{parse_prop_file, home_dir, HostReport, SystemImageInfo};
use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone)]
pub struct ProfileIssue {
    pub key: String,
    pub current_value: String,
    pub recommended_value: String,
    pub reason: String,
}

#[derive(Debug, Clone)]
pub struct AvdProfile {
    pub name: String,
    pub ini_file: PathBuf,
    pub avd_dir: PathBuf,
    pub config_path: PathBuf,
    pub properties: BTreeMap<String, String>,
    pub issues: Vec<ProfileIssue>,
}

impl AvdProfile {
    pub fn is_healthy_for_playstore(&self) -> bool {
        self.issues.is_empty()
    }
}

pub struct ProfileConfigOptions {
    pub name: String,
    pub ram_mb: u32,
    pub cpu_cores: u32,
    pub data_partition_gb: u32,
    pub lcd_width: u32,
    pub lcd_height: u32,
    pub lcd_density: u32,
    pub camera_back: String,
    pub camera_front: String,
}

impl Default for ProfileConfigOptions {
    fn default() -> Self {
        Self {
            name: "DroidRun_PlayStore".to_string(),
            ram_mb: 6144,
            cpu_cores: 6,
            data_partition_gb: 16,
            lcd_width: 1080,
            lcd_height: 2400,
            lcd_density: 420,
            camera_back: "webcam0".to_string(),
            camera_front: "webcam0".to_string(),
        }
    }
}

/// Scans `~/.android/avd` for all existing Android profiles and diagnoses compatibility issues.
pub fn list_profiles(host: &HostReport) -> Vec<AvdProfile> {
    let mut profiles = Vec::new();
    let Ok(entries) = fs::read_dir(&host.avd_home) else {
        return profiles;
    };

    let best_img = host.best_playstore_image();

    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("avd") || !path.is_dir() {
            continue;
        }
        let Some(stem) = path.file_stem().and_then(|s| s.to_str()) else {
            continue;
        };
        let name = stem.to_string();
        let ini_file = host.avd_home.join(format!("{name}.ini"));
        let config_path = path.join("config.ini");
        if !config_path.exists() {
            continue;
        }
        let properties = parse_prop_file(&config_path);
        let issues = diagnose_properties(&properties, host, best_img);

        profiles.push(AvdProfile {
            name,
            ini_file,
            avd_dir: path,
            config_path,
            properties,
            issues,
        });
    }

    profiles.sort_by(|a, b| a.name.cmp(&b.name));
    profiles
}

pub fn diagnose_properties(
    props: &BTreeMap<String, String>,
    host: &HostReport,
    best_img: Option<&SystemImageInfo>,
) -> Vec<ProfileIssue> {
    let mut issues = Vec::new();

    // 1. Check PlayStore.enabled
    let playstore_enabled = props
        .get("PlayStore.enabled")
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if playstore_enabled != "yes" && playstore_enabled != "true" {
        issues.push(ProfileIssue {
            key: "PlayStore.enabled".to_string(),
            current_value: props
                .get("PlayStore.enabled")
                .cloned()
                .unwrap_or_else(|| "<ausente>".to_string()),
            recommended_value: "yes".to_string(),
            reason: "Com PlayStore.enabled=no, o Google Play Services desativa o perfil comercial da Play Store e restringe o catálogo.".to_string(),
        });
    }

    // 2. Check tag.id
    let tag_id = props.get("tag.id").cloned().unwrap_or_default();
    if !tag_id.contains("playstore") {
        issues.push(ProfileIssue {
            key: "tag.id".to_string(),
            current_value: if tag_id.is_empty() {
                "<ausente>".to_string()
            } else {
                tag_id
            },
            recommended_value: "google_apis_playstore".to_string(),
            reason: "Imagem sem tag google_apis_playstore não inclui o aplicativo oficial da Google Play Store (Phonesky/Vending).".to_string(),
        });
    }

    // 3. Check Rear Camera (hw.camera.back)
    let cam_back = props
        .get("hw.camera.back")
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if cam_back != "webcam0" {
        issues.push(ProfileIssue {
            key: "hw.camera.back".to_string(),
            current_value: if cam_back.is_empty() {
                "<ausente>".to_string()
            } else {
                cam_back
            },
            recommended_value: "webcam0".to_string(),
            reason: "Definir hw.camera.back=webcam0 conecta a câmera traseira do Android diretamente à webcam física do notebook (essencial para ler QR Code Pix, boletos, documentos e usar apps de banco).".to_string(),
        });
    }

    // 4. Check Front Camera (hw.camera.front)
    let cam_front = props
        .get("hw.camera.front")
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if cam_front.is_empty() || cam_front == "none" || cam_front == "webcam1" {
        issues.push(ProfileIssue {
            key: "hw.camera.front".to_string(),
            current_value: if cam_front.is_empty() {
                "<ausente>".to_string()
            } else {
                cam_front
            },
            recommended_value: "webcam0".to_string(),
            reason: "Câmera frontal ausente ou apontando para nó de metadados inválido (webcam1) remove 'android.hardware.camera.front'.".to_string(),
        });
    }

    // 5. Check CPU Architecture vs Host Hypervisor (e.g. arm64-v8a image on x86_64 host)
    let current_abi = props.get("abi.type").cloned().unwrap_or_default();
    if host.arch == "x86_64" && current_abi == "arm64-v8a" {
        if let Some(img) = best_img {
            if img.primary_abi == "x86_64" && img.has_arm64_translation {
                issues.push(ProfileIssue {
                    key: "abi.type / image.sysdir.1".to_string(),
                    current_value: format!("arm64-v8a ({})", props.get("image.sysdir.1").cloned().unwrap_or_default()),
                    recommended_value: format!("x86_64 + libndk_translation ({})", img.relative_sysdir),
                    reason: "Em PCs x86_64, imagens puras arm64-v8a não podem usar KVM/WHPX e rodam extremamente lentas. A imagem x86_64 com libndk_translation.so usa aceleração nativa KVM e traduz apps arm64-v8a em tempo real.".to_string(),
                });
            }
        }
    }

    // 6. Check GPU Acceleration mode
    let gpu_mode = props.get("hw.gpu.mode").cloned().unwrap_or_default();
    if gpu_mode != "host" {
        issues.push(ProfileIssue {
            key: "hw.gpu.mode".to_string(),
            current_value: if gpu_mode.is_empty() {
                "<ausente>".to_string()
            } else {
                gpu_mode
            },
            recommended_value: "host".to_string(),
            reason: "hw.gpu.mode=host habilita Vulkan 1.1+ e OpenGL ES 3.1+ direto na GPU física, exigido por jogos e apps modernos.".to_string(),
        });
    }

    // 7. Check mandatory sensors & hardware flags
    for (key, desc) in [
        ("hw.accelerometer", "Acelerômetro exigido por jogos e rotação"),
        ("hw.gyroscope", "Giroscópio exigido por apps 3D/AR e jogos"),
        ("hw.gps", "GPS exigido por apps de mapas, transporte, entregas e bancos"),
        ("hw.gsmModem", "Modem/Telefonia exigido por apps que verificam suporte celular (SMS/voz)"),
        ("hw.battery", "Simulação de bateria exigida por serviços de background"),
        ("hw.sdCard", "Armazenamento externo exigido por apps de mídia"),
        ("hw.sensors.magnetic_field", "Bússola/Magnetômetro exigido por navegação"),
        ("hw.sensors.proximity", "Sensor de proximidade"),
        ("hw.sensors.light", "Sensor de luminosidade"),
    ] {
        let val = props.get(key).map(|v| v.to_lowercase()).unwrap_or_default();
        if val != "yes" && val != "true" {
            issues.push(ProfileIssue {
                key: key.to_string(),
                current_value: if val.is_empty() {
                    "<ausente>".to_string()
                } else {
                    val
                },
                recommended_value: "yes".to_string(),
                reason: desc.to_string(),
            });
        }
    }

    issues
}

/// Applies all Play Store, ARM64 translation, camera, sensor, and GPU fixes to an existing AVD profile.
pub fn fix_profile(profile: &AvdProfile, host: &HostReport) -> io::Result<Vec<String>> {
    let mut changes = Vec::new();
    let mut props = profile.properties.clone();

    // Backup original config.ini
    let backup_path = profile.avd_dir.join("config.ini.bak");
    let _ = fs::copy(&profile.config_path, &backup_path);

    let best_img = host.best_playstore_image();

    // Ensure Play Store is enabled
    if props.get("PlayStore.enabled").map(|s| s.as_str()) != Some("yes") {
        props.insert("PlayStore.enabled".to_string(), "yes".to_string());
        changes.push("PlayStore.enabled = yes".to_string());
    }

    // Ensure Google Play tag and Play Store system image with ARM64 translation
    if let Some(img) = best_img {
        let current_sysdir = props.get("image.sysdir.1").cloned().unwrap_or_default();
        let needs_image_switch = !current_sysdir.contains("playstore")
            || (host.arch == "x86_64" && props.get("abi.type").map(|s| s.as_str()) == Some("arm64-v8a") && img.primary_abi == "x86_64");

        if needs_image_switch {
            props.insert("image.sysdir.1".to_string(), img.relative_sysdir.clone());
            props.insert("abi.type".to_string(), img.primary_abi.clone());
            props.insert("hw.cpu.arch".to_string(), img.primary_abi.clone());
            props.insert("target".to_string(), format!("android-{}", img.api_level));
            changes.push(format!(
                "image.sysdir.1 = {} (Google Play + ABI {:?})",
                img.relative_sysdir, img.supported_abis
            ));
        }
    }

    props.insert("tag.id".to_string(), "google_apis_playstore".to_string());
    props.insert("tag.ids".to_string(), "google_apis_playstore".to_string());
    props.insert("tag.display".to_string(), "Google Play".to_string());
    props.insert("tag.displaynames".to_string(), "Google Play".to_string());

    // Fix cameras: connect BOTH rear and front cameras to physical webcam0 so QR codes,
    // banking document capture, and facial biometrics all use the real laptop webcam.
    let cam_back = props
        .get("hw.camera.back")
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if cam_back != "webcam0" {
        props.insert("hw.camera.back".to_string(), "webcam0".to_string());
        changes.push("hw.camera.back = webcam0 (câmera traseira conectada à webcam real do PC para QR Code/Pix/Bancos)".to_string());
    }

    let cam_front = props
        .get("hw.camera.front")
        .map(|s| s.to_lowercase())
        .unwrap_or_default();
    if cam_front.is_empty() || cam_front == "none" || cam_front == "webcam1" {
        props.insert("hw.camera.front".to_string(), "webcam0".to_string());
        changes.push("hw.camera.front = webcam0 (habilita android.hardware.camera.front na webcam real)".to_string());
    }

    // Fix GPU mode to host
    if props.get("hw.gpu.mode").map(|s| s.as_str()) != Some("host") {
        props.insert("hw.gpu.enabled".to_string(), "yes".to_string());
        props.insert("hw.gpu.mode".to_string(), "host".to_string());
        changes.push("hw.gpu.mode = host (Vulkan + OpenGL ES 3.1+ diretos na GPU)".to_string());
    }

    // Ensure all sensors and hardware capabilities are active
    for key in [
        "hw.accelerometer",
        "hw.accelerometer_uncalibrated",
        "hw.gyroscope",
        "hw.sensors.gyroscope_uncalibrated",
        "hw.gps",
        "hw.gsmModem",
        "hw.battery",
        "hw.audioInput",
        "hw.audioOutput",
        "hw.sdCard",
        "hw.keyboard",
        "hw.lcd.backlight",
        "hw.sensors.light",
        "hw.sensors.pressure",
        "hw.sensors.humidity",
        "hw.sensors.proximity",
        "hw.sensors.magnetic_field",
        "hw.sensors.magnetic_field_uncalibrated",
        "hw.sensors.orientation",
        "hw.sensors.temperature",
        "hw.useext4",
    ] {
        if props.get(key).map(|s| s.as_str()) != Some("yes") {
            props.insert(key.to_string(), "yes".to_string());
            changes.push(format!("{key} = yes"));
        }
    }

    props.insert("hw.screen".to_string(), "multi-touch".to_string());

    // Optimize CPU & RAM if under-provisioned
    let recommended_cores = (host.cpu_cores / 2).clamp(4, 8);
    let current_cores = props
        .get("hw.cpu.ncore")
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(2);
    if current_cores < 4 {
        props.insert("hw.cpu.ncore".to_string(), recommended_cores.to_string());
        changes.push(format!("hw.cpu.ncore = {recommended_cores}"));
    }

    let current_ram = props
        .get("hw.ramSize")
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(2048);
    if current_ram < 4096 {
        props.insert("hw.ramSize".to_string(), "6144".to_string());
        changes.push("hw.ramSize = 6144".to_string());
    }

    // Force cold boot so Android PackageManager re-registers all hardware features cleanly
    props.insert("fastboot.forceColdBoot".to_string(), "yes".to_string());
    props.insert("fastboot.forceFastBoot".to_string(), "no".to_string());

    write_prop_file(&profile.config_path, &props)?;

    // Also remove stale hardware-qemu.ini PlayStore.enabled = false if present
    let hw_qemu_path = profile.avd_dir.join("hardware-qemu.ini");
    if hw_qemu_path.exists() {
        let mut hw_props = parse_prop_file(&hw_qemu_path);
        hw_props.insert("PlayStore.enabled".to_string(), "true".to_string());
        hw_props.insert("hw.gpu.mode".to_string(), "host".to_string());
        let _ = write_prop_file(&hw_qemu_path, &hw_props);
    }

    ensure_user_advanced_features()?;
    changes.push("advancedFeatures.ini configurado (PlayStoreImage, Vulkan, GLESDynamicVersion, VirtioWifi)".to_string());

    // Configure modem simulator for Brazil carrier (Claro 72405 / Vivo 72406 / TIM 72402)
    if let Err(e) = configure_telephony_modem(&profile.avd_dir, host, "72405") {
        eprintln!("[AVISO] Falha ao configurar modem_simulator: {e}");
    } else {
        changes.push("modem_simulator = Claro BR (72405) + EF_AD MNC 2 dígitos (evita bloqueio de operadora e país)".to_string());
    }

    Ok(changes)
}

/// Creates a brand-new, Play Store & ARM64 ready Android profile from scratch.
pub fn create_profile(
    host: &HostReport,
    opts: &ProfileConfigOptions,
    image: &SystemImageInfo,
) -> io::Result<AvdProfile> {
    fs::create_dir_all(&host.avd_home)?;

    let avd_dir = host.avd_home.join(format!("{}.avd", opts.name));
    fs::create_dir_all(&avd_dir)?;

    let ini_file = host.avd_home.join(format!("{}.ini", opts.name));
    let ini_content = format!(
        "avd.ini.encoding=UTF-8\npath={}\npath.rel=avd/{}.avd\ntarget=android-{}\n",
        avd_dir.display(),
        opts.name,
        image.api_level
    );
    fs::write(&ini_file, ini_content)?;

    let mut props = BTreeMap::new();
    props.insert("PlayStore.enabled".to_string(), "yes".to_string());
    props.insert("abi.type".to_string(), image.primary_abi.clone());
    props.insert("avd.id".to_string(), opts.name.clone());
    props.insert("avd.ini.encoding".to_string(), "UTF-8".to_string());
    props.insert("avd.name".to_string(), opts.name.clone());
    props.insert("disk.cachePartition".to_string(), "yes".to_string());
    props.insert("disk.cachePartition.size".to_string(), "66MB".to_string());
    props.insert(
        "disk.dataPartition.size".to_string(),
        format!("{}G", opts.data_partition_gb),
    );
    props.insert("fastboot.forceChosenSnapshotBoot".to_string(), "no".to_string());
    props.insert("fastboot.forceColdBoot".to_string(), "yes".to_string());
    props.insert("fastboot.forceFastBoot".to_string(), "no".to_string());
    props.insert("hw.accelerometer".to_string(), "yes".to_string());
    props.insert("hw.accelerometer_uncalibrated".to_string(), "yes".to_string());
    props.insert("hw.arc".to_string(), "false".to_string());
    props.insert("hw.audioInput".to_string(), "yes".to_string());
    props.insert("hw.audioOutput".to_string(), "yes".to_string());
    props.insert("hw.battery".to_string(), "yes".to_string());
    props.insert("hw.camera.back".to_string(), opts.camera_back.clone());
    props.insert("hw.camera.back.orientation".to_string(), "90".to_string());
    props.insert("hw.camera.front".to_string(), opts.camera_front.clone());
    props.insert("hw.camera.front.orientation".to_string(), "90".to_string());
    props.insert("hw.cpu.arch".to_string(), image.primary_abi.clone());
    props.insert("hw.cpu.ncore".to_string(), opts.cpu_cores.to_string());
    props.insert("hw.dPad".to_string(), "no".to_string());
    props.insert("hw.device.manufacturer".to_string(), "Google".to_string());
    props.insert("hw.device.name".to_string(), "pixel_7_pro".to_string());
    props.insert("hw.gltransport".to_string(), "pipe".to_string());
    props.insert("hw.gps".to_string(), "yes".to_string());
    props.insert("hw.gpu.enabled".to_string(), "yes".to_string());
    props.insert("hw.gpu.mode".to_string(), "host".to_string());
    props.insert("hw.gsmModem".to_string(), "yes".to_string());
    props.insert("hw.gyroscope".to_string(), "yes".to_string());
    props.insert("hw.initialOrientation".to_string(), "portrait".to_string());
    props.insert("hw.keyboard".to_string(), "yes".to_string());
    props.insert("hw.keyboard.charmap".to_string(), "qwerty2".to_string());
    props.insert("hw.keyboard.lid".to_string(), "yes".to_string());
    props.insert("hw.lcd.backlight".to_string(), "yes".to_string());
    props.insert("hw.lcd.circular".to_string(), "false".to_string());
    props.insert("hw.lcd.density".to_string(), opts.lcd_density.to_string());
    props.insert("hw.lcd.depth".to_string(), "32".to_string());
    props.insert("hw.lcd.height".to_string(), opts.lcd_height.to_string());
    props.insert("hw.lcd.width".to_string(), opts.lcd_width.to_string());
    props.insert("hw.lcd.vsync".to_string(), "60".to_string());
    props.insert("hw.mainKeys".to_string(), "no".to_string());
    props.insert("hw.ramSize".to_string(), opts.ram_mb.to_string());
    props.insert("hw.rotaryInput".to_string(), "no".to_string());
    props.insert("hw.screen".to_string(), "multi-touch".to_string());
    props.insert("hw.sdCard".to_string(), "yes".to_string());
    props.insert("hw.sensors.gyroscope_uncalibrated".to_string(), "yes".to_string());
    props.insert("hw.sensors.humidity".to_string(), "yes".to_string());
    props.insert("hw.sensors.light".to_string(), "yes".to_string());
    props.insert("hw.sensors.magnetic_field".to_string(), "yes".to_string());
    props.insert("hw.sensors.magnetic_field_uncalibrated".to_string(), "yes".to_string());
    props.insert("hw.sensors.orientation".to_string(), "yes".to_string());
    props.insert("hw.sensors.pressure".to_string(), "yes".to_string());
    props.insert("hw.sensors.proximity".to_string(), "yes".to_string());
    props.insert("hw.sensors.temperature".to_string(), "yes".to_string());
    props.insert("hw.useext4".to_string(), "yes".to_string());
    props.insert("image.sysdir.1".to_string(), image.relative_sysdir.clone());
    props.insert("runtime.network.latency".to_string(), "none".to_string());
    props.insert("runtime.network.speed".to_string(), "full".to_string());
    props.insert("sdcard.size".to_string(), "1024 MB".to_string());
    props.insert("showDeviceFrame".to_string(), "yes".to_string());
    props.insert("tag.display".to_string(), "Google Play".to_string());
    props.insert("tag.displaynames".to_string(), "Google Play".to_string());
    props.insert("tag.id".to_string(), "google_apis_playstore".to_string());
    props.insert("tag.ids".to_string(), "google_apis_playstore".to_string());
    props.insert("target".to_string(), format!("android-{}", image.api_level));
    props.insert("vm.heapSize".to_string(), "576M".to_string());

    let config_path = avd_dir.join("config.ini");
    write_prop_file(&config_path, &props)?;
    ensure_user_advanced_features()?;

    let _ = configure_telephony_modem(&avd_dir, host, "72405");

    let issues = diagnose_properties(&props, host, Some(image));
    Ok(AvdProfile {
        name: opts.name.clone(),
        ini_file,
        avd_dir,
        config_path,
        properties: props,
        issues,
    })
}

pub fn ensure_user_advanced_features() -> io::Result<PathBuf> {
    let android_dir = home_dir().join(".android");
    fs::create_dir_all(&android_dir)?;
    let adv_path = android_dir.join("advancedFeatures.ini");

    let mut features = parse_prop_file(&adv_path);
    for (k, v) in [
        ("PlayStoreImage", "on"),
        ("GLESDynamicVersion", "on"),
        ("VirtioWifi", "on"),
        ("BluetoothEmulation", "on"),
    ] {
        features.insert(k.to_string(), v.to_string());
    }
    // Remove low-level memory overrides that can conflict with hybrid Intel/NVIDIA GPUs
    for k in ["GrallocSync", "GLDMA", "GLDirectMem", "Vulkan"] {
        features.remove(k);
    }

    let mut content = String::new();
    for (k, v) in &features {
        content.push_str(&format!("{k} = {v}\n"));
    }
    fs::write(&adv_path, content)?;
    Ok(adv_path)
}

/// Automatically provisions the QEMU modem simulator with Brazilian carrier identities
/// (Claro 72405, Vivo 72406, TIM 72402) and standard 2-digit MNC (EF_AD = 00000002).
/// This prevents Google, WhatsApp, and banking apps from rejecting SMS phone verification
/// due to operator/geographic discrepancies (e.g. US T-Mobile 310260 on a +55 number).
pub fn configure_telephony_modem(avd_dir: &Path, host: &HostReport, carrier_code: &str) -> io::Result<()> {
    let modem_dir = avd_dir.join("modem_simulator");
    let files_dir = modem_dir.join("etc").join("modem_simulator").join("files");
    fs::create_dir_all(&files_dir)?;

    let numeric_operator_content = r#"<?xml version="1.0" encoding="utf-8"?>
<resources xmlns:xliff="urn:oasis:names:tc:xliff:document:1.2">
    <add-resource name="numeric_to_operator" type="array"/>
    <string-array name="numeric_to_operator">
        <item numeric="72405">Claro=CLARO</item>
        <item numeric="72406">Vivo=VIVO</item>
        <item numeric="72402">TIM=TIM</item>
        <item numeric="310260">T-Mobile=TMOBILE</item>
    </string-array>
</resources>
"#;

    let _ = fs::write(files_dir.join("numeric_operator.xml"), numeric_operator_content);

    // Also patch system images modem_simulator templates so cold boots don't revert to USA T-Mobile
    if let Some(sdk_root) = &host.sdk_root {
        let sys_images_root = PathBuf::from(sdk_root).join("system-images");
        if sys_images_root.exists() {
            let mut search_stack = vec![sys_images_root];
            while let Some(dir) = search_stack.pop() {
                if let Ok(entries) = fs::read_dir(&dir) {
                    for entry in entries.flatten() {
                        let path = entry.path();
                        if path.is_dir() {
                            if path.file_name().and_then(|n| n.to_str()) == Some("modem_simulator") {
                                let tmpl_icc = path.join("iccprofile_for_sim0.xml");
                                if tmpl_icc.exists() {
                                    let _ = patch_icc_profile(&tmpl_icc, carrier_code);
                                }
                                let tmpl_num = path.join("etc").join("modem_simulator").join("files").join("numeric_operator.xml");
                                if tmpl_num.exists() {
                                    let _ = fs::write(&tmpl_num, numeric_operator_content);
                                }
                            } else {
                                search_stack.push(path);
                            }
                        }
                    }
                }
            }
        }
    }

    let avd_icc_path = modem_dir.join("iccprofile_for_sim0.xml");
    if avd_icc_path.exists() {
        let _ = patch_icc_profile(&avd_icc_path, carrier_code);
    }

    Ok(())
}

fn patch_icc_profile(path: &Path, carrier_code: &str) -> io::Result<()> {
    let Ok(mut content) = fs::read_to_string(path) else {
        return Ok(());
    };
    let target_cimi = format!("<CIMI>{carrier_code}000000000</CIMI>");
    if let Some(start) = content.find("<CIMI>") {
        if let Some(end) = content[start..].find("</CIMI>") {
            let full_tag = &content[start..start + end + 7];
            content = content.replace(full_tag, &target_cimi);
        }
    }
    // Set 2-digit MNC in EF_AD
    content = content.replace("144,0,00000003", "144,0,00000002");
    fs::write(path, content)?;
    Ok(())
}

pub fn write_prop_file(path: &Path, props: &BTreeMap<String, String>) -> io::Result<()> {
    let mut out = String::new();
    for (k, v) in props {
        out.push_str(&format!("{k}={v}\n"));
    }
    fs::write(path, out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::platform::HostOs;

    #[test]
    fn test_diagnose_broken_playstore_profile() {
        let mut props = BTreeMap::new();
        props.insert("PlayStore.enabled".to_string(), "no".to_string());
        props.insert("tag.id".to_string(), "google_apis_playstore".to_string());
        props.insert("hw.camera.back".to_string(), "none".to_string());
        props.insert("hw.camera.front".to_string(), "webcam1".to_string());
        props.insert("hw.gpu.mode".to_string(), "auto".to_string());

        let dummy_host = HostReport {
            os: HostOs::Linux,
            arch: "x86_64".to_string(),
            cpu_cores: 8,
            hypervisor_available: true,
            hypervisor_detail: String::new(),
            sdk_root: None,
            avd_home: PathBuf::new(),
            emulator_bin: None,
            adb_bin: None,
            sdkmanager_bin: None,
            qemu_bin: None,
            webcams: vec![],
            installed_images: vec![],
        };

        let issues = diagnose_properties(&props, &dummy_host, None);
        assert!(issues.iter().any(|i| i.key == "PlayStore.enabled"));
        assert!(issues.iter().any(|i| i.key == "hw.camera.back"));
        assert!(issues.iter().any(|i| i.key == "hw.gpu.mode"));
    }
}
