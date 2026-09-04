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
}
