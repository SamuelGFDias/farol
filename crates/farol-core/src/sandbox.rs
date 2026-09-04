//! Composição dos argumentos de `bwrap` (bubblewrap) que envolvem o spawn de
//! todo plugin (feature `006-sandbox-permissoes-bubblewrap`) — aplica de
//! verdade, via sandbox de processo, o isolamento de rede/filesystem/exec
//! que antes desta feature o core só declarava (`CapabilityManifest`) sem
//! nunca impor.
//!
//! Fonte normativa exata da ordem e dos flags:
//! `specs/006-sandbox-permissoes-bubblewrap/contracts/bwrap-invocation-contract.md`.
//! O "porquê" de cada decisão está em `research.md` (D1-D12) — em particular
//! D5 documenta um bug real de ordem já encontrado e corrigido nesta feature,
//! e sua correção de escopo (2026-09-03, achada na verificação da Camada 2/
//! `harness.sh`): **qualquer** bind de caminho real do host (não só
//! `extra_binds`) que esteja aninhado sob `/tmp` some se estiver posicionado
//! antes de `--tmpfs /tmp`, porque o `--tmpfs`, processado depois, monta uma
//! tmpfs vazia por cima e esconde o bind anterior. Prova empírica: o bind do
//! interpretador (`--ro-bind <python3 resolvido> ...`) apontava para um shim
//! de teste do `harness.sh` sob `$(mktemp -d)` (`/tmp/farol-harness-XXXX/
//! bin/python3`) e falhava com `execvp ...: No such file or directory`
//! enquanto posicionado antes de `--tmpfs /tmp`. [`build_bwrap_args`] por
//! isso MUST emitir `--proc`/`--dev`/`--tmpfs /tmp` logo depois das flags de
//! namespace e antes de qualquer bind real (interpretador, DNS/TLS,
//! `/usr/bin` etc., raiz do repo, `extra_binds`).
//!
//! Consumido por `plugin_worker::worker()` (T008), que spawna
//! `Command::new("bwrap")` com os argumentos daqui em vez de
//! `Command::new(&config.command)` diretamente.

use std::path::{Path, PathBuf};

/// Perfil de sandbox de um plugin, resolvido estaticamente por
/// `plugin_worker::known_plugins()` — **nunca** a partir do
/// `CapabilityManifest` que o próprio plugin declara em runtime (D1:
/// confiar no autorrelato do processo sobre o isolamento dele mesmo não
/// seria uma fronteira de segurança real).
///
/// Ver `data-model.md` § `SandboxProfile` para a tabela de campos.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SandboxProfile {
    /// `true` ⟹ `--share-net` + binds read-only de DNS/TLS (D2).
    pub allow_network: bool,
    /// `true` ⟹ bind read-only de `/usr/bin`, `/bin`, `/usr/local/bin` — a
    /// mediação de `exec` é por visibilidade de filesystem, não por seccomp
    /// (D3, débito técnico reconhecido lá).
    pub allow_exec: bool,
    /// Binds adicionais nomeados por plugin (D5/D6) — vazio para a maioria;
    /// sempre aplicados por último na composição de [`build_bwrap_args`].
    pub extra_binds: Vec<BindMount>,
}

/// Um bind adicional específico de plugin (`scan_root` do `git-local`,
/// socket Docker de `docker-containers`, D5/D6).
///
/// `SRC == DEST` sempre — não há campo de remapeamento de caminho
/// (`data-model.md`, nota sobre `sandbox_path` não existir): todo bind desta
/// feature usa o mesmo caminho no host e dentro do sandbox.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct BindMount {
    pub host_path: PathBuf,
    pub writable: bool,
}

/// Default de `scan_root` quando o `config.toml` de `git-local` está
/// ausente, ilegível, malformado ou sem o campo — replica literalmente
/// `DEFAULT_SCAN_ROOT` de `plugins/git-local/config.py` (T016).
const GIT_LOCAL_DEFAULT_SCAN_ROOT: &str = "~/dev";

/// Resolve o `scan_root` de `git-local` do lado do core, para popular o
/// `extra_binds` do `SandboxProfile` desse plugin (`known_plugins()`,
/// `contracts/bwrap-invocation-contract.md` § "`extra_binds` conhecidos").
///
/// Replica EXATAMENTE a mesma lógica de `plugins/git-local/config.py::
/// load_scan_root`: lê `config.toml` no mesmo caminho resolvido por
/// [`crate::config_store::plugin_config_path`] (`$XDG_CONFIG_HOME/farol/
/// plugins/git-local/config.toml`, fallback `~/.config/farol/plugins/
/// git-local/config.toml`), campo `scan_root` (TOML), default `"~/dev"` se
/// o arquivo não existir, o campo estiver ausente, ou o parse falhar por
/// qualquer motivo — **nunca falha**, sempre devolve um `PathBuf` com `~`
/// expandido.
pub fn resolve_git_local_scan_root() -> PathBuf {
    let path = crate::config_store::plugin_config_path("git-local");

    let raw_value = std::fs::read_to_string(&path)
        .ok()
        .and_then(|content| content.parse::<toml::Table>().ok())
        .and_then(|table| table.get("scan_root").and_then(|v| v.as_str().map(str::to_string)))
        .filter(|s| !s.trim().is_empty())
        .unwrap_or_else(|| GIT_LOCAL_DEFAULT_SCAN_ROOT.to_string());

    expand_tilde(&raw_value)
}

/// Expande um `~` inicial para o `$HOME` do processo atual — mesma
/// convenção de `pathlib.Path.expanduser()` do plugin Python original.
/// Sem `$HOME` definida, devolve o caminho literal (com `~`) sem falhar.
fn expand_tilde(raw: &str) -> PathBuf {
    if let Some(rest) = raw.strip_prefix("~/") {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home).join(rest);
        }
    } else if raw == "~" {
        if let Some(home) = std::env::var_os("HOME") {
            return PathBuf::from(home);
        }
    }
    PathBuf::from(raw)
}

/// Resolve o caminho absoluto de `command` (hoje sempre `"python3"`)
/// procurando nos diretórios de `$PATH` do processo atual do Farol (D10) —
/// sem depender de nenhuma crate nova (`which`, etc.), só
/// `std::env::var("PATH")` + `std::fs::metadata` por candidato.
///
/// `bwrap` executa o `COMMAND` final por caminho dentro do namespace de
/// mount já restrito — resolver o caminho absoluto do lado de fora, no
/// host, e bindar exatamente esse arquivo evita ter que reabrir `/usr/bin`
/// inteiro só para permitir a busca de `$PATH` *dentro* do sandbox (D10),
/// o que reintroduziria a mesma questão de granularidade grosseira de D3.
///
/// Devolve `None` se nenhum diretório do `PATH` tiver o binário — mapeado
/// pelo chamador (`plugin_worker::worker`) para o mesmo caminho de erro de
/// `WorkerEvent::SpawnFailed` já usado para outras falhas de spawn (D8).
pub fn resolve_interpreter_path(command: &str) -> Option<PathBuf> {
    let path_var = std::env::var("PATH").ok()?;
    for dir in path_var.split(':') {
        if dir.is_empty() {
            continue;
        }
        let candidate = Path::new(dir).join(command);
        if std::fs::metadata(&candidate).is_ok() {
            return Some(candidate);
        }
    }
    None
}

/// Constrói a lista de argumentos de `bwrap` para spawnar `command`/`args`
/// (o comando/args do plugin real, já resolvidos como caminho absoluto em
/// `interpreter_path` por [`resolve_interpreter_path`]) sob o perfil de
/// isolamento `profile`, com o filesystem do código do plugin ancorado em
/// `repo_root` (bind read-only da raiz do repositório inteira, D4).
///
/// A ordem dos blocos abaixo é normativa
/// (`contracts/bwrap-invocation-contract.md` § "Regra de ordem") e MUST ser
/// preservada exatamente: (1) namespaces; (2) `--proc`/`--dev`/`--tmpfs /tmp`
/// — logo depois das flags de namespace, antes de qualquer bind real, para
/// que nenhum bind subsequente corra risco de estar aninhado sob um desses
/// três caminhos e ser sombreado (D5, correção de escopo 2026-09-03); (3)
/// binds base de lib/interpretador; (4) binds condicionais a
/// `allow_network`; (5) binds condicionais a `allow_exec`; (6) bind da raiz
/// do repo + `--chdir`; (7) `extra_binds`, sempre por último entre os binds
/// reais — a posição relativa aos passos 3-6 não importa, só estar depois
/// do passo 2; (8) `--` + o comando final.
///
/// `command` (o nome/comando original do plugin, ex. `"python3"`) não entra
/// na lista de argumentos produzida — o processo final é sempre invocado
/// pelo caminho absoluto já resolvido em `interpreter_path` (D10). O
/// parâmetro é mantido na assinatura por simetria com `config.command`/
/// `config.args` do chamador (`plugin_worker::worker`, T008) e para
/// eventual uso futuro (ex.: logging), por isso prefixado com `_` — não
/// participa da composição dos argumentos.
pub fn build_bwrap_args(
    repo_root: &Path,
    interpreter_path: &Path,
    profile: &SandboxProfile,
    _command: &str,
    args: &[String],
) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();

    // (1) Namespaces + die-with-parent.
    out.push("--unshare-all".to_string());
    if profile.allow_network {
        out.push("--share-net".to_string());
    }
    out.push("--die-with-parent".to_string());

    // (2) Mounts sintéticos genéricos — logo depois das flags de namespace e
    // antes de QUALQUER bind real (D5, correção de escopo 2026-09-03): um
    // bind real posicionado antes de `--tmpfs /tmp` some se estiver aninhado
    // sob `/tmp`, porque o `--tmpfs`, processado depois, monta uma tmpfs
    // vazia por cima e esconde o bind anterior.
    out.push("--proc".to_string());
    out.push("/proc".to_string());
    out.push("--dev".to_string());
    out.push("/dev".to_string());
    out.push("--tmpfs".to_string());
    out.push("/tmp".to_string());

    // (3) Binds base de biblioteca/interpretador — sempre presentes.
    push_ro_bind_try(&mut out, "/usr/lib", "/usr/lib");
    push_ro_bind_try(&mut out, "/usr/lib64", "/usr/lib64");
    push_ro_bind_try(&mut out, "/lib", "/lib");
    push_ro_bind_try(&mut out, "/lib64", "/lib64");
    push_ro_bind_try(&mut out, "/etc/ld.so.cache", "/etc/ld.so.cache");
    let interpreter_str = interpreter_path.to_string_lossy().into_owned();
    out.push("--ro-bind".to_string());
    out.push(interpreter_str.clone());
    out.push(interpreter_str);

    // (4) Binds condicionais a `allow_network` (DNS/TLS).
    if profile.allow_network {
        push_ro_bind_try(&mut out, "/etc/resolv.conf", "/etc/resolv.conf");
        push_ro_bind_try(&mut out, "/etc/hosts", "/etc/hosts");
        push_ro_bind_try(&mut out, "/etc/ssl", "/etc/ssl");
        push_ro_bind_try(&mut out, "/etc/pki", "/etc/pki");
    }

    // (5) Binds condicionais a `allow_exec`.
    if profile.allow_exec {
        push_ro_bind_try(&mut out, "/usr/bin", "/usr/bin");
        push_ro_bind_try(&mut out, "/bin", "/bin");
        push_ro_bind_try(&mut out, "/usr/local/bin", "/usr/local/bin");
    }

    // (6) Bind read-only da raiz do repo + `--chdir` (D4).
    let repo_root_str = repo_root.to_string_lossy().into_owned();
    out.push("--ro-bind".to_string());
    out.push(repo_root_str.clone());
    out.push(repo_root_str.clone());
    out.push("--chdir".to_string());
    out.push(repo_root_str);

    // (7) `extra_binds` do `SandboxProfile` — SEMPRE por último entre os
    // binds reais, depois do passo 2 (D5, regressão de ordem coberta em
    // `sandbox_unit_tests`).
    for bind in &profile.extra_binds {
        let host_str = bind.host_path.to_string_lossy().into_owned();
        if bind.writable {
            out.push("--bind-try".to_string());
        } else {
            out.push("--ro-bind-try".to_string());
        }
        out.push(host_str.clone());
        out.push(host_str);
    }

    // (8) `--` + comando final (caminho absoluto) + args.
    out.push("--".to_string());
    out.push(interpreter_path.to_string_lossy().into_owned());
    out.extend(args.iter().cloned());

    out
}

fn push_ro_bind_try(out: &mut Vec<String>, src: &str, dest: &str) {
    out.push("--ro-bind-try".to_string());
    out.push(src.to_string());
    out.push(dest.to_string());
}

#[cfg(test)]
mod sandbox_unit_tests {
    use super::*;

    fn profile(allow_network: bool, allow_exec: bool, extra_binds: Vec<BindMount>) -> SandboxProfile {
        SandboxProfile {
            allow_network,
            allow_exec,
            extra_binds,
        }
    }

    #[test]
    fn no_network_no_exec_omits_share_net_and_usr_bin() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, false, vec![]),
            "python3",
            &[],
        );

        assert!(!args.contains(&"--share-net".to_string()));
        assert!(!args.contains(&"/usr/bin".to_string()));
        assert!(!args.contains(&"/bin".to_string()));
        assert!(!args.contains(&"/usr/local/bin".to_string()));
        // Namespaces e die-with-parent continuam presentes.
        assert!(args.contains(&"--unshare-all".to_string()));
        assert!(args.contains(&"--die-with-parent".to_string()));
    }

    #[test]
    fn network_profile_adds_share_net_and_dns_binds() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(true, false, vec![]),
            "python3",
            &[],
        );

        assert!(args.contains(&"--share-net".to_string()));
        assert!(args.contains(&"/etc/resolv.conf".to_string()));
        assert!(args.contains(&"/etc/hosts".to_string()));
        assert!(args.contains(&"/etc/ssl".to_string()));
        assert!(args.contains(&"/etc/pki".to_string()));
    }

    #[test]
    fn exec_profile_adds_usr_bin_and_bin() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, true, vec![]),
            "python3",
            &[],
        );

        assert!(args.contains(&"/usr/bin".to_string()));
        assert!(args.contains(&"/bin".to_string()));
        assert!(args.contains(&"/usr/local/bin".to_string()));
    }

    #[test]
    fn extra_binds_appear_and_reflect_writable_flag() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(
                false,
                false,
                vec![
                    BindMount {
                        host_path: PathBuf::from("/home/user/dev"),
                        writable: true,
                    },
                    BindMount {
                        host_path: PathBuf::from("/var/run/docker.sock"),
                        writable: false,
                    },
                ],
            ),
            "python3",
            &[],
        );

        // Writable vira `--bind-try`, não `--ro-bind-try`.
        let dev_flag_index = args.iter().position(|a| a == "/home/user/dev").unwrap() - 1;
        assert_eq!(args[dev_flag_index], "--bind-try");

        let docker_flag_index = args
            .iter()
            .position(|a| a == "/var/run/docker.sock")
            .unwrap()
            - 1;
        assert_eq!(args[docker_flag_index], "--ro-bind-try");
    }

    /// Regressão do bug real de ordem encontrado e corrigido em
    /// `research.md` D5: `extra_binds` MUST vir depois de `--tmpfs /tmp` na
    /// lista de argumentos — um bind posicionado antes some se estiver
    /// aninhado sob `/tmp`, porque `--tmpfs` monta por cima. Trava a ordem
    /// comparando índices na `Vec<String>` resultante.
    #[test]
    fn extra_binds_come_after_tmpfs_tmp() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(
                true,
                true,
                vec![BindMount {
                    host_path: PathBuf::from("/tmp/scan-root-teste"),
                    writable: true,
                }],
            ),
            "python3",
            &[],
        );

        let tmpfs_flag_index = args
            .iter()
            .position(|a| a == "--tmpfs")
            .expect("--tmpfs MUST estar presente");
        let extra_bind_flag_index = args
            .iter()
            .position(|a| a == "--bind-try")
            .expect("o extra_bind writable MUST estar presente como --bind-try");

        assert!(
            extra_bind_flag_index > tmpfs_flag_index,
            "extra_binds MUST vir depois de --tmpfs /tmp (research.md D5); \
             tmpfs em {tmpfs_flag_index}, extra_bind em {extra_bind_flag_index}"
        );
    }

    /// T022 (feature 006, US3/P3, `research.md` D9): confirma — não constrói,
    /// já é garantido por omissão, já que `known_plugins()` nunca popula
    /// `extra_binds` com um caminho sob `~/.config/farol` — que **nenhum**
    /// bind emitido por [`build_bwrap_args`] para os 4 perfis reais de
    /// `plugin_worker::known_plugins()` aponta para dentro da base de
    /// configuração/segredos do Farol (`config_store::farol_config_base_dir()`,
    /// que também é a base de `secrets_store::secrets_path()` — T017/D8 lá).
    ///
    /// Não roda `bwrap`: audita só a `Vec<String>` produzida, percorrendo os
    /// argumentos aos pares sempre que encontra um dos 4 flags de bind
    /// (`--ro-bind`, `--bind`, `--ro-bind-try`, `--bind-try`) — cada um é
    /// sempre seguido de `SRC` e depois `DEST` (`SRC == DEST` sempre nesta
    /// feature, doc de [`BindMount`], mas o teste audita os dois
    /// independentemente, sem assumir a igualdade).
    const BIND_FLAGS: [&str; 4] = ["--ro-bind", "--bind", "--ro-bind-try", "--bind-try"];

    /// Um único bind (`SRC`, `DEST`) extraído por [`bind_pairs`] — nome do
    /// flag mantido só para mensagens de assert legíveis.
    struct BindPair<'a> {
        flag: &'a str,
        src: &'a str,
        dest: &'a str,
    }

    /// Percorre `args` aos pares sempre que encontra um dos 4 flags de bind
    /// (`--ro-bind`, `--bind`, `--ro-bind-try`, `--bind-try`), devolvendo
    /// `(SRC, DEST)` de cada um — extraído à parte de
    /// [`no_known_plugin_binds_the_farol_config_base_dir`] só para manter o
    /// nível de aninhamento do laço principal do teste dentro do limite do
    /// clippy (`excessive_nesting`).
    fn bind_pairs<'a>(plugin_name: &str, args: &'a [String]) -> Vec<BindPair<'a>> {
        let mut pairs = Vec::new();
        let mut index = 0;
        while index < args.len() {
            if !BIND_FLAGS.contains(&args[index].as_str()) {
                index += 1;
                continue;
            }
            let flag = args[index].as_str();
            let src = args
                .get(index + 1)
                .unwrap_or_else(|| panic!("plugin {plugin_name}: flag {flag:?} sem SRC subsequente: {args:?}"));
            let dest = args
                .get(index + 2)
                .unwrap_or_else(|| panic!("plugin {plugin_name}: flag {flag:?} sem DEST subsequente: {args:?}"));
            pairs.push(BindPair { flag, src, dest });
            index += 3;
        }
        pairs
    }

    #[test]
    fn no_known_plugin_binds_the_farol_config_base_dir() {
        let base_dir = crate::config_store::farol_config_base_dir();
        let base_dir_str = base_dir.to_string_lossy().into_owned();
        assert!(
            !base_dir_str.is_empty(),
            "farol_config_base_dir() não deveria resolver para uma string vazia"
        );

        for spawn_config in crate::plugin_worker::known_plugins() {
            let args = build_bwrap_args(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &spawn_config.sandbox_profile,
                &spawn_config.command,
                &spawn_config.args,
            );

            for pair in bind_pairs(&spawn_config.plugin_name, &args) {
                assert!(
                    !pair.src.contains(&base_dir_str) && !pair.src.starts_with(&base_dir_str),
                    "plugin {}: bind {:?} SRC={:?} aponta para dentro de \
                     farol_config_base_dir()={base_dir_str:?} — segredos/config vazariam \
                     para o sandbox (research.md D9)",
                    spawn_config.plugin_name,
                    pair.flag,
                    pair.src
                );
                assert!(
                    !pair.dest.contains(&base_dir_str) && !pair.dest.starts_with(&base_dir_str),
                    "plugin {}: bind {:?} DEST={:?} aponta para dentro de \
                     farol_config_base_dir()={base_dir_str:?} — segredos/config vazariam \
                     para o sandbox (research.md D9)",
                    spawn_config.plugin_name,
                    pair.flag,
                    pair.dest
                );
            }
        }
    }

    #[test]
    fn final_segment_uses_absolute_interpreter_path_and_preserves_args() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, false, vec![]),
            "python3",
            &["plugins/git-local/main.py".to_string()],
        );

        let separator_index = args.iter().position(|a| a == "--").unwrap();
        assert_eq!(args[separator_index + 1], "/usr/bin/python3");
        assert_eq!(args[separator_index + 2], "plugins/git-local/main.py");
        assert_eq!(args.len(), separator_index + 3);
    }
}

/// Testes de integração real com o `bwrap` de verdade instalado nesta
/// máquina de desenvolvimento (`bubblewrap 0.11.0`, `/usr/bin/bwrap`) — não
/// mocka nada: monta um `SandboxProfile`, chama [`build_bwrap_args`] e
/// spawna `python3` de verdade sob o `bwrap` resultante, replicando os dois
/// experimentos negativos já validados manualmente nesta sessão (D11,
/// `contracts/bwrap-invocation-contract.md` § "Casos negativos de
/// referência").
///
/// **Não são `#[ignore]`**: `bwrap` está confirmado presente no `PATH`
/// desta máquina. Num ambiente sem `bwrap` instalado, estes dois testes
/// falham de forma óbvia (o `assert!` sobre o `status`/`stdout` do processo
/// spawnado não bate, ou o próprio `Command::new("bwrap").spawn()` retorna
/// `Err`/panica no `.expect(...)`) — não é requisito desta task tratar essa
/// ausência graciosamente em teste, só documentar o comportamento.
#[cfg(test)]
mod sandbox_integration_tests {
    use super::*;
    use std::process::Command;

    fn python3_path() -> PathBuf {
        resolve_interpreter_path("python3").expect(
            "python3 MUST estar no PATH desta máquina de desenvolvimento para estes testes rodarem",
        )
    }

    fn repo_root() -> PathBuf {
        // `cargo test` já roda com cwd = raiz do crate (`crates/farol-core`);
        // sobe dois níveis para a raiz do repositório, mesma convenção
        // documentada em `plugin_worker::known_plugins`.
        std::env::current_dir()
            .expect("cwd do processo de teste")
            .parent()
            .and_then(Path::parent)
            .expect("raiz do repositório (dois níveis acima de crates/farol-core)")
            .to_path_buf()
    }

    /// Perfil sem rede: `socket.create_connection` a um IP externo real
    /// MUST falhar — replica o experimento manual de `research.md` D2/D11
    /// ("Network is unreachable").
    #[test]
    fn network_denied_blocks_outbound_connection() {
        let interpreter = python3_path();
        let profile = SandboxProfile {
            allow_network: false,
            allow_exec: false,
            extra_binds: vec![],
        };
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                "import socket\n\
                 try:\n\
                 \tsocket.create_connection(('1.1.1.1', 80), timeout=2)\n\
                 \tprint('LEAKED')\n\
                 except OSError:\n\
                 \tprint('BLOCKED')\n"
                    .to_string(),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("BLOCKED"),
            "esperava rede bloqueada (BLOCKED) sob allow_network=false; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Perfil sem exec: `subprocess.run(['/usr/bin/true'])` MUST falhar com
    /// `FileNotFoundError` — replica o experimento manual de `research.md`
    /// D3/D11 ("No such file or directory").
    #[test]
    fn exec_denied_blocks_external_binary() {
        let interpreter = python3_path();
        let profile = SandboxProfile {
            allow_network: false,
            allow_exec: false,
            extra_binds: vec![],
        };
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                "import subprocess\n\
                 try:\n\
                 \tsubprocess.run(['/usr/bin/true'])\n\
                 \tprint('LEAKED')\n\
                 except FileNotFoundError:\n\
                 \tprint('BLOCKED')\n"
                    .to_string(),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("BLOCKED"),
            "esperava exec bloqueado (BLOCKED) sob allow_exec=false; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// Localiza a entrada de `plugin_name` no registro real
    /// (`plugin_worker::known_plugins()`) — usado pelos testes abaixo (T013/
    /// T018) para atrelar o teste à configuração real de cada plugin, em vez
    /// de um `SandboxProfile` sintético construído à mão.
    fn known_profile(plugin_name: &str) -> SandboxProfile {
        crate::plugin_worker::known_plugins()
            .into_iter()
            .find(|p| p.plugin_name == plugin_name)
            .unwrap_or_else(|| panic!("known_plugins() MUST conter uma entrada para {plugin_name}"))
            .sandbox_profile
    }

    /// T013/T018: perfil REAL de `docker-containers` (`allow_network:
    /// false`, `contracts/bwrap-invocation-contract.md` § "Perfis
    /// resolvidos por plugin`) — mesmo experimento negativo de rede que
    /// `network_denied_blocks_outbound_connection`, mas atrelado à
    /// configuração real do plugin em vez de um profile sintético.
    #[test]
    fn docker_containers_real_profile_denies_network() {
        let interpreter = python3_path();
        let profile = known_profile("docker-containers");
        assert!(
            !profile.allow_network,
            "docker-containers MUST ter allow_network=false (contracts/bwrap-invocation-contract.md)"
        );
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                "import socket\n\
                 try:\n\
                 \tsocket.create_connection(('1.1.1.1', 80), timeout=2)\n\
                 \tprint('LEAKED')\n\
                 except OSError:\n\
                 \tprint('BLOCKED')\n"
                    .to_string(),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("BLOCKED"),
            "esperava rede bloqueada (BLOCKED) sob o perfil real de docker-containers; \
             stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// T013/T018: perfil REAL de `uptime-kuma` (`allow_exec: false`) — mesmo
    /// experimento negativo de exec que `exec_denied_blocks_external_binary`,
    /// mas atrelado à configuração real do plugin.
    #[test]
    fn uptime_kuma_real_profile_denies_exec() {
        let interpreter = python3_path();
        let profile = known_profile("uptime-kuma");
        assert!(
            !profile.allow_exec,
            "uptime-kuma MUST ter allow_exec=false (contracts/bwrap-invocation-contract.md)"
        );
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                "import subprocess\n\
                 try:\n\
                 \tsubprocess.run(['/usr/bin/true'])\n\
                 \tprint('LEAKED')\n\
                 except FileNotFoundError:\n\
                 \tprint('BLOCKED')\n"
                    .to_string(),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("BLOCKED"),
            "esperava exec bloqueado (BLOCKED) sob o perfil real de uptime-kuma; \
             stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// T014: prova o lado positivo de `allow_network: true` com um servidor
    /// TCP local de verdade — não basta provar que a ausência bloqueia
    /// (`network_denied_blocks_outbound_connection`); este teste prova que a
    /// concessão realmente libera rede, incluindo destinos locais. Sobe um
    /// `TcpListener` no próprio processo de teste (fora do sandbox) e conecta
    /// a ele de dentro do sandbox sob o perfil REAL de `uptime-kuma`
    /// (`allow_network: true`). `--share-net` (D2) compartilha o namespace de
    /// rede do host, então `127.0.0.1` dentro do sandbox é o mesmo loopback
    /// do processo de teste — a porta efêmera é alcançável normalmente.
    #[test]
    fn uptime_kuma_real_profile_allows_local_tcp_connection() {
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").expect("bind numa porta efêmera local");
        let port = listener.local_addr().expect("endereço local do listener").port();

        // Aceita (e descarta) a conexão recebida numa thread separada, só
        // para que o `connect()` do lado do sandbox complete o three-way
        // handshake normalmente em vez de ficar só no backlog do kernel.
        let accept_thread = std::thread::spawn(move || {
            let _ = listener.accept();
        });

        let interpreter = python3_path();
        let profile = known_profile("uptime-kuma");
        assert!(
            profile.allow_network,
            "uptime-kuma MUST ter allow_network=true (contracts/bwrap-invocation-contract.md)"
        );
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                format!(
                    "import socket\n\
                     s = socket.create_connection(('127.0.0.1', {port}), timeout=5)\n\
                     s.close()\n\
                     print('CONNECTED')\n"
                ),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        accept_thread.join().expect("thread de accept não deve panicar");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("CONNECTED"),
            "esperava conexão TCP local bem-sucedida (CONNECTED) sob allow_network=true; \
             stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    /// T019: `git fetch` de verdade sob o perfil de `git-local` com
    /// `extra_binds` populado (`resolve_git_local_scan_root`, T016/T017).
    /// Cria um repositório de origem + um clone num diretório temporário sob
    /// `std::env::temp_dir()` (mesmo padrão já usado por
    /// `config_store`/`secrets_store`/`e2e_tests`) — funciona mesmo aninhado
    /// sob `/tmp` porque `extra_binds` já é emitido depois de `--tmpfs /tmp`
    /// na ordem normativa de `build_bwrap_args` (D5/D14, correção de escopo
    /// 2026-09-03): não há mais o bug de sombreamento que motivou a correção
    /// de ordem desta feature. O `SandboxProfile` usado aqui é construído à
    /// mão (não `known_profile("git-local")`) só para poder apontar
    /// `extra_binds` para o diretório temporário deste teste em vez do
    /// `scan_root` real resolvido do host — `allow_network`/`allow_exec`
    /// continuam idênticos ao perfil real.
    #[test]
    fn git_local_profile_allows_git_fetch_via_extra_bind() {
        use std::process::Command as StdCommand;

        let temp_root = std::env::temp_dir().join(format!(
            "farol-sandbox-git-local-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("relógio do sistema não deve estar antes de UNIX_EPOCH")
                .as_nanos()
        ));
        std::fs::create_dir_all(&temp_root).expect("criar diretório temporário do teste");

        let origin_dir = temp_root.join("origin");
        let clone_dir = temp_root.join("clone");

        let git_env = [
            ("GIT_AUTHOR_NAME", "Farol Sandbox Test"),
            ("GIT_AUTHOR_EMAIL", "farol-sandbox-test@example.invalid"),
            ("GIT_COMMITTER_NAME", "Farol Sandbox Test"),
            ("GIT_COMMITTER_EMAIL", "farol-sandbox-test@example.invalid"),
        ];

        let run_git = |args: &[&str], cwd: &Path| {
            let status = StdCommand::new("git")
                .args(args)
                .current_dir(cwd)
                .envs(git_env)
                .status()
                .expect("git MUST estar instalado nesta máquina de desenvolvimento");
            assert!(status.success(), "git {args:?} falhou em {cwd:?}");
        };

        // Origem com um primeiro commit.
        std::fs::create_dir_all(&origin_dir).expect("criar diretório de origem");
        run_git(&["init", "-q", "-b", "main"], &origin_dir);
        std::fs::write(origin_dir.join("arquivo1.txt"), "commit1\n").expect("escrever arquivo1");
        run_git(&["add", "arquivo1.txt"], &origin_dir);
        run_git(&["commit", "-q", "-m", "commit1"], &origin_dir);

        // Clone (fora do sandbox — só a etapa de `fetch` abaixo roda dentro).
        run_git(
            &[
                "clone",
                "-q",
                &origin_dir.to_string_lossy(),
                &clone_dir.to_string_lossy(),
            ],
            &temp_root,
        );

        // Segundo commit na origem, ainda não presente no clone.
        std::fs::write(origin_dir.join("arquivo2.txt"), "commit2\n").expect("escrever arquivo2");
        run_git(&["add", "arquivo2.txt"], &origin_dir);
        run_git(&["commit", "-q", "-m", "commit2"], &origin_dir);

        let interpreter = python3_path();
        let profile = SandboxProfile {
            allow_network: true,
            allow_exec: true,
            extra_binds: vec![BindMount {
                host_path: temp_root.clone(),
                writable: true,
            }],
        };
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                format!(
                    "import subprocess\n\
                     r = subprocess.run(['git', '-C', {clone_dir:?}, 'fetch', 'origin'], \
                     capture_output=True, text=True)\n\
                     assert r.returncode == 0, r.stderr\n\
                     r2 = subprocess.run(\n\
                     \t['git', '-C', {clone_dir:?}, 'log', 'origin/main', '--oneline'],\n\
                     \tcapture_output=True, text=True,\n\
                     )\n\
                     assert r2.returncode == 0, r2.stderr\n\
                     print(r2.stdout)\n",
                    clone_dir = clone_dir.to_string_lossy(),
                ),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            output.status.success(),
            "git fetch sob o sandbox de git-local falhou; stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stdout.contains("commit2"),
            "esperava que o clone tivesse o commit2 trazido pelo fetch dentro do sandbox; \
             stdout={stdout:?} stderr={stderr:?}"
        );

        let _ = std::fs::remove_dir_all(&temp_root);
    }

    /// T020: `docker ps` de verdade sob o perfil de `docker-containers`
    /// (`allow_network: false`, socket já bindado por `extra_binds` em
    /// `known_plugins()`, T017), confirmando que o protocolo do daemon
    /// Docker via socket Unix funciona mesmo sem rede. Requer Docker
    /// instalado e o daemon acessível (`docker ps` sem erro) no ambiente onde
    /// os testes rodam — confirmado presente nesta máquina de desenvolvimento
    /// (`docker ps` funciona sem erro fora do sandbox). Num ambiente sem
    /// Docker instalado/daemon acessível, este teste MUST ser marcado
    /// `#[ignore = "requer Docker instalado e daemon acessível"]` em vez de
    /// rodar (mesma disciplina já usada na feature 005 para testes que
    /// dependem de Docker real) — não se aplica aqui porque Docker está
    /// disponível neste ambiente de execução.
    #[test]
    fn docker_containers_real_profile_allows_docker_ps_via_socket() {
        let interpreter = python3_path();
        let profile = known_profile("docker-containers");
        assert!(
            !profile.allow_network,
            "docker-containers MUST ter allow_network=false (contracts/bwrap-invocation-contract.md)"
        );
        assert!(
            profile
                .extra_binds
                .iter()
                .any(|b| b.host_path == Path::new("/var/run/docker.sock")),
            "docker-containers MUST ter o socket Docker em extra_binds (T017)"
        );

        let args = build_bwrap_args(&repo_root(), &interpreter, &profile, "python3", &[
            "-c".to_string(),
            "import subprocess\n\
             r = subprocess.run(['docker', 'ps'], capture_output=True, text=True)\n\
             assert r.returncode == 0, r.stderr\n\
             print('DOCKER_PS_OK')\n"
                .to_string(),
        ]);

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("DOCKER_PS_OK"),
            "esperava `docker ps` funcionando sob docker-containers (allow_network=false); \
             stdout={stdout:?} stderr={stderr:?}"
        );
    }

    /// T023 (feature 006, US3/P3, `research.md` D9): prova, com `bwrap` real,
    /// que o processo do plugin `uptime-kuma` (perfil real de
    /// `plugin_worker::known_plugins()`) não consegue abrir
    /// `secrets.toml` do Farol de dentro do sandbox — mesmo caminho que
    /// `crate::secrets_store::secrets_path()` resolveria do lado de fora,
    /// já que nenhum bind desta feature aponta para
    /// `config_store::farol_config_base_dir()` (T022 confirma isso
    /// estaticamente; este teste confirma o efeito real de I/O). Complementa
    /// (não substitui) T022: T022 audita a `Vec<String>` sem executar nada;
    /// este teste é o experimento negativo de verdade (D11, mesmo padrão de
    /// `uptime_kuma_real_profile_denies_exec` acima), incluindo o caso do
    /// arquivo existir de fato no host (criado pelo próprio teste) — provando
    /// que não é só "o arquivo não existe", é "o sandbox não enxerga o
    /// caminho".
    #[test]
    fn uptime_kuma_real_profile_cannot_read_farol_secrets() {
        // Garante que `secrets.toml` existe de verdade no host neste
        // processo de teste — se o bind vazasse, a leitura teria sucesso
        // (LEAKED); só um `FileNotFoundError` por caminho inexistente não
        // provaria isolamento nenhum.
        let secrets_path = crate::secrets_store::secrets_path();
        if let Some(parent) = secrets_path.parent() {
            std::fs::create_dir_all(parent)
                .expect("criar o diretório pai de secrets.toml no host para o teste");
        }
        let secrets_already_existed = secrets_path.exists();
        if !secrets_already_existed {
            std::fs::write(&secrets_path, "[uptime-kuma]\nfake = \"nao-deveria-vazar\"\n")
                .expect("escrever secrets.toml de teste no host");
        }

        let interpreter = python3_path();
        let profile = known_profile("uptime-kuma");
        let args = build_bwrap_args(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &[
                "-c".to_string(),
                format!(
                    "try:\n\
                     \topen({secrets_path:?}).read()\n\
                     \tprint('LEAKED')\n\
                     except FileNotFoundError:\n\
                     \tprint('BLOCKED')\n",
                    secrets_path = secrets_path.to_string_lossy(),
                ),
            ],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        if !secrets_already_existed {
            let _ = std::fs::remove_file(&secrets_path);
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            stdout.contains("BLOCKED"),
            "esperava que o sandbox de uptime-kuma não conseguisse abrir secrets.toml do Farol \
             (BLOCKED, research.md D9); stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}
