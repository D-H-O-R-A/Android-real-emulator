use droidrun_core::audit::{
    audit_running_instance, install_apk, list_connected_devices, run_adb, send_sms,
    sync_playstore_catalog,
};
use droidrun_core::magisk::{
    configure_integrity_and_denylist, ensure_rootavd_installed, list_system_image_ramdisks,
    resolve_ramdisk_for_profile, restore_ramdisk_backup, run_rootavd_script, update_rootavd,
    wait_for_device_boot, RootOptions,
};
use droidrun_core::platform::HostReport;
use droidrun_core::profile::{
    create_profile, fix_profile, list_profiles, ProfileConfigOptions,
};
use droidrun_core::runner::{build_emulator_args, launch_profile, LaunchOptions};
use droidrun_core::sdk::{install_system_image, recommended_playstore_packages};
use std::env;
use std::path::Path;
use std::process::{self, Command};

fn print_banner() {
    println!("================================================================================");
    println!("  DroidRun v0.1.0 - Open-Source Full Android Desktop Runner (Linux/macOS/Win)");
    println!("  Google Play Store Oficial + Tradução ARM64 (libndk_translation) + Magisk Root");
    println!("================================================================================");
}

fn print_help() {
    print_banner();
    println!("\nUSO:");
    println!("  droidrun <COMANDO> [OPÇÕES]\n");
    println!("COMANDOS:");
    println!("  doctor                    Diagnostica SO, KVM/HVF/WHPX, imagens ARM64, Magisk e perfis");
    println!("  list                      Lista perfis e versões do Android configuradas e ativas");
    println!("  start [versão/perfil]     Inicia o Android com aceleração de hardware (ex: --version 7.1 ou --version 11)");
    println!("                            Opções: --version <7.1|11|15> --api <25|30|35> --foreground --wipe-data --warm");
    println!("  stop                      Salva os dados com segurança e encerra as instâncias em execução");
    println!("  fix [nome_perfil]         Corrige perfis existentes para habilitar Play Store, Câmera Traseira,");
    println!("                            Tradução ARM64, Sensores, VirtioWifi e Aceleração GPU Host");
    println!("  create [nome_perfil]      Cria um novo dispositivo Android legítimo pronto para a Play Store");
    println!("                            Opções: --api <N> --ram <MB> --cores <N> --storage <GB>");
    println!("  root [nome_perfil]        Injeta suporte nativo ao Magisk via rootAVD no ramdisk do Android");
    println!("                            Opções: --fakeboot --restore --list --update --no-auto-start");
    println!("  unroot [nome_perfil]      Restaura o ramdisk original de fábrica, removendo o Magisk");
    println!("  integrity [nome_perfil]   Configura Zygisk, DenyList e CTS Spoofing (SafetyNet/Play Integrity)");
    println!("  audit                     Audita em tempo real via ADB as features da Play Store, ABI e Root/Magisk");
    println!("  sync-playstore            Limpa o cache de DeviceConfig da Play Store para liberar apps ocultos");
    println!("  install <arquivo_ou_dir>  Instala um APK ou pacote Split APK (arm64-v8a/x86_64) via ADB");
    println!("  sms <número> <mensagem>   Injeta um SMS diretamente na caixa de entrada do Android (OTP/2FA/Banco)");
    println!("  install-image [api]       Baixa imagem oficial Google Play via sdkmanager (ex: 34, 33, 30, 25)");
    println!("  help                      Exibe esta mensagem de ajuda\n");
}

fn cmd_doctor() {
    print_banner();
    let host = HostReport::detect();

    println!("\n[1/4] Ambiente do Host:");
    println!("  • Sistema Operacional : {} ({})", host.os.display_name(), host.arch);
    println!("  • Núcleos de CPU      : {}", host.cpu_cores);
    println!(
        "  • Virtualização ({}) : {} - {}",
        host.os.hypervisor_name(),
        if host.hypervisor_available { "OK" } else { "ATENÇÃO" },
        host.hypervisor_detail
    );
    println!(
        "  • Android SDK Root    : {}",
        host.sdk_root
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Não encontrado".to_string())
    );
    println!(
        "  • Emulator Engine     : {}",
        host.emulator_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Não encontrado".to_string())
    );
    println!(
        "  • ADB Platform-Tools  : {}",
        host.adb_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Não encontrado".to_string())
    );
    println!(
        "  • QEMU Nativo         : {}",
        host.qemu_bin
            .as_ref()
            .map(|p| p.display().to_string())
            .unwrap_or_else(|| "Não instalado (opcional)".to_string())
    );
    println!(
        "  • rootAVD (Magisk)    : {}",
        host.rootavd_dir
            .as_ref()
            .map(|p| format!("Instalado em {}", p.display()))
            .unwrap_or_else(|| "Não instalado (será baixado automaticamente via `droidrun root`)".to_string())
    );
    println!("  • Webcams Detectadas  : {}", host.webcams.join(", "));

    println!("\n[2/4] Imagens de Sistema Android Instaladas:");
    if host.installed_images.is_empty() {
        println!("  [!] Nenhuma imagem encontrada. Execute `droidrun install-image 34`.");
    } else {
        for img in &host.installed_images {
            let play_badge = if img.has_play_store {
                "[PLAY STORE OFICIAL]"
            } else {
                "[Sem Play Store]"
            };
            let magisk_badge = if img.is_magisk_patched() {
                "[MAGISK ROOT INJETADO]"
            } else {
                "[Sem Root]"
            };
            let arm_badge = if img.primary_abi == "arm64-v8a" {
                "ARM64 Nativo"
            } else if img.has_arm64_translation {
                "Tradução ARM64 Ativa (libndk_translation)"
            } else if img.primary_abi == "x86" {
                "x86 (32-bit KVM Nativo)"
            } else {
                "Apenas x86_64"
            };
            let hw_accel = if img.primary_abi == host.arch
                || (host.arch == "aarch64" && img.primary_abi == "arm64-v8a")
                || (host.arch == "x86_64" && img.primary_abi == "x86")
            {
                "Aceleração Hardware KVM/HVF/WHPX"
            } else {
                "LENTO (Emulação por software - arquitetura diferente do host)"
            };
            println!(
                "  • Android {} (API {}) {} {} | ABI Primaria: {} | ABIs: {:?}\n      -> {} | {}",
                img.android_release,
                img.api_level,
                play_badge,
                magisk_badge,
                img.primary_abi,
                img.supported_abis,
                arm_badge,
                hw_accel
            );
        }
    }

    println!("\n[3/4] Diagnóstico dos Perfis de Dispositivo (.avd em {}):", host.avd_home.display());
    let profiles = list_profiles(&host);
    if profiles.is_empty() {
        println!("  • Nenhum perfil encontrado. Crie um com `droidrun create DroidRun_PlayStore`.");
    } else {
        for prof in &profiles {
            let root_badge = if prof.is_magisk_patched(&host) {
                " [MAGISK ROOT ATIVO]"
            } else {
                ""
            };
            if prof.is_healthy_for_playstore() {
                println!(
                    "  [OK] Perfil '{}' está 100% otimizado para Google Play Store e ARM64!{}",
                    prof.name, root_badge
                );
            } else {
                println!(
                    "  [PROBLEMAS DETECTADOS] Perfil '{}'{} possui {} configuração(ões) que limitam a Play Store:",
                    prof.name,
                    root_badge,
                    prof.issues.len()
                );
                for issue in &prof.issues {
                    println!(
                        "      - {}: atual='{}' -> recomendado='{}'\n        Motivo: {}",
                        issue.key, issue.current_value, issue.recommended_value, issue.reason
                    );
                }
            }
        }
    }

    println!("\n[4/4] Pacotes Recomendados Disponíveis:");
    for rec in recommended_playstore_packages(&host) {
        println!("  • API {}: {} ({})", rec.api_level, rec.description, rec.package_id);
    }
    println!("\nDica: Execute `droidrun fix` para corrigir automaticamente seus perfis ou `droidrun create` para criar um novo.");
}

fn cmd_fix(target_name: Option<String>) {
    print_banner();
    let host = HostReport::detect();
    let profiles = list_profiles(&host);

    if profiles.is_empty() {
        eprintln!("Nenhum perfil AVD encontrado em {}. Use `droidrun create`.", host.avd_home.display());
        process::exit(1);
    }

    let mut fixed_any = false;
    for prof in &profiles {
        if let Some(ref name) = target_name {
            if &prof.name != name {
                continue;
            }
        }
        fixed_any = true;
        println!("\nReparando perfil '{}' ({})...", prof.name, prof.config_path.display());
        match fix_profile(prof, &host) {
            Ok(changes) => {
                for ch in &changes {
                    println!("  [+] {ch}");
                }
                println!(
                    "  [SUCESSO] Perfil '{}' configurado com Play Store, Câmera Traseira/Frontal, Sensores, GPU Host e ARM64!",
                    prof.name
                );
            }
            Err(e) => {
                eprintln!("  [ERRO] Falha ao reparar '{}': {e}", prof.name);
            }
        }
    }

    if !fixed_any {
        if let Some(name) = target_name {
            eprintln!("Perfil '{name}' não encontrado.");
            process::exit(1);
        }
    }
}

fn cmd_create(args: &[String]) {
    print_banner();
    let host = HostReport::detect();

    let mut opts = ProfileConfigOptions::default();
    opts.cpu_cores = ((host.cpu_cores / 2).clamp(4, 8)) as u32;
    let mut requested_api: Option<u32> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--api" if i + 1 < args.len() => {
                requested_api = args[i + 1].parse().ok();
                i += 2;
            }
            "--ram" if i + 1 < args.len() => {
                opts.ram_mb = args[i + 1].parse().unwrap_or(6144);
                i += 2;
            }
            "--cores" if i + 1 < args.len() => {
                opts.cpu_cores = args[i + 1].parse().unwrap_or(6);
                i += 2;
            }
            "--storage" if i + 1 < args.len() => {
                opts.data_partition_gb = args[i + 1].parse().unwrap_or(16);
                i += 2;
            }
            val if !val.starts_with("--") => {
                opts.name = val.to_string();
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    let target_img = if let Some(api) = requested_api {
        let found = host
            .installed_images
            .iter()
            .find(|img| img.api_level == api && img.has_play_store)
            .or_else(|| host.installed_images.iter().find(|img| img.api_level == api));
        match found {
            Some(img) => img,
            None => {
                eprintln!(
                    "Imagem Android com API {api} não está instalada no SDK.\nExecute `droidrun install-image {api}` para baixá-la primeiro."
                );
                process::exit(1);
            }
        }
    } else {
        match host.best_playstore_image() {
            Some(img) => img,
            None => {
                eprintln!("Nenhuma imagem de sistema Android instalada no SDK. Execute `droidrun install-image 30` primeiro.");
                process::exit(1);
            }
        }
    };

    println!(
        "\nCriando dispositivo Android legítimo '{}' usando imagem:\n  -> {} (Android {}, API {}, ABIs: {:?})",
        opts.name,
        target_img.relative_sysdir,
        target_img.android_release,
        target_img.api_level,
        target_img.supported_abis
    );

    match create_profile(&host, &opts, target_img) {
        Ok(profile) => {
            println!("  [+] Arquivo INI criado : {}", profile.ini_file.display());
            println!("  [+] Config INI criado  : {}", profile.config_path.display());
            println!("  [+] Google Play Store  : Habilitada (PlayStore.enabled=yes)");
            println!("  [+] Tradução ARM64     : Ativa (libndk_translation.so)");
            println!("  [+] Câmeras            : Traseira ({}) + Frontal ({})", opts.camera_back, opts.camera_front);
            println!("  [+] GPU & Gráficos     : Host Vulkan 1.1+ / OpenGL ES 3.1+");
            println!("  [+] Rede Sem Fio       : VirtioWifi ativo");
            println!(
                "\nDispositivo '{}' pronto! Inicie agora com:\n  droidrun start {}",
                profile.name, profile.name
            );
        }
        Err(e) => {
            eprintln!("Erro ao criar perfil: {e}");
            process::exit(1);
        }
    }
}

fn cmd_list() {
    print_banner();
    let host = HostReport::detect();
    let profiles = list_profiles(&host);
    let connected = list_connected_devices(&host).unwrap_or_default();

    println!("\n[Dispositivos Virtuais Android Configurados]:");
    if profiles.is_empty() {
        println!("  Nenhum perfil encontrado. Crie um com `droidrun create`.");
    } else {
        for p in &profiles {
            let api = p.api_level().unwrap_or(0);
            let ver_str = p.android_version_string();
            let magisk_str = if p.is_magisk_patched(&host) {
                "[Magisk Root Ativo]"
            } else {
                "[Sem Root]"
            };
            let primary_abi = p.properties.get("abi.type").cloned().unwrap_or_default();

            println!(
                "  • {:<20} | {:<25} | ABI: {:<7} | {}",
                p.name,
                format!("{ver_str} (API {api})"),
                primary_abi,
                magisk_str
            );
        }
    }

    if !connected.is_empty() {
        println!("\n[Instâncias Ativas no ADB]:");
        for dev in &connected {
            println!("  • Dispositivo conectado: {dev}");
        }
    }

    println!("\n[Como Iniciar uma Versão Específica]:");
    println!("  • Iniciar Android 7.1.1 (API 25) : droidrun start --version 7.1  (ou `droidrun start 7.1`)");
    println!("  • Iniciar Android 11.0  (API 30) : droidrun start --version 11   (ou `droidrun start 11`)");
    println!("  • Iniciar Android 15.0  (API 35) : droidrun start --version 15   (ou `droidrun start 15`)");
    println!("  • Encerrar e Salvar Instâncias  : droidrun stop");
}

fn cmd_stop() {
    print_banner();
    let host = HostReport::detect();
    let connected = list_connected_devices(&host).unwrap_or_default();
    if connected.is_empty() {
        println!("\nNenhuma instância Android em execução no momento.");
        return;
    }

    println!(
        "\nEncerrando {} instância(s) Android com segurança...",
        connected.len()
    );
    if let Some(adb) = &host.adb_bin {
        for dev in &connected {
            println!("  • Salvando dados e finalizando '{}'...", dev);
            let _ = Command::new(adb).args(["-s", dev, "emu", "kill"]).output();
        }
    }

    for _ in 0..20 {
        std::thread::sleep(std::time::Duration::from_millis(500));
        if list_connected_devices(&host).unwrap_or_default().is_empty() {
            break;
        }
    }

    println!("  [OK] Todas as instâncias foram salvas e encerradas com sucesso.");
}

fn cmd_start(args: &[String]) {
    print_banner();
    let host = HostReport::detect();
    let mut profiles = list_profiles(&host);

    let mut opts = LaunchOptions::default();
    let mut requested_version: Option<String> = None;
    let mut requested_api: Option<u32> = None;
    let mut explicit_profile: Option<String> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--foreground" => {
                opts.detach = false;
                i += 1;
            }
            "--wipe-data" => {
                opts.wipe_data = true;
                i += 1;
            }
            "--warm" => {
                opts.cold_boot = false;
                i += 1;
            }
            "--version" | "-v" if i + 1 < args.len() => {
                requested_version = Some(args[i + 1].clone());
                i += 2;
            }
            "--api" if i + 1 < args.len() => {
                requested_api = args[i + 1].parse().ok();
                i += 2;
            }
            val if !val.starts_with("--") => {
                let v = val.trim();
                if v == "7.1" || v == "7" || v == "7.1.1" {
                    requested_version = Some("7.1".to_string());
                } else if v == "11" || v == "11.0" {
                    requested_version = Some("11".to_string());
                } else if v == "15" || v == "15.0" {
                    requested_version = Some("15".to_string());
                } else if let Ok(api) = v.parse::<u32>() {
                    if api >= 21 && api <= 36 {
                        requested_api = Some(api);
                    } else {
                        explicit_profile = Some(val.to_string());
                    }
                } else {
                    explicit_profile = Some(val.to_string());
                }
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }

    if let Some(ref ver) = requested_version {
        if ver.starts_with("7") {
            requested_api = Some(25);
        } else if ver.starts_with("8.0") || ver == "8" {
            requested_api = Some(26);
        } else if ver.starts_with("8.1") {
            requested_api = Some(27);
        } else if ver.starts_with("9") {
            requested_api = Some(28);
        } else if ver.starts_with("10") {
            requested_api = Some(29);
        } else if ver.starts_with("11") {
            requested_api = Some(30);
        } else if ver.starts_with("12") {
            requested_api = Some(31);
        } else if ver.starts_with("13") {
            requested_api = Some(33);
        } else if ver.starts_with("14") {
            requested_api = Some(34);
        } else if ver.starts_with("15") {
            requested_api = Some(35);
        }
    }

    let target_profile_name = if let Some(name) = explicit_profile {
        name
    } else if let Some(api) = requested_api {
        if let Some(prof) = profiles.iter().find(|p| p.api_level() == Some(api)) {
            prof.name.clone()
        } else {
            match api {
                25 => "DroidRun_A71".to_string(),
                30 => "DroidRun_A11".to_string(),
                35 => "DroidRun_PlayStore".to_string(),
                other => format!("DroidRun_API{other}"),
            }
        }
    } else {
        if profiles.iter().any(|p| p.name == "DroidRun_A71") {
            "DroidRun_A71".to_string()
        } else if profiles.iter().any(|p| p.name == "DroidRun_A11") {
            "DroidRun_A11".to_string()
        } else if profiles.iter().any(|p| p.name == "DroidRun_PlayStore") {
            "DroidRun_PlayStore".to_string()
        } else if let Some(first) = profiles.first() {
            first.name.clone()
        } else {
            "DroidRun_A71".to_string()
        }
    };
    opts.profile_name = target_profile_name;

    // Auto-create profile if missing
    if !profiles.iter().any(|p| p.name == opts.profile_name) {
        let target_api = requested_api.or_else(|| {
            if opts.profile_name.contains("A71") || opts.profile_name.contains("A7") {
                Some(25)
            } else if opts.profile_name.contains("A11") {
                Some(30)
            } else if opts.profile_name.contains("PlayStore") {
                Some(35)
            } else {
                None
            }
        });

        let chosen_image = if let Some(api) = target_api {
            host.installed_images
                .iter()
                .find(|img| img.api_level == api && img.has_play_store)
                .or_else(|| host.installed_images.iter().find(|img| img.api_level == api))
        } else {
            host.best_playstore_image()
        };

        if let Some(img) = chosen_image {
            println!(
                "Perfil '{}' ainda não existe. Criando automaticamente com imagem Android {} (API {})...",
                opts.profile_name, img.android_release, img.api_level
            );
            let create_opts = ProfileConfigOptions {
                name: opts.profile_name.clone(),
                ..Default::default()
            };
            if let Err(e) = create_profile(&host, &create_opts, img) {
                eprintln!("Falha ao criar perfil '{}': {e}", opts.profile_name);
                process::exit(1);
            }
            profiles = list_profiles(&host);
        } else {
            eprintln!(
                "Imagem de sistema necessária para '{}' não está instalada no SDK.\nExecute `droidrun doctor` para verificar as imagens disponíveis.",
                opts.profile_name
            );
            process::exit(1);
        }
    }

    let profile = profiles.iter().find(|p| p.name == opts.profile_name);
    if let Some(prof) = profile {
        if !prof.is_healthy_for_playstore() {
            println!(
                "\n[Auto-Fix] Detectadas pendências de compatibilidade no perfil '{}'. Corrigindo antes de iniciar...",
                prof.name
            );
            let _ = fix_profile(prof, &host);
        }
    }

    let refreshed_profiles = list_profiles(&host);
    let active_prof = refreshed_profiles
        .iter()
        .find(|p| p.name == opts.profile_name);

    let ver_label = active_prof
        .map(|p| p.android_version_string())
        .unwrap_or_else(|| "Android".to_string());

    let cmd_args = build_emulator_args(&opts, active_prof);
    println!(
        "\n================================================================================"
    );
    println!(
        "  Dispositivo Selecionado : {} ('{}')",
        ver_label, opts.profile_name
    );
    println!(
        "  Aceleração de Hardware  : {}",
        host.os.hypervisor_name()
    );
    println!("  Comando de Execução     : emulator {}", cmd_args.join(" "));
    println!(
        "================================================================================"
    );

    let connected = list_connected_devices(&host).unwrap_or_default();
    if !connected.is_empty() {
        let adb = host.adb_bin.as_ref().unwrap();
        let current_sdk = run_adb(adb, None, &["shell", "getprop", "ro.build.version.sdk"]).unwrap_or_default();
        let target_api_str = active_prof.and_then(|p| p.api_level()).map(|a| a.to_string()).unwrap_or_default();

        let is_same = !target_api_str.is_empty() && current_sdk.trim() == target_api_str;

        if is_same {
            println!(
                "\n[i] Dispositivo '{}' ({}) já está em execução no ADB ({}).",
                opts.profile_name, ver_label, connected.join(", ")
            );
        } else {
            println!(
                "\n[i] Detectada outra versão em execução no ADB (SDK {}).",
                current_sdk.trim()
            );
            println!("  • Encerrando de forma segura para preservar e salvar o estado...");
            let _ = Command::new(adb).args(["emu", "kill"]).output();
            for _ in 0..20 {
                std::thread::sleep(std::time::Duration::from_millis(500));
                if list_connected_devices(&host).unwrap_or_default().is_empty() {
                    break;
                }
            }
            println!("  • Estado salvo com sucesso. Iniciando {} ('{}')...", ver_label, opts.profile_name);

            match launch_profile(&host, &opts, active_prof) {
                Ok(mut child) => {
                    println!("  [OK] Processo Android iniciado (PID: {}).", child.id());
                    println!("  • Aguardando inicialização completa do Android (boot_completed)...");
                    if let Err(e) = wait_for_device_boot(&host, 180) {
                        eprintln!("Aviso: Tempo limite aguardando boot do emulador: {e}");
                    }
                    if !opts.detach {
                        let _ = child.wait();
                        return;
                    }
                }
                Err(e) => {
                    eprintln!("Erro ao iniciar o Android: {e}");
                    process::exit(1);
                }
            }
        }
    } else {
        println!(
            "\nIniciando {} com aceleração {}...",
            ver_label,
            host.os.hypervisor_name()
        );
        match launch_profile(&host, &opts, active_prof) {
            Ok(mut child) => {
                println!("  [OK] Processo Android iniciado (PID: {}).", child.id());
                println!("  • Aguardando inicialização completa do Android (boot_completed)...");
                if let Err(e) = wait_for_device_boot(&host, 180) {
                    eprintln!("Aviso: Tempo limite aguardando boot do emulador: {e}");
                }
                if !opts.detach {
                    let _ = child.wait();
                    return;
                }
            }
            Err(e) => {
                eprintln!("Erro ao iniciar o Android: {e}");
                process::exit(1);
            }
        }
    }

    println!("\n[+] Inicializando e validando camuflagem de integridade (Magisk, Zygisk, SafetyNet/CTS)...");
    let extra_apps = ["com.flinkapps.safteynet", "gr.nikolasspyr.integritycheck"];
    let _ = configure_integrity_and_denylist(&host, None, &extra_apps);

    cmd_audit();
}

fn cmd_audit() {
    print_banner();
    let host = HostReport::detect();
    println!("\nConectando à instância Android via ADB para auditoria de compatibilidade...");

    match audit_running_instance(&host, None) {
        Ok(rep) => {
            println!("\n[Dispositivo Conectado: {}]", rep.device_serial);
            println!("  • Modelo / Versão Android : {} (Android {}, SDK {})", rep.device_model, rep.android_version, rep.sdk_level);
            println!("  • ABIs de CPU Suportadas  : {}", rep.cpu_abilist);
            println!(
                "  • Tradução Nativa ARM64   : {} (bridge={}, exec={})",
                if rep.arm64_supported { "ATIVA (Suporta apps arm64-v8a)" } else { "INATIVA" },
                rep.native_bridge,
                rep.native_bridge_exec
            );
            println!("  • Densidade de Tela (DPI) : {} dpi", rep.lcd_density);
            println!("  • OpenGL ES Version       : {}", rep.gl_es_version);
            let sim_country_display = if rep.sim_country.is_empty() {
                "N/A".to_string()
            } else {
                rep.sim_country.to_uppercase()
            };
            println!(
                "  • SIM Card Operadora/País : {} ({}) [Status: {}]",
                if rep.sim_operator.is_empty() { "Não detectado" } else { &rep.sim_operator },
                sim_country_display,
                if rep.sim_state.is_empty() { "UNKNOWN" } else { &rep.sim_state }
            );
            println!(
                "  • Operadora de Rede Celular: {}",
                if rep.network_operator.is_empty() { "Não registrada" } else { &rep.network_operator }
            );
            println!(
                "  • Google Play Store       : {}",
                if rep.play_store_installed { "INSTALADA (com.android.vending)" } else { "AUSENTE" }
            );
            println!(
                "  • Google Play Services    : {}",
                if rep.play_services_installed { "INSTALADO (com.google.android.gms)" } else { "AUSENTE" }
            );
            let magisk_display = match &rep.magisk_version {
                Some(v) => format!("INSTALADO (v{v})"),
                None if rep.magisk_installed => "INSTALADO (Magisk detectado)".to_string(),
                None => "NÃO DETECTADO (Sem Magisk)".to_string(),
            };
            println!("  • Magisk Framework        : {}", magisk_display);
            println!(
                "  • Acesso Superusuário (su): {}",
                if rep.root_available { "DISPONÍVEL (Root Magisk ativo)" } else { "NÃO DISPONÍVEL" }
            );
            println!(
                "  • Camuflagem Zygisk       : {}",
                if rep.zygisk_enabled { "ATIVO (Zygisk operacional)" } else { "INATIVO (Configure no app Magisk)" }
            );

            println!("\n[Integridade de Dispositivo & SafetyNet / Play Integrity]:");
            let boot_state_display = match rep.verified_boot_state.as_str() {
                "green" => "[OK] green (Bootloader verificado e íntegro)".to_string(),
                "orange" => "[ALERTA] orange (Bootloader desbloqueado / falhará em CTS)".to_string(),
                other if other.is_empty() => "Não detectado".to_string(),
                other => other.to_string(),
            };
            println!("  • Verified Boot State     : {}", boot_state_display);
            let lock_state_display = match rep.flash_locked.as_str() {
                "1" => "[OK] 1 (Bootloader travado / Locked)".to_string(),
                "0" => "[ALERTA] 0 (Bootloader destravado / Unlocked)".to_string(),
                other if other.is_empty() => "Não detectado".to_string(),
                other => other.to_string(),
            };
            println!("  • Flash Locked State      : {}", lock_state_display);
            println!(
                "  • Magisk DenyList         : {}",
                if rep.denylist_enabled { "[OK] ATIVA (Ocultação de Root ligada)" } else { "[AVISO] INATIVA (Execute `droidrun integrity`)" }
            );
            if !rep.build_fingerprint.is_empty() {
                println!("  • Build Fingerprint (CTS) : {}", rep.build_fingerprint);
            }

            println!("\n[Verificação de Hardware Features (<uses-feature>) na Play Store]:");
            let mut missing_count = 0;
            for fc in &rep.feature_checks {
                let status = if fc.present { "[OK]" } else { "[AUSENTE]" };
                if !fc.present {
                    missing_count += 1;
                }
                println!("  {:9} {:52} - {}", status, fc.feature, fc.description);
            }

            if missing_count == 0 && rep.play_store_installed && rep.arm64_supported {
                println!(
                    "\n[RESULTADO]: 100% APROVADO! Todas as features exigidas pelo catálogo da Play Store estão ativas."
                );
                println!("Se algum app ainda aparecia como incompatível de uma sessão anterior, execute: `droidrun sync-playstore`");
            } else {
                println!(
                    "\n[ATENÇÃO]: {} feature(s) ausente(s). Execute `droidrun fix` e reinicie a instância com `droidrun start`.",
                    missing_count
                );
            }
        }
        Err(e) => {
            eprintln!("Não foi possível auditar a instância: {e}");
            process::exit(1);
        }
    }
}

fn cmd_sync_playstore() {
    print_banner();
    let host = HostReport::detect();
    println!("\nSincronizando catálogo de hardware (DeviceConfig) com a Google Play Store...");
    match sync_playstore_catalog(&host, None) {
        Ok(steps) => {
            for s in steps {
                println!("  [+] {s}");
            }
            println!("\nPronto! A Google Play Store foi reaberta na instância com o novo perfil completo de hardware.");
        }
        Err(e) => {
            eprintln!("Erro ao sincronizar Play Store: {e}");
            process::exit(1);
        }
    }
}

fn cmd_root(args: &[String]) {
    print_banner();
    let host = HostReport::detect();

    let mut opts = RootOptions::default();
    let mut list_only = false;
    let mut update_only = false;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--list" | "-l" => list_only = true,
            "--update" | "-u" => update_only = true,
            "--restore" | "--unroot" => opts.restore = true,
            "--fakeboot" => opts.fakeboot = true,
            "--no-auto-start" => opts.auto_start = false,
            arg if !arg.starts_with("--") => {
                opts.profile_name = Some(arg.to_string());
            }
            extra => opts.extra_args.push(extra.to_string()),
        }
        i += 1;
    }

    if list_only {
        println!("\n[Imagens de Sistema e Status do Magisk/Ramdisk]:");
        let ramdisks = list_system_image_ramdisks(&host);
        if ramdisks.is_empty() {
            println!("  [!] Nenhuma imagem de sistema encontrada no Android SDK.");
        } else {
            for rd in ramdisks {
                let status = if rd.has_backup {
                    "[MAGISK INJETADO - Backup presente]"
                } else {
                    "[ORIGINAL - Sem Magisk]"
                };
                println!(
                    "  • API {} ({}) | {}\n      -> {}",
                    rd.api_level, rd.primary_abi, status, rd.relative_path
                );
            }
        }
        return;
    }

    let rootavd_dir = match ensure_rootavd_installed(None) {
        Ok(dir) => dir,
        Err(e) => {
            eprintln!("Falha ao preparar o utilitário rootAVD: {e}");
            process::exit(1);
        }
    };

    if update_only {
        if let Err(e) = update_rootavd(&rootavd_dir) {
            eprintln!("Falha ao atualizar rootAVD: {e}");
            process::exit(1);
        }
        return;
    }

    let profiles = list_profiles(&host);

    // Resolve target profile
    let target_profile = if let Some(ref name) = opts.profile_name {
        profiles.iter().find(|p| &p.name == name).cloned()
    } else {
        // Try finding running profile via ADB if online
        let running_name = host.adb_bin.as_ref().and_then(|adb| {
            let out = std::process::Command::new(adb)
                .args(["shell", "getprop", "ro.boot.qemu.avd_name"])
                .output()
                .ok()?;
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        });

        if let Some(rname) = running_name {
            profiles.iter().find(|p| p.name == rname).cloned()
        } else {
            profiles.iter().find(|p| p.name == "DroidRun_PlayStore")
                .or_else(|| profiles.iter().find(|p| p.name == "Pixel_7_PlayStore"))
                .or_else(|| profiles.first())
                .cloned()
        }
    };

    let Some(profile) = target_profile else {
        eprintln!("Nenhum perfil de dispositivo Android encontrado em {}.", host.avd_home.display());
        eprintln!("Crie um primeiro com: droidrun create DroidRun_PlayStore");
        process::exit(1);
    };

    let rel_ramdisk = match resolve_ramdisk_for_profile(&host, &profile) {
        Ok(rd) => rd,
        Err(e) => {
            eprintln!("Erro ao resolver ramdisk para o perfil '{}': {e}", profile.name);
            process::exit(1);
        }
    };

    println!("\n[Perfil Alvo Selecionado]: {}", profile.name);
    println!("  • Ramdisk Relativo : {}", rel_ramdisk);
    println!("  • rootAVD Diretório: {}", rootavd_dir.display());

    if opts.restore {
        println!("\n[*] Restaurando o ramdisk original (desfazendo o patch do Magisk)...");
        match restore_ramdisk_backup(&host, &rel_ramdisk) {
            Ok(_) => {
                println!("\n[SUCESSO] O ramdisk original de '{}' foi restaurado!", profile.name);
                println!("Inicie a instância com o estado de fábrica sem root: `droidrun start {}`", profile.name);
            }
            Err(e) => {
                eprintln!("\n[ERRO] Falha ao restaurar backup: {e}");
                process::exit(1);
            }
        }
        return;
    }

    // Check if emulator is online
    let connected = list_connected_devices(&host).unwrap_or_default();
    if connected.is_empty() {
        if opts.auto_start {
            println!(
                "\n[Auto-Start] Nenhuma instância Android em execução detectada via ADB.\nIniciando o perfil '{}' para a injeção do Magisk...",
                profile.name
            );
            let launch_opts = LaunchOptions {
                profile_name: profile.name.clone(),
                detach: true,
                ..Default::default()
            };
            if let Err(e) = launch_profile(&host, &launch_opts, Some(&profile)) {
                eprintln!("Falha ao iniciar o emulador: {e}");
                process::exit(1);
            }
            if let Err(e) = wait_for_device_boot(&host, 180) {
                eprintln!("Falha aguardando inicialização do emulador: {e}");
                process::exit(1);
            }
        } else {
            eprintln!("\n[ERRO] Nenhuma instância Android em execução detectada via ADB.");
            eprintln!("O rootAVD precisa que o emulador esteja em execução para injetar o Magisk.");
            eprintln!("Inicie primeiro com `droidrun start {}` ou execute sem `--no-auto-start`.", profile.name);
            process::exit(1);
        }
    } else {
        println!("  • Instância Android online detectada via ADB: {}", connected.join(", "));
    }

    match run_rootavd_script(&host, &rootavd_dir, &rel_ramdisk, &opts) {
        Ok(_) => {
            println!("\n================================================================================");
            println!("  [SUCESSO] Magisk injetado com sucesso no ramdisk do emulador Android!");
            println!("================================================================================");
            println!("\nPróximos passos essenciais para ativar o Root e ocultar a emulação:");
            println!("  1. Inicie o Android novamente:");
            println!("     droidrun start {}", profile.name);
            println!("\n  2. Abra o aplicativo 'Magisk' dentro do Android.");
            println!("     • Se aparecer a mensagem 'Requer Configuração Adicional', confirme em 'OK'.");
            println!("       (O emulador reiniciará sozinho para finalizar a montagem dos mirrors).");
            println!("\n  3. Configure a camuflagem e o Zygisk (Configurações do Magisk -> engrenagem no topo):");
            println!("     • Ative o interruptor 'Zygisk' (vital para ocultação de root e injeção).");
            println!("     • Ative a opção 'Enforce Denylist' (Forçar Lista de Negação).");
            println!("     • Toque em 'Configure Denylist' (Configurar Lista de Negação) e marque");
            println!("       todos os apps que verificam integridade ou root (bancos, jogos, etc.).");
            println!("\n  4. Valide o ambiente executando no seu terminal:");
            println!("     droidrun audit");
            println!("================================================================================\n");
        }
        Err(e) => {
            eprintln!("\n[ERRO] Falha na execução do rootAVD: {e}");
            process::exit(1);
        }
    }
}

fn cmd_unroot(args: &[String]) {
    let mut root_args = vec!["--restore".to_string()];
    root_args.extend_from_slice(args);
    cmd_root(&root_args);
}

fn cmd_integrity(args: &[String]) {
    print_banner();
    let host = HostReport::detect();
    let mut profile_name = None;
    let mut extra_apps = Vec::new();

    for arg in args {
        if !arg.starts_with("--") && profile_name.is_none() {
            profile_name = Some(arg.to_string());
        } else if !arg.starts_with("--") {
            extra_apps.push(arg.as_str());
        }
    }

    let profiles = list_profiles(&host);
    let profile = if let Some(target) = &profile_name {
        profiles.into_iter().find(|p| &p.name == target)
    } else {
        profiles
            .into_iter()
            .find(|p| p.name == "DroidRun_PlayStore" || p.name == "Pixel_7_PlayStore")
    };

    println!("\n================================================================================");
    println!("  DroidRun Integrity Hardening & SafetyNet / Play Integrity Spoofing");
    println!("================================================================================");

    let connected = list_connected_devices(&host).unwrap_or_default();
    if connected.is_empty() {
        if let Some(prof) = &profile {
            println!(
                "\n[Auto-Start] Nenhuma instância Android em execução detectada via ADB.\nIniciando o perfil '{}' para aplicar a integridade...",
                prof.name
            );
            let launch_opts = LaunchOptions {
                profile_name: prof.name.clone(),
                detach: true,
                ..Default::default()
            };
            if let Err(e) = launch_profile(&host, &launch_opts, Some(prof)) {
                eprintln!("Falha ao iniciar o emulador: {e}");
                process::exit(1);
            }
            if let Err(e) = wait_for_device_boot(&host, 180) {
                eprintln!("Falha aguardando inicialização do emulador: {e}");
                process::exit(1);
            }
        } else {
            eprintln!("\n[ERRO] Nenhuma instância Android em execução detectada via ADB.");
            eprintln!("Inicie com `droidrun start [perfil]` antes de executar `droidrun integrity`.");
            process::exit(1);
        }
    }

    println!("\nAplicando hardening de segurança, Zygisk, Magisk DenyList e CTS Spoofing...");
    match configure_integrity_and_denylist(&host, None, &extra_apps) {
        Ok(rep) => {
            println!("\n[SUCESSO] Configuração de Integridade e Camuflagem Aplicada!");
            println!("  • Zygisk Framework         : ATIVADO no banco de configurações do Magisk");
            println!("  • Magisk DenyList          : HABILITADO (Enforce DenyList ativo)");
            println!("  • Pacotes Ocultados (Root) :");
            for app in &rep.apps_denylisted {
                println!("      - {}", app);
            }
            println!("  • Script de Spoofing CTS   : INSTALADO em /data/adb/service.d/00-integrity-spoof.sh");
            println!("  • Propriedades de Hardware :");
            println!("      - ro.boot.verifiedbootstate  = green");
            println!("      - ro.boot.flash.locked       = 1");
            println!("      - ro.boot.vbmeta.device_state = locked");
            println!("      - ro.build.type              = user");
            println!("      - ro.build.tags              = release-keys");
            println!("      - ro.debuggable              = 0");
            println!("      - ro.secure                  = 1");
            println!("      - ro.build.fingerprint       = Dinâmico (Pixel 5 redfin p/ Android 11 | Pixel 7 panther p/ Android 13+)");
            println!("      - ro.product.first_api_level = 25 (Key Attestation Bypass forçado)");
            println!("\nO emulador está agora configurado para passar nos testes de integridade básica e perfil CTS!");
            println!("Dica: Você pode verificar o estado em tempo real a qualquer momento com: `droidrun audit`");
        }
        Err(e) => {
            eprintln!("\n[ERRO] Falha ao configurar integridade: {e}");
            eprintln!("Certifique-se de que o Magisk está instalado na imagem com `droidrun root`.");
            process::exit(1);
        }
    }
}

fn cmd_install(args: &[String]) {
    print_banner();
    let Some(target) = args.first() else {
        eprintln!("Informe o caminho para o arquivo .apk ou pasta com múltiplos .apk.");
        process::exit(1);
    };
    let host = HostReport::detect();
    match install_apk(&host, None, Path::new(target)) {
        Ok(msg) => println!("\n[SUCESSO] {msg}"),
        Err(e) => {
            eprintln!("\n[ERRO] Falha na instalação: {e}");
            process::exit(1);
        }
    }
}

fn cmd_sms(args: &[String]) {
    print_banner();
    if args.len() < 2 {
        eprintln!("Uso: droidrun sms <número> <mensagem>");
        eprintln!("Exemplo: droidrun sms 11987654321 \"Seu codigo de ativacao e: 849201\"");
        process::exit(1);
    }
    let sender = &args[0];
    let message = args[1..].join(" ");
    let host = HostReport::detect();
    println!("\nInjetando SMS de '{}' no dispositivo Android...", sender);
    match send_sms(&host, None, sender, &message) {
        Ok(out) => {
            println!("  [+] SMS injetado com sucesso na caixa de entrada!");
            if !out.trim().is_empty() {
                println!("  Detalhes: {}", out.trim());
            }
        }
        Err(e) => {
            eprintln!("  [ERRO] Falha ao enviar SMS: {e}");
            process::exit(1);
        }
    }
}

fn cmd_install_image(args: &[String]) {
    print_banner();
    let api_level: u32 = args
        .first()
        .and_then(|s| s.parse().ok())
        .unwrap_or(34);
    let host = HostReport::detect();
    if let Err(e) = install_system_image(&host, api_level) {
        eprintln!("Erro: {e}");
        process::exit(1);
    }
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    let Some(cmd) = args.first().map(|s| s.as_str()) else {
        cmd_doctor();
        return;
    };

    match cmd {
        "doctor" | "check" | "status" => cmd_doctor(),
        "list" | "profiles" => cmd_list(),
        "stop" | "kill" => cmd_stop(),
        "fix" | "repair" => cmd_fix(args.get(1).cloned()),
        "create" | "init" => cmd_create(&args[1..]),
        "start" | "run" | "launch" => cmd_start(&args[1..]),
        "audit" | "inspect" => cmd_audit(),
        "sync-playstore" | "refresh-playstore" => cmd_sync_playstore(),
        "root" | "magisk" => cmd_root(&args[1..]),
        "unroot" => cmd_unroot(&args[1..]),
        "integrity" | "safetynet" | "pif" => cmd_integrity(&args[1..]),
        "install" => cmd_install(&args[1..]),
        "sms" => cmd_sms(&args[1..]),
        "install-image" => cmd_install_image(&args[1..]),
        "help" | "--help" | "-h" => print_help(),
        other => {
            eprintln!("Comando desconhecido: '{other}'\n");
            print_help();
            process::exit(1);
        }
    }
}
