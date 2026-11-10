use droidrun_core::audit::{audit_running_instance, install_apk, send_sms, sync_playstore_catalog};
use droidrun_core::platform::HostReport;
use droidrun_core::profile::{
    create_profile, fix_profile, list_profiles, ProfileConfigOptions,
};
use droidrun_core::runner::{build_emulator_args, launch_profile, LaunchOptions};
use droidrun_core::sdk::{install_system_image, recommended_playstore_packages};
use std::env;
use std::path::Path;
use std::process;

fn print_banner() {
    println!("================================================================================");
    println!("  DroidRun v0.1.0 - Open-Source Full Android Desktop Runner (Linux/macOS/Win)");
    println!("  Google Play Store Oficial + Tradução ARM64 (libndk_translation) + GPU Host");
    println!("================================================================================");
}

fn print_help() {
    print_banner();
    println!("\nUSO:");
    println!("  droidrun <COMANDO> [OPÇÕES]\n");
    println!("COMANDOS:");
    println!("  doctor                    Diagnostica SO, KVM/HVF/WHPX, imagens ARM64 e perfis existentes");
    println!("  fix [nome_perfil]         Corrige perfis existentes para habilitar Play Store, Câmera Traseira,");
    println!("                            Tradução ARM64, Sensores, VirtioWifi e Aceleração GPU Host");
    println!("  create [nome_perfil]      Cria um novo dispositivo Android legítimo pronto para a Play Store");
    println!("                            Opções: --ram <MB> --cores <N> --storage <GB>");
    println!("  start [nome_perfil]       Repara automaticamente e inicia o Android com aceleração máxima");
    println!("                            Opções: --foreground --wipe-data --warm");
    println!("  audit                     Audita em tempo real via ADB as features da Play Store e ABI ARM64");
    println!("  sync-playstore            Limpa o cache de DeviceConfig da Play Store para liberar apps ocultos");
    println!("  install <arquivo_ou_dir>  Instala um APK ou pacote Split APK (arm64-v8a/x86_64) via ADB");
    println!("  sms <número> <mensagem>   Injeta um SMS diretamente na caixa de entrada do Android (OTP/2FA/Banco)");
    println!("  install-image [api]       Baixa imagem oficial Google Play via sdkmanager (ex: 34, 33, 30)");
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
            let arm_badge = if img.primary_abi == "arm64-v8a" {
                "ARM64 Nativo"
            } else if img.has_arm64_translation {
                "Tradução ARM64 Ativa (libndk_translation)"
            } else {
                "Apenas x86_64"
            };
            let hw_accel = if img.primary_abi == host.arch
                || (host.arch == "aarch64" && img.primary_abi == "arm64-v8a")
            {
                "Aceleração Hardware KVM/HVF/WHPX"
            } else {
                "LENTO (Emulação por software - arquitetura diferente do host)"
            };
            println!(
                "  • Android {} (API {}) {} | ABI Primaria: {} | ABIs: {:?}\n      -> {} | {}",
                img.android_release,
                img.api_level,
                play_badge,
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
            if prof.is_healthy_for_playstore() {
                println!(
                    "  [OK] Perfil '{}' está 100% otimizado para Google Play Store e ARM64!",
                    prof.name
                );
            } else {
                println!(
                    "  [PROBLEMAS DETECTADOS] Perfil '{}' possui {} configuração(ões) que limitam a Play Store:",
                    prof.name,
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
    let Some(best_img) = host.best_playstore_image() else {
        eprintln!("Nenhuma imagem de sistema Android instalada no SDK. Execute `droidrun install-image 34` primeiro.");
        process::exit(1);
    };

    let mut opts = ProfileConfigOptions::default();
    opts.cpu_cores = ((host.cpu_cores / 2).clamp(4, 8)) as u32;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
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

    println!(
        "\nCriando dispositivo Android legítimo '{}' usando imagem:\n  -> {} (Android {}, API {}, ABIs: {:?})",
        opts.name,
        best_img.relative_sysdir,
        best_img.android_release,
        best_img.api_level,
        best_img.supported_abis
    );

    match create_profile(&host, &opts, best_img) {
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

fn cmd_start(args: &[String]) {
    print_banner();
    let host = HostReport::detect();
    let mut profiles = list_profiles(&host);

    let mut opts = LaunchOptions::default();
    // Pick DroidRun_PlayStore if it exists, otherwise Pixel_7_PlayStore, otherwise first profile
    if profiles.iter().any(|p| p.name == "DroidRun_PlayStore") {
        opts.profile_name = "DroidRun_PlayStore".to_string();
    } else if profiles.iter().any(|p| p.name == "Pixel_7_PlayStore") {
        opts.profile_name = "Pixel_7_PlayStore".to_string();
    } else if let Some(first) = profiles.first() {
        opts.profile_name = first.name.clone();
    }

    for arg in args {
        match arg.as_str() {
            "--foreground" => opts.detach = false,
            "--wipe-data" => opts.wipe_data = true,
            "--warm" => opts.cold_boot = false,
            name if !name.starts_with("--") => opts.profile_name = name.to_string(),
            _ => {}
        }
    }

    // If profile doesn't exist yet, create it automatically!
    if !profiles.iter().any(|p| p.name == opts.profile_name) {
        if let Some(best_img) = host.best_playstore_image() {
            println!(
                "Perfil '{}' ainda não existe. Criando automaticamente com suporte total à Play Store...",
                opts.profile_name
            );
            let create_opts = ProfileConfigOptions {
                name: opts.profile_name.clone(),
                ..Default::default()
            };
            if let Err(e) = create_profile(&host, &create_opts, best_img) {
                eprintln!("Falha ao criar perfil '{}': {e}", opts.profile_name);
                process::exit(1);
            }
            profiles = list_profiles(&host);
        }
    }

    let profile = profiles.iter().find(|p| p.name == opts.profile_name);
    if let Some(prof) = profile {
        if !prof.is_healthy_for_playstore() {
            println!(
                "\n[Auto-Fix] Detectadas {} pendências de compatibilidade da Play Store no perfil '{}'. Corrigindo antes de iniciar...",
                prof.issues.len(),
                prof.name
            );
            let _ = fix_profile(prof, &host);
        }
    }

    let refreshed_profiles = list_profiles(&host);
    let active_prof = refreshed_profiles
        .iter()
        .find(|p| p.name == opts.profile_name);

    let cmd_args = build_emulator_args(&opts, active_prof);
    println!(
        "\nIniciando Android '{}' com aceleração {}...",
        opts.profile_name,
        host.os.hypervisor_name()
    );
    println!("  Argumentos: emulator {}", cmd_args.join(" "));

    match launch_profile(&host, &opts, active_prof) {
        Ok(mut child) => {
            println!("  [OK] Processo Android iniciado (PID: {}).", child.id());
            println!("  • Após o boot completo, você pode executar:");
            println!("      droidrun audit           (para verificar todas as features e ABIs)");
            println!("      droidrun sync-playstore  (para atualizar o catálogo da Play Store)");
            if !opts.detach {
                let _ = child.wait();
            }
        }
        Err(e) => {
            eprintln!("Erro ao iniciar o Android: {e}");
            process::exit(1);
        }
    }
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
        "fix" | "repair" => cmd_fix(args.get(1).cloned()),
        "create" | "init" => cmd_create(&args[1..]),
        "start" | "run" | "launch" => cmd_start(&args[1..]),
        "audit" | "inspect" => cmd_audit(),
        "sync-playstore" | "refresh-playstore" => cmd_sync_playstore(),
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
