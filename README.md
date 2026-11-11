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

### 5. Iniciar e Alternar Entre Versões do Android Facilmente

Você pode listar, iniciar qualquer versão específica do Android ou encerrar instâncias com comandos simples:

```bash
# Listar todas as versões instaladas, APIs e status de Magisk Root:
droidrun list

# Iniciar Android 7.1.1 (API 25 - Nougat) sem restrições de Play Integrity modernas:
droidrun start --version 7.1
# ou de forma abreviada:
droidrun start 7.1

# Iniciar Android 11.0 (API 30 - Red Velvet Cake) com Magisk Root ativo:
droidrun start --version 11
# ou de forma abreviada:
droidrun start 11

# Iniciar Android 15.0 (API 35):
droidrun start --version 15
# ou de forma abreviada:
droidrun start 15

# Encerrar de forma segura e salvar o estado de todas as instâncias em execução:
droidrun stop
```

> **Troca Automática e Segura de Versão**: Ao iniciar uma versão diferente enquanto outra estiver aberta, o DroidRun detecta a versão ativa no ADB, envia `adb emu kill` para salvar todos os dados de disco e snapshots em segurança, aguarda o encerramento do processo e inicia a versão solicitada automaticamente sem corrupção de dados.

### 6. Auditar uma Instância em Execução e Sincronizar o Catálogo da Play Store

Com o Android aberto:

```bash
# Verifica via ADB todas as 16+ features de hardware, Play Store e tradução ARM64:
./target/release/droidrun audit

# Limpa o cache antigo de DeviceConfig da Play Store para liberar todos os apps ocultos:
./target/release/droidrun sync-playstore
```

### 7. Injetar Suporte Nativo ao Magisk Root (rootAVD Integrado)

Para testes avançados de segurança, pentest, engenharia reversa com Frida ou uso de módulos Zygisk:

```bash
# Injeta o Magisk nativamente no ramdisk do perfil (inicia o emulador e aplica o patch automaticamente):
./target/release/droidrun root DroidRun_PlayStore

# Ou para listar o status de Magisk de todas as imagens do SDK:
./target/release/droidrun root --list

# Para desfazer o root e restaurar a imagem original de fábrica:
./target/release/droidrun unroot DroidRun_PlayStore
```

#### Passo Essencial: Ativando Zygisk e Ocultando a Emulação (Configure Denylist)

Após a injeção do Magisk pelo DroidRun:
1. Abra o emulador com `./target/release/droidrun start DroidRun_PlayStore`.
2. Abra o aplicativo **Magisk** instalado na gaveta de apps.
3. Se o app solicitar *"Requer Configuração Adicional"*, confirme em **OK** (o emulador reiniciará sozinho).
4. Vá em **Configurações** do Magisk (ícone de engrenagem no canto superior direito):
   - Ative a opção **Zygisk** (essencial para injeção de processos e camuflagem).
   - Ative a opção **Enforce Denylist** (Forçar Lista de Negação).
   - Toque em **Configure Denylist** (Configurar Lista de Negação) e marque todos os aplicativos bancários, jogos ou apps que verificam integridade/emulação.
5. Ou aplique a configuração completa de integridade automaticamente via DroidRun:
   ```bash
   ./target/release/droidrun integrity DroidRun_PlayStore
   ```
   *(Habilita Zygisk, ativa DenyList para Google Play Services / apps bancários e faz o spoof da fingerprint CTS do Google Pixel 7 no boot)*.
6. Verifique a ativação completa do ambiente com:
   ```bash
   ./target/release/droidrun audit
   ```

---

### 8. Instalar APKs ou Pacotes Split APK (`.apk` / pasta de splits `arm64-v8a`)

```bash
./target/release/droidrun install caminho/para/app.apk
```

---

## Comparativo de Métodos de Magisk em Emuladores Android

| Método | Estabilidade | Detecção por Apps | Compatibilidade Linux | Aceleração por Hardware |
| :--- | :--- | :--- | :--- | :--- |
| **Método 1: BlueStacks + BlueStacks-Root-GUI** | Média | Facilmente detectado por assinaturas proprietárias do BlueStacks | Não (apenas Windows/Wine instável) | Proprietária Windows |
| **Método 2: NoxPlayer / LDPlayer (Modo Root)** | Baixa | Alta taxa de detecção (SuperSU/phh antigos deixam rastros no `/system`) | Não (apenas Windows) | Proprietária Windows |
| **Método 3: DroidRun + rootAVD (Nativo Oficial)** | **Máxima** | **Indetectável com Zygisk + Denylist** (Kernel oficial Google Play) | **100% Nativo (KVM)** | **Máxima (KVM / HVF / WHPX)** |
