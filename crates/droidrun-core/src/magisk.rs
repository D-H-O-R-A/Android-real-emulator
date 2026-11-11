use crate::platform::{home_dir, HostOs, HostReport};
use crate::profile::AvdProfile;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Clone)]
pub struct RootOptions {
    pub profile_name: Option<String>,
    pub fakeboot: bool,
    pub restore: bool,
    pub auto_start: bool,
    pub extra_args: Vec<String>,
}

impl Default for RootOptions {
    fn default() -> Self {
        Self {
            profile_name: None,
            fakeboot: false,
            restore: false,
            auto_start: true,
            extra_args: Vec::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct RamdiskTargetInfo {
    pub relative_path: String,
    pub full_path: PathBuf,
    pub api_level: u32,
    pub primary_abi: String,
    pub is_patched: bool,
    pub has_backup: bool,
}

#[derive(Debug, Clone, Default)]
pub struct LiveMagiskInfo {
    pub has_su: bool,
    pub su_path: Option<String>,
    pub magisk_version: Option<String>,
    pub magisk_version_code: Option<String>,
    pub magisk_app_installed: bool,
    pub zygisk_enabled: bool,
    pub selinux_enforcing: bool,
}

/// Returns the standard installation directory for rootAVD managed by DroidRun (`~/.droidrun/tools/rootAVD`).
pub fn default_rootavd_dir() -> PathBuf {
    home_dir().join(".droidrun").join("tools").join("rootAVD")
}

/// Locates an existing installation of rootAVD on the host system.
pub fn find_existing_rootavd() -> Option<PathBuf> {
    if let Ok(custom) = std::env::var("ROOTAVD_DIR") {
        let p = PathBuf::from(custom);
        if is_valid_rootavd_dir(&p) {
            return Some(p);
        }
    }

    let default_dir = default_rootavd_dir();
    if is_valid_rootavd_dir(&default_dir) {
        return Some(default_dir);
    }

    let local_dirs = [
        PathBuf::from("rootAVD"),
        PathBuf::from("tools/rootAVD"),
        home_dir().join("rootAVD"),
        home_dir().join("Downloads/rootAVD"),
    ];

    for dir in &local_dirs {
        if is_valid_rootavd_dir(dir) {
            return Some(dir.clone());
        }
    }

    None
}

/// Checks whether a given directory contains rootAVD scripts (`rootAVD.sh` or `rootAVD.bat`).
pub fn is_valid_rootavd_dir(path: &Path) -> bool {
    path.join("rootAVD.sh").exists() || path.join("rootAVD.bat").exists()
}

/// Ensures rootAVD is installed, automatically cloning it from GitHub if absent.
pub fn ensure_rootavd_installed(custom_dir: Option<&Path>) -> io::Result<PathBuf> {
    if let Some(existing) = find_existing_rootavd() {
        return Ok(existing);
    }

    let target_dir = match custom_dir {
        Some(p) => p.to_path_buf(),
        None => default_rootavd_dir(),
    };

    if is_valid_rootavd_dir(&target_dir) {
        return Ok(target_dir);
    }

    if let Some(parent) = target_dir.parent() {
        fs::create_dir_all(parent)?;
    }

    println!("[*] Baixando a ferramenta oficial rootAVD do GitHub (https://github.com/newbit1/rootAVD)...");
    let status = Command::new("git")
        .args([
            "clone",
            "--depth",
            "1",
            "https://github.com/newbit1/rootAVD.git",
        ])
        .arg(&target_dir)
        .status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("Falha ao clonar rootAVD para {}", target_dir.display()),
        ));
    }

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let sh_script = target_dir.join("rootAVD.sh");
        if sh_script.exists() {
            let mut perms = fs::metadata(&sh_script)?.permissions();
            perms.set_mode(0o755);
            let _ = fs::set_permissions(&sh_script, perms);
        }
    }

    println!("  [OK] rootAVD instalado com sucesso em: {}", target_dir.display());
    Ok(target_dir)
}

/// Updates the local rootAVD installation using `git pull`.
pub fn update_rootavd(rootavd_dir: &Path) -> io::Result<()> {
    println!("[*] Atualizando rootAVD em {}...", rootavd_dir.display());
    let status = Command::new("git")
        .args(["-C"])
        .arg(rootavd_dir)
        .args(["pull"])
        .status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            "Falha ao executar git pull no diretório do rootAVD.",
        ));
    }
    println!("  [OK] rootAVD atualizado com sucesso!");
    Ok(())
}

/// Scans installed Android system images and lists all ramdisk files with their root/backup status.
pub fn list_system_image_ramdisks(host: &HostReport) -> Vec<RamdiskTargetInfo> {
    let mut targets = Vec::new();
    let Some(sdk_root) = &host.sdk_root else {
        return targets;
    };

    for img in &host.installed_images {
        let rel_sys = img.relative_sysdir.trim_matches('/').to_string();
        let ramdisk_rel = format!("{rel_sys}/ramdisk.img");
        let full_ramdisk = sdk_root.join(&ramdisk_rel);
        if full_ramdisk.exists() {
            let backup_file = sdk_root.join(format!("{rel_sys}/ramdisk.img.backup"));
            let has_backup = backup_file.exists();
            targets.push(RamdiskTargetInfo {
                relative_path: ramdisk_rel,
                full_path: full_ramdisk,
                api_level: img.api_level,
                primary_abi: img.primary_abi.clone(),
                is_patched: has_backup,
                has_backup,
            });
        }
    }

    targets
}

/// Resolves the relative ramdisk path for a specific AVD profile.
pub fn resolve_ramdisk_for_profile(
    host: &HostReport,
    profile: &AvdProfile,
) -> io::Result<String> {
    let Some(sysdir) = profile.properties.get("image.sysdir.1") else {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Propriedade 'image.sysdir.1' não encontrada na configuração do perfil '{}'.", profile.name),
        ));
    };

    let trimmed = sysdir.trim_matches('/');
    let rel_ramdisk = format!("{trimmed}/ramdisk.img");

    if let Some(sdk_root) = &host.sdk_root {
        let full_path = sdk_root.join(&rel_ramdisk);
        if !full_path.exists() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                format!(
                    "Arquivo ramdisk.img não encontrado no SDK: {}",
                    full_path.display()
                ),
            ));
        }
    }

    Ok(rel_ramdisk)
}

/// Restores the original unpatched ramdisk.img from backup for a target relative ramdisk path.
pub fn restore_ramdisk_backup(host: &HostReport, relative_ramdisk: &str) -> io::Result<()> {
    let sdk_root = host.sdk_root.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Android SDK root não encontrado.")
    })?;

    let ramdisk_path = sdk_root.join(relative_ramdisk);
    let backup_path = ramdisk_path.with_extension("img.backup");

    if !backup_path.exists() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!(
                "Nenhum backup encontrado em {}. A imagem já está no estado original.",
                backup_path.display()
            ),
        ));
    }

    fs::copy(&backup_path, &ramdisk_path)?;
    println!(
        "  [OK] Imagem restaurada: {} <- {}",
        ramdisk_path.display(),
        backup_path.display()
    );

    // Also check for kernel-ranchu backup if present
    if let Some(parent) = ramdisk_path.parent() {
        let kernel_orig = parent.join("kernel-ranchu");
        let kernel_backup = parent.join("kernel-ranchu.backup");
        if kernel_backup.exists() {
            let _ = fs::copy(&kernel_backup, &kernel_orig);
            println!("  [OK] Kernel original restaurado: {}", kernel_orig.display());
        }
    }

    Ok(())
}

/// Waits for an Android instance to be fully booted via ADB (`sys.boot_completed == 1`).
pub fn wait_for_device_boot(host: &HostReport, timeout_secs: u64) -> io::Result<String> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Binário ADB não encontrado.")
    })?;

    println!("[*] Aguardando o Android inicializar completamente via ADB (timeout: {}s)...", timeout_secs);
    let start = Instant::now();
    let timeout = Duration::from_secs(timeout_secs);

    // First wait for device server to detect device
    let _ = Command::new(adb).args(["wait-for-device"]).status();

    while start.elapsed() < timeout {
        let out = Command::new(adb)
            .args(["shell", "getprop", "sys.boot_completed"])
            .output();

        if let Ok(o) = out {
            let val = String::from_utf8_lossy(&o.stdout).trim().to_string();
            if val == "1" {
                println!("  [OK] Android inicializado com sucesso e pronto para conexões ADB!");
                return Ok("device_online".to_string());
            }
        }
        thread::sleep(Duration::from_millis(1500));
    }

    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "Tempo esgotado aguardando boot completo do Android (sys.boot_completed=1).",
    ))
}

/// Executes rootAVD with the proper environment variables and arguments to patch the ramdisk.
pub fn run_rootavd_script(
    host: &HostReport,
    rootavd_dir: &Path,
    relative_ramdisk: &str,
    opts: &RootOptions,
) -> io::Result<()> {
    let sdk_root = host.sdk_root.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Android SDK root não encontrado.")
    })?;

    let platform_tools = host.adb_bin.as_ref().and_then(|p| p.parent()).ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "Pasta platform-tools do ADB não encontrada.")
    })?;

    let mut env_path = platform_tools.to_string_lossy().to_string();
    if let Ok(existing) = std::env::var("PATH") {
        #[cfg(windows)]
        {
            env_path = format!("{};{}", env_path, existing);
        }
        #[cfg(not(windows))]
        {
            env_path = format!("{}:{}", env_path, existing);
        }
    }

    let mut cmd_args: Vec<String> = Vec::new();
    cmd_args.push(relative_ramdisk.to_string());

    if opts.restore {
        cmd_args.push("restore".to_string());
    } else if opts.fakeboot {
        cmd_args.push("FAKEBOOTIMG".to_string());
    }

    for extra in &opts.extra_args {
        cmd_args.push(extra.clone());
    }

    println!("\n================================================================================");
    println!("  Iniciando Injeção Nativa do Magisk via rootAVD");
    println!("  Alvo: {}", relative_ramdisk);
    println!("  Comando: rootAVD {}", cmd_args.join(" "));
    println!("================================================================================\n");

    let is_windows = host.os == HostOs::Windows;
    let mut command = if is_windows {
        let bat = rootavd_dir.join("rootAVD.bat");
        let mut c = Command::new("cmd");
        c.arg("/c").arg(bat);
        c
    } else {
        let sh = rootavd_dir.join("rootAVD.sh");
        let mut c = Command::new("bash");
        c.arg(sh);
        c
    };

    command.args(&cmd_args);
    command.current_dir(rootavd_dir);
    command.env("ANDROID_HOME", sdk_root);
    command.env("ANDROID_SDK_ROOT", sdk_root);
    command.env("PATH", env_path);

    command.stdin(Stdio::inherit());
    command.stdout(Stdio::inherit());
    command.stderr(Stdio::inherit());

    let status = command.status()?;

    if !status.success() {
        return Err(io::Error::new(
            io::ErrorKind::Other,
            format!("rootAVD finalizou com código de erro: {:?}", status.code()),
        ));
    }

    Ok(())
}

/// Checks live Magisk and root status on a running Android device via ADB.
pub fn inspect_live_magisk(host: &HostReport, serial: Option<&str>) -> LiveMagiskInfo {
    let mut info = LiveMagiskInfo::default();
    let Some(adb) = &host.adb_bin else {
        return info;
    };

    let run_cmd = |args: &[&str]| -> Option<String> {
        let mut c = Command::new(adb);
        if let Some(s) = serial {
            c.arg("-s").arg(s);
        }
        c.args(args);
        let out = c.output().ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let combined = if !stdout.is_empty() { stdout } else { stderr };
        if out.status.success() || !combined.is_empty() {
            Some(combined)
        } else {
            None
        }
    };

    // Check 'which su'
    if let Some(su) = run_cmd(&["shell", "which", "su"]) {
        if !su.is_empty() && !su.contains("not found") {
            info.has_su = true;
            info.su_path = Some(su);
        }
    }

    // Check magisk binary version
    if let Some(ver) = run_cmd(&["shell", "magisk", "-v"]) {
        if !ver.is_empty() && !ver.contains("not found") {
            info.magisk_version = Some(ver);
        }
    }
    if let Some(code) = run_cmd(&["shell", "magisk", "-V"]) {
        if !code.is_empty() && !code.contains("not found") {
            info.magisk_version_code = Some(code);
        }
    }

    // Check Magisk App package
    if let Some(pkgs) = run_cmd(&["shell", "pm", "list", "packages"]) {
        if pkgs.lines().any(|l| l.contains("com.topjohnwu.magisk")) {
            info.magisk_app_installed = true;
        }
    }

    // Check SELinux status
    if let Some(enforce) = run_cmd(&["shell", "getenforce"]) {
        info.selinux_enforcing = enforce.eq_ignore_ascii_case("Enforcing");
    }

    // Check Zygisk status (via persist.sys.zygisk or magisk database)
    if let Some(zygisk_prop) = run_cmd(&["shell", "getprop", "persist.sys.zygisk"]) {
        if zygisk_prop == "1" || zygisk_prop.eq_ignore_ascii_case("true") {
            info.zygisk_enabled = true;
        }
    }
    if !info.zygisk_enabled {
        if let Some(sqlite_out) = run_cmd(&["shell", "su -c 'magisk --sqlite \"select * from settings;\"' 2>&1"]) {
            if sqlite_out.contains("key=zygisk|value=1") || sqlite_out.contains("zygisk|value=1") || sqlite_out.contains("zygisk|1") {
                info.zygisk_enabled = true;
            }
        }
    }

    info
}

#[derive(Debug, Clone, Default)]
pub struct IntegrityReport {
    pub zygisk_enabled: bool,
    pub denylist_enabled: bool,
    pub apps_denylisted: Vec<String>,
    pub spoof_script_installed: bool,
    pub live_props_applied: bool,
}

#[derive(Debug, Clone, Default)]
pub struct IntegrityStatus {
    pub zygisk_enabled: bool,
    pub denylist_enabled: bool,
    pub verified_boot_state: String,
    pub flash_locked: String,
    pub vbmeta_device_state: String,
    pub build_fingerprint: String,
    pub build_type: String,
    pub debuggable: String,
    pub sepolicy_enforcing: bool,
}

/// Applies security hardening, Zygisk configuration, Magisk DenyList enforcement,
/// and bootloader/CTS fingerprint spoofing to ensure the device passes SafetyNet
/// (Basic Integrity, CTS Profile Match) and Google Play Integrity API checks.
pub fn configure_integrity_and_denylist(
    host: &HostReport,
    serial: Option<&str>,
    extra_packages: &[&str],
) -> io::Result<IntegrityReport> {
    let adb = host.adb_bin.as_ref().ok_or_else(|| {
        io::Error::new(io::ErrorKind::NotFound, "ADB não encontrado no HostReport")
    })?;

    let run_adb = |args: &[&str]| -> io::Result<String> {
        let mut c = Command::new(adb);
        if let Some(s) = serial {
            c.arg("-s").arg(s);
        }
        c.args(args);
        let out = c.output()?;
        let res = String::from_utf8_lossy(&out.stdout).to_string();
        Ok(res)
    };

    let mut report = IntegrityReport::default();

    // 1. Enable Zygisk in Magisk DB
    let zygisk_cmd = "magisk --sqlite \"insert or replace into settings (key,value) values('zygisk',1);\"";
    let _ = run_adb(&["shell", "su", "0", "-c", zygisk_cmd]);
    let _ = run_adb(&["shell", "su -c 'magisk --sqlite \"insert or replace into settings (key,value) values(\\\"zygisk\\\",1);\"'"]);
    report.zygisk_enabled = true;

    // 2. Enable DenyList in Magisk
    let denylist_enable = "magisk --denylist enable";
    let _ = run_adb(&["shell", "su", "0", "-c", denylist_enable]);
    let _ = run_adb(&["shell", "su -c 'magisk --denylist enable'"]);
    report.denylist_enabled = true;

    // 3. Denylist standard packages for SafetyNet / Play Integrity
    let mut packages_to_hide = vec![
        ("com.google.android.gms", Some("com.google.android.gms.unstable")),
        ("com.google.android.gms", None),
        ("com.android.vending", None),
        ("com.flinkapps.safteynet", None),
    ];
    for extra in extra_packages {
        packages_to_hide.push((extra, None));
    }

    for (pkg, proc) in packages_to_hide {
        let cmd = match proc {
            Some(p) => format!("magisk --denylist add {pkg} {p}"),
            None => format!("magisk --denylist add {pkg}"),
        };
        let _ = run_adb(&["shell", "su", "-c", &cmd]);
        report.apps_denylisted.push(match proc {
            Some(p) => format!("{pkg} ({p})"),
            None => pkg.to_string(),
        });
    }

    // 4. Create and deploy /data/adb/service.d/00-integrity-spoof.sh
    let spoof_script = r#"#!/system/bin/sh
# DroidRun - Device Integrity & SafetyNet/Play Integrity Hardware Spoofing
MAGISK_DIR="/data/adb/magisk"
RESETPROP="$MAGISK_DIR/resetprop"
if [ ! -x "$RESETPROP" ]; then
    RESETPROP="resetprop"
fi

# 1. Enforce verified locked boot state
$RESETPROP -n ro.boot.verifiedbootstate green
$RESETPROP -n ro.boot.flash.locked 1
$RESETPROP -n ro.boot.vbmeta.device_state locked
$RESETPROP -n ro.boot.veritymode enforcing
$RESETPROP -n vendor.boot.verifiedbootstate green
$RESETPROP -n vendor.boot.vbmeta.device_state locked
$RESETPROP -n sys.oem_unlock_allowed 0

# 2. Enforce release/user build props (no test-keys or debuggable flags)
$RESETPROP -n ro.build.type user
$RESETPROP -n ro.build.tags release-keys
$RESETPROP -n ro.debuggable 0
$RESETPROP -n ro.secure 1
$RESETPROP -n ro.adb.secure 1

# 3. Dynamic Certified CTS device fingerprint based on Android SDK level
SDK_VER="$(getprop ro.build.version.sdk)"
if [ "$SDK_VER" = "30" ] || [ "$SDK_VER" -le 30 ]; then
    # Android 11: Google Pixel 5 (redfin) CTS Certified Profile
    DEV_NAME="redfin"
    FP="google/redfin/redfin:11/RQ3A.211001.001/7641976:user/release-keys"
    $RESETPROP -n ro.build.fingerprint "$FP"
    $RESETPROP -n ro.bootimage.build.fingerprint "$FP"
    $RESETPROP -n ro.system.build.fingerprint "$FP"
    $RESETPROP -n ro.vendor.build.fingerprint "$FP"
    $RESETPROP -n ro.product.brand "google"
    $RESETPROP -n ro.product.manufacturer "Google"
    $RESETPROP -n ro.product.model "Pixel 5"
    $RESETPROP -n ro.product.name "redfin"
    $RESETPROP -n ro.product.device "redfin"
    $RESETPROP -n ro.product.board "redfin"
    $RESETPROP -n ro.hardware "redfin"
    $RESETPROP -n ro.boot.hardware "redfin"
    $RESETPROP -n ro.board.platform "lito"
    $RESETPROP -n ro.boot.hardware.sku "redfin"
    $RESETPROP -n ro.boot.hardware.platform "lito"
    $RESETPROP -n ro.soc.model "Snapdragon 765G"
    $RESETPROP -n ro.soc.manufacturer "Qualcomm"
    $RESETPROP -n ro.build.version.security_patch "2021-10-01"
else
    # Android 13/14/15: Google Pixel 7 (panther) CTS Certified Profile
    DEV_NAME="panther"
    FP="google/panther/panther:14/AP1A.240505.004/11581452:user/release-keys"
    $RESETPROP -n ro.build.fingerprint "$FP"
    $RESETPROP -n ro.bootimage.build.fingerprint "$FP"
    $RESETPROP -n ro.system.build.fingerprint "$FP"
    $RESETPROP -n ro.vendor.build.fingerprint "$FP"
    $RESETPROP -n ro.product.brand "google"
    $RESETPROP -n ro.product.manufacturer "Google"
    $RESETPROP -n ro.product.model "Pixel 7"
    $RESETPROP -n ro.product.name "panther"
    $RESETPROP -n ro.product.device "panther"
    $RESETPROP -n ro.product.board "panther"
    $RESETPROP -n ro.hardware "panther"
    $RESETPROP -n ro.boot.hardware "panther"
    $RESETPROP -n ro.board.platform "gs201"
    $RESETPROP -n ro.boot.hardware.sku "panther"
    $RESETPROP -n ro.boot.hardware.platform "gs201"
    $RESETPROP -n ro.soc.model "Tensor G2"
    $RESETPROP -n ro.soc.manufacturer "Google"
    $RESETPROP -n ro.build.version.security_patch "2024-05-05"
fi
$RESETPROP -n ro.hardware.vulkan "$DEV_NAME"
$RESETPROP -n ro.hardware.gralloc "$DEV_NAME"
$RESETPROP -n ro.hardware.power "$DEV_NAME"
$RESETPROP -n ro.boot.hardware.vulkan "$DEV_NAME"

# 5. Fallback to basic software attestation & remove all emulator signatures
$RESETPROP -n ro.product.first_api_level 25
$RESETPROP -n ro.board.first_api_level 25
$RESETPROP --delete ro.kernel.qemu
$RESETPROP --delete ro.boot.qemu
$RESETPROP --delete ro.boot.qemu.avd_name
$RESETPROP --delete qemu.sf.lcd_density
for p in $($RESETPROP | grep -oE "ro\.boot\.qemu\.[a-zA-Z0-9_\.]+"); do
    $RESETPROP --delete "$p"
done
for p in $($RESETPROP | grep -oE "vendor\.qemu\.[a-zA-Z0-9_\.]+"); do
    $RESETPROP --delete "$p"
done
"#;

    // Ensure /data/adb/service.d and post-fs-data.d directories exist
    let _ = run_adb(&["shell", "su", "-c", "mkdir -p /data/adb/service.d /data/adb/post-fs-data.d"]);

    // Write temporary script to host temp dir, then push via adb
    let tmp_path = std::env::temp_dir().join(format!("droidrun-spoof-{}.sh", std::process::id()));
    fs::write(&tmp_path, spoof_script)?;

    let mut push_cmd = Command::new(adb);
    if let Some(s) = serial {
        push_cmd.arg("-s").arg(s);
    }
    push_cmd.args(["push", tmp_path.to_str().unwrap(), "/data/local/tmp/00-integrity-spoof.sh"]);
    let _ = push_cmd.output();
    let _ = fs::remove_file(&tmp_path);

    let move_cmd = "cp /data/local/tmp/00-integrity-spoof.sh /data/adb/service.d/00-integrity-spoof.sh && cp /data/local/tmp/00-integrity-spoof.sh /data/adb/post-fs-data.d/00-integrity-spoof.sh && chmod 755 /data/adb/service.d/00-integrity-spoof.sh /data/adb/post-fs-data.d/00-integrity-spoof.sh && rm -f /data/local/tmp/00-integrity-spoof.sh";
    let _ = run_adb(&["shell", "su", "-c", move_cmd]);
    report.spoof_script_installed = true;

    // 5. Execute immediately to spoof live properties without requiring a reboot
    let run_now = "sh /data/adb/service.d/00-integrity-spoof.sh";
    let _ = run_adb(&["shell", "su", "-c", run_now]);
    report.live_props_applied = true;

    Ok(report)
}

/// Inspects the live integrity properties on a connected Android device.
pub fn inspect_integrity_status(host: &HostReport, serial: Option<&str>) -> IntegrityStatus {
    let mut status = IntegrityStatus::default();
    let Some(adb) = &host.adb_bin else {
        return status;
    };
    let run_cmd = |args: &[&str]| -> Option<String> {
        let mut c = Command::new(adb);
        if let Some(s) = serial {
            c.arg("-s").arg(s);
        }
        c.args(args);
        let out = c.output().ok()?;
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        let combined = if !stdout.is_empty() { stdout } else { stderr };
        if out.status.success() || !combined.is_empty() {
            Some(combined)
        } else {
            None
        }
    };

    if let Some(v) = run_cmd(&["shell", "getprop", "ro.boot.verifiedbootstate"]) {
        status.verified_boot_state = v;
    }
    if let Some(v) = run_cmd(&["shell", "getprop", "ro.boot.flash.locked"]) {
        status.flash_locked = v;
    }
    if let Some(v) = run_cmd(&["shell", "getprop", "ro.boot.vbmeta.device_state"]) {
        status.vbmeta_device_state = v;
    }
    if let Some(v) = run_cmd(&["shell", "getprop", "ro.build.fingerprint"]) {
        status.build_fingerprint = v;
    }
    if let Some(v) = run_cmd(&["shell", "getprop", "ro.build.type"]) {
        status.build_type = v;
    }
    if let Some(v) = run_cmd(&["shell", "getprop", "ro.debuggable"]) {
        status.debuggable = v;
    }
    if let Some(enforce) = run_cmd(&["shell", "getenforce"]) {
        status.sepolicy_enforcing = enforce.eq_ignore_ascii_case("Enforcing");
    }
    if let Some(dl) = run_cmd(&["shell", "su -c 'magisk --denylist status' 2>&1"]) {
        let dl_lower = dl.to_lowercase();
        status.denylist_enabled = dl_lower.contains("enabled") || dl_lower.contains("enforce");
    }
    if let Some(sqlite_out) = run_cmd(&["shell", "su -c 'magisk --sqlite \"select * from settings;\"' 2>&1"]) {
        if sqlite_out.contains("key=zygisk|value=1") || sqlite_out.contains("zygisk|value=1") || sqlite_out.contains("zygisk|1") {
            status.zygisk_enabled = true;
        }
    }
    status
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_rootavd_dir_not_empty() {
        let p = default_rootavd_dir();
        assert!(p.to_string_lossy().contains(".droidrun"));
        assert!(p.to_string_lossy().contains("rootAVD"));
    }

    #[test]
    fn test_root_options_defaults() {
        let opts = RootOptions::default();
        assert!(!opts.fakeboot);
        assert!(!opts.restore);
        assert!(opts.auto_start);
        assert!(opts.extra_args.is_empty());
    }
}
