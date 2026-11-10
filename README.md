# DroidRun (`Android-real-emulator`)

Ferramenta open-source multiplataforma escrita em **Rust** para executar o **Android Oficial Completo com Google Play Store**, tradução binária **ARM64 (`libndk_translation` / `berberis`)**, aceleração gráfica nativa (**Vulkan 1.1+ / OpenGL ES 3.1+**) e suporte completo a `<uses-feature>` em **Linux, macOS e Windows**.

---

## Por que emuladores comuns e ISOs falham na Play Store?

Quando você tenta baixar aplicativos na Google Play Store em emuladores mal configurados ou sistemas x86 como BlissOS, a Play Store exibe *"Este app não é compatível com o seu dispositivo"* ou o aplicativo fecha ao abrir devido a **4 causas técnicas principais**:

1. **`PlayStore.enabled=no` no perfil de hardware**: Desativa o perfil comercial certificado do Google Play Services.
2. **`hw.camera.back=none` (Ausência da feature `android.hardware.camera`)**: No Android, a permissão `<uses-feature android:name="android.hardware.camera"/>` exige especificamente uma **câmera traseira**. Quando ela está definida como `none`, a Play Store oculta automaticamente milhares de apps (bancos, carteiras digitais, entregas, redes sociais, leitores de QR Code).
3. **Arquitetura de CPU (`x86_64` vs `arm64-v8a`)**: A maioria dos aplicativos Android possui bibliotecas C/C++ (`.so`) compiladas apenas para `arm64-v8a`. Sem a ponte de tradução nativa (`ro.dalvik.vm.native.bridge=libndk_translation.so`) aliada ao hypervisor nativo (`KVM` no Linux, `HVF` no macOS, `WHPX` no Windows), os aplicativos não instalam ou ficam inutilizáveis de tão lentos.
4. **Falta de Wi-Fi Virtual (`VirtioWifi`), Sensores e Vulkan (`-gpu host`)**: Aplicativos que exigem `android.hardware.wifi`, giroscópio, GPS ou Vulkan falham quando esses barramentos virtuais estão desativados.

O **DroidRun** automatiza a detecção, criação, reparo e execução de instâncias Android que resolvem todos esses pontos.

---

## Compatibilidade Multiplataforma

| Sistema Operacional | Hypervisor de Hardware | Arquitetura Host | Suporte ARM64 (`arm64-v8a`) |
| :--- | :--- | :--- | :--- |
| **Linux** | `KVM` (`/dev/kvm`) | `x86_64` / `aarch64` | Via `libndk_translation.so` (em `x86_64`) ou Nativo |
| **macOS** | `Hypervisor.framework (HVF)` | Apple Silicon (`M1-M4`) / Intel | Execução **100% Nativa** em Apple Silicon |
| **Windows** | `WHPX` (Hyper-V) / `AEHD` | `x86_64` | Via `libndk_translation.so` |

---

## Como Compilar e Usar

### 1. Compilar com Cargo (Rust)

```bash
cargo build --release
```

O binário estará disponível em `./target/release/droidrun`.

### 2. Diagnosticar Seu Computador e Perfis Existentes

```bash
./target/release/droidrun doctor
```

### 3. Corrigir Perfis Existentes (Habilitar Play Store, Câmeras, Sensores, GPU Host e ARM64)

```bash
./target/release/droidrun fix
```

### 4. Criar um Novo Dispositivo Android Legítimo Pronto para a Play Store

```bash
./target/release/droidrun create DroidRun_PlayStore --ram 6144 --cores 6 --storage 16
```

### 5. Iniciar o Android com Aceleração Máxima

```bash
./target/release/droidrun start DroidRun_PlayStore
```

### 6. Auditar uma Instância em Execução e Sincronizar o Catálogo da Play Store

Com o Android aberto:

```bash
# Verifica via ADB todas as 16+ features de hardware, Play Store e tradução ARM64:
./target/release/droidrun audit

# Limpa o cache antigo de DeviceConfig da Play Store para liberar todos os apps ocultos:
./target/release/droidrun sync-playstore
```

### 7. Instalar APKs ou Pacotes Split APK (`.apk` / pasta de splits `arm64-v8a`)

```bash
./target/release/droidrun install caminho/para/app.apk
```
