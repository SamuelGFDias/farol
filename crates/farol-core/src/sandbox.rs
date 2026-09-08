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

/// Erro de montagem do sandbox: sinaliza que um mecanismo de enforcement
/// REAL (não só visibilidade de filesystem) não pôde ser aplicado no sistema
/// atual (feature `009-sandbox-hardening`, T003, foundational a US1/US2).
///
/// Fail-closed (FR-004): nenhuma variante tem um caminho de fallback
/// silencioso para o comportamento antigo (só filesystem para `exec`,
/// liga/desliga total para `network`) — o chamador MUST recusar montar o
/// sandbox sem a proteção correspondente. Como [`build_bwrap_args`] é hoje
/// uma função infalível (`Vec<String>`, chamada por `plugin_worker::worker()`
/// sem tratamento de `Result` — mudar essa assinatura pública está fora do
/// escopo desta feature, ver `plan.md` § Complexity Tracking), o contrato é
/// propagar a mensagem deste erro via `panic!` no ponto exato em que o
/// mecanismo se revela indisponível, nunca continuar a composição dos
/// argumentos como se a proteção estivesse em vigor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SandboxMountError {
    /// [US1] O filtro seccomp real de `exec` (`crates/farol-seccomp-preload`,
    /// aplicado via `LD_PRELOAD`) não pôde ser localizado/aplicado neste
    /// sistema — na prática, hoje, sempre porque o `.so` ainda não foi
    /// compilado (`cargo build -p farol-seccomp-preload`); a mensagem já vem
    /// pronta de [`crate::sandbox_seccomp::locate_seccomp_preload_library`].
    SeccompUnavailable(String),
    /// [US2] `nft`/`iptables` ausentes/inutilizáveis para aplicar a
    /// allowlist de rede por host declarada (T010/T014), ou um host
    /// declarado que não resolveu para nenhum IP (T009, edge case de
    /// `spec.md`) — construída por
    /// `sandbox_network::detect_firewall_tool`/`resolve_hosts_to_ips` e
    /// propagada via `panic!` por
    /// [`build_bwrap_args_with_network_hosts`], nunca um fallback
    /// silencioso para liga/desliga total.
    NetworkFirewallUnavailable(String),
}

impl std::fmt::Display for SandboxMountError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SandboxMountError::SeccompUnavailable(detail) => {
                write!(f, "mediação real de exec via seccomp indisponível: {detail}")
            }
            SandboxMountError::NetworkFirewallUnavailable(detail) => {
                write!(f, "allowlist de rede via nftables/iptables indisponível: {detail}")
            }
        }
    }
}

impl std::error::Error for SandboxMountError {}

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
/// `allow_network`; (5) binds condicionais a `allow_exec` (bind read-only de
/// `/usr/bin`/`/bin`/`/usr/local/bin` quando `true`, mediação por
/// visibilidade de filesystem, D3 da feature 006); (6) bind da raiz do repo
/// e `--chdir`; (6b) [US1, T005, feature `009-sandbox-hardening`] mediação
/// REAL de `exec` via seccomp quando `allow_exec = false`: bind read-only do
/// `.so` de `farol-seccomp-preload` + `--setenv LD_PRELOAD <caminho>` —
/// MUST vir depois do passo (6), nunca antes (mesma classe de bug de
/// sombreamento de ordem já corrigida para `extra_binds` em D5; o `.so`
/// vive sob `<repo_root>/target/{debug,release}/`, aninhado sob o bind da
/// raiz do repo); (7) `extra_binds`, sempre por último entre os binds reais
/// — a posição relativa aos passos 3-6b não importa, só estar depois do
/// passo 2; (8) `--` + o comando final.
///
/// Fail-closed (FR-004, `plan.md` D1 revisado): quando `allow_exec = false`
/// e o `.so` de `farol-seccomp-preload` não pode ser localizado
/// (`sandbox_seccomp::locate_seccomp_preload_library` devolve
/// `Err(SandboxMountError::SeccompUnavailable)`), esta função entra em
/// `panic!` com a mensagem do erro em vez de compor um sandbox sem a
/// proteção real de `exec` — nunca degrada silenciosamente para o
/// comportamento antigo (só visibilidade de filesystem).
///
/// `command` (o nome/comando original do plugin, ex. `"python3"`) não entra
/// na lista de argumentos produzida — o processo final é sempre invocado
/// pelo caminho absoluto já resolvido em `interpreter_path` (D10). O
/// parâmetro é mantido na assinatura por simetria com `config.command`/
/// `config.args` do chamador (`plugin_worker::worker`, T008) e para
/// eventual uso futuro (ex.: logging), por isso prefixado com `_` — não
/// participa da composição dos argumentos.
///
/// [US2, feature `009-sandbox-hardening`] Assinatura pública intocada de
/// propósito — chamada hoje por `plugin_worker::worker()` com exatamente
/// estes 5 parâmetros posicionais, arquivo que esta subtarefa está proibida
/// de editar. Sempre delega para
/// [`build_bwrap_args_with_network_hosts`] com `network_hosts: &[]`
/// (nenhuma allowlist de rede por host) — comportamento 100% idêntico ao
/// anterior à feature 009/US2 (ver doc daquela função e de
/// `sandbox_network` para o porquê disso, não de um novo campo em
/// [`SandboxProfile`]).
pub fn build_bwrap_args(
    repo_root: &Path,
    interpreter_path: &Path,
    profile: &SandboxProfile,
    command: &str,
    args: &[String],
) -> Vec<String> {
    build_bwrap_args_with_network_hosts(repo_root, interpreter_path, profile, command, args, &[])
}

/// [US2, T009-T011, feature `009-sandbox-hardening`] Mesma composição de
/// [`build_bwrap_args`], estendida com `network_hosts`: quando não vazia (e
/// `profile.allow_network` é `true`), restringe o egress de rede aos IPs
/// desses hosts (FR-005/FR-006/FR-008/FR-009) em vez do liga/desliga total
/// via `--share-net` de antes desta feature — ver módulo
/// [`crate::sandbox_network`] para o mecanismo completo (resolução de
/// host→IP no processo pai, geração do script wrapper `nft`/`iptables`, e a
/// nota extensa sobre por que este parâmetro existe como argumento
/// explícito em vez de um campo novo em [`SandboxProfile`]).
///
/// `network_hosts` vazia (o caso de TODOS os 4 plugins de referência de
/// `plugin_worker::known_plugins()` hoje, já que a extração de hosts do
/// manifesto de um plugin para este parâmetro é integração explicitamente
/// Fora de Escopo desta subtarefa) reproduz exatamente o comportamento de
/// `build_bwrap_args` anterior a esta função existir — nenhuma regressão
/// (T012).
///
/// Fail-closed (FR-004 por analogia): `panic!` com a mensagem do
/// [`SandboxMountError`] correspondente se os hosts declarados não
/// resolverem para IP (T009) ou se nem `nft` nem `iptables` estiverem
/// disponíveis (T010/T014) — nunca monta um sandbox com `network_hosts` não
/// vazia sem a allowlist real em vigor.
pub(crate) fn build_bwrap_args_with_network_hosts(
    repo_root: &Path,
    interpreter_path: &Path,
    profile: &SandboxProfile,
    _command: &str,
    args: &[String],
    network_hosts: &[String],
) -> Vec<String> {
    // [US2] Só ativa o mecanismo de allowlist por host quando rede está
    // ligada E há pelo menos um host declarado — `network_hosts` não vazia
    // com `allow_network=false` (combinação sem sentido, nunca produzida
    // por `plugin_worker::known_plugins()`) é tratada como se estivesse
    // vazia, sem nenhum efeito.
    let network_allowlist_active = profile.allow_network && !network_hosts.is_empty();

    let mut out: Vec<String> = Vec::new();

    // (1) Namespaces + die-with-parent.
    out.push("--unshare-all".to_string());
    if network_allowlist_active {
        // [US2] Desvio deliberado do texto curto de T011 ("preservando
        // --share-net") — ver doc extensa do módulo `sandbox_network` para
        // a justificativa completa (D3/FR-008 exigem um namespace de rede
        // "recém-criado"/"criado pelo bwrap", que só existe SEM
        // `--share-net`; `--cap-add CAP_NET_ADMIN` só é efetivo, validado
        // empiricamente, sobre um namespace de rede que o próprio user
        // namespace novo do `bwrap` é dono — nunca sobre o namespace real
        // do host que `--share-net` compartilharia).
        out.push("--uid".to_string());
        out.push("0".to_string());
        out.push("--gid".to_string());
        out.push("0".to_string());
        out.push("--cap-add".to_string());
        out.push("CAP_NET_ADMIN".to_string());
    } else if profile.allow_network {
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

    // (6b) [US1, T005] Mediação REAL de `exec` via seccomp (issue #12):
    // quando `allow_exec = false`, localiza o `.so` já compilado de
    // `farol-seccomp-preload` (`sandbox_seccomp::locate_seccomp_preload_library`),
    // bind-monta-o read-only dentro do sandbox e repassa `--setenv LD_PRELOAD
    // <caminho>` ao `bwrap` — o construtor ELF do `.so` (`#[ctor]`) aplica o
    // filtro seccomp-bpf real DENTRO do processo do plugin, depois que o
    // `bwrap` já o `exec`ou com sucesso (D1 revisado de `plan.md`), fechando
    // o vetor de escrever-e-executar um binário por caminho absoluto num
    // `tmpfs` gravável que a mediação por visibilidade de filesystem (D3 da
    // feature 006) sozinha não cobria.
    //
    // Posicionado DEPOIS do bind da raiz do repo (passo 6, acima), nunca
    // antes — mesma classe de bug de sombreamento de ordem já documentada e
    // testada para `extra_binds` (D5): o `.so` vive sob
    // `<repo_root>/target/{debug,release}/`, aninhado sob esse bind.
    //
    // Quando `allow_exec = true`, NÃO aplica filtro nenhum (D2 de
    // `plan.md`) — mantém o comportamento atual do passo (5) acima, sem
    // mudança.
    //
    // Fail-closed (FR-004): `panic!` com a mensagem do erro se o `.so` não
    // puder ser localizado — nunca monta o sandbox sem a proteção real.
    //
    // [US2] Quando `network_allowlist_active`, o `--setenv LD_PRELOAD` NÃO é
    // emitido aqui — seria herdado pelo próprio processo do WRAPPER de rede
    // (passo 8 abaixo, o processo que o `bwrap` de fato `exec`a primeiro),
    // cujo construtor ELF bloquearia o `execve` que o PRÓPRIO wrapper
    // precisa fazer (para `nft`/`ip`, e para o `exec` final do
    // interpretador) antes mesmo de aplicar a allowlist — mesma classe de
    // bug de D1, um nível acima (ver doc de
    // `sandbox_network::plan_network_wrapper`). Em vez disso, o caminho do
    // `.so` é guardado em `ld_preload_path_for_wrapper` e o PRÓPRIO SCRIPT
    // do wrapper exporta `LD_PRELOAD` só imediatamente antes do seu `exec`
    // final.
    let mut ld_preload_path_for_wrapper: Option<PathBuf> = None;
    if !profile.allow_exec {
        match crate::sandbox_seccomp::locate_seccomp_preload_library() {
            Ok(preload_path) => {
                let preload_str = preload_path.to_string_lossy().into_owned();
                out.push("--ro-bind".to_string());
                out.push(preload_str.clone());
                out.push(preload_str.clone());
                if network_allowlist_active {
                    ld_preload_path_for_wrapper = Some(preload_path);
                } else {
                    out.push("--setenv".to_string());
                    out.push("LD_PRELOAD".to_string());
                    out.push(preload_str);
                }
            }
            Err(err) => panic!("{err}"),
        }
    }

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

    // (8) `--` + comando final (caminho absoluto) + args — ou, [US2] quando
    // `network_allowlist_active`, os binds das ferramentas do wrapper de
    // rede (T009/T010) seguidos de `--` + o wrapper `sh -c <script>
    // <interpreter> <args...>` (T011) no lugar do exec direto do
    // interpretador. Fail-closed: `panic!` com a mensagem do
    // `SandboxMountError` se os hosts não resolverem (T009) ou se nem
    // `nft` nem `iptables` estiverem disponíveis (T010/T014).
    if network_allowlist_active {
        let wrapper = crate::sandbox_network::plan_network_wrapper(
            network_hosts,
            ld_preload_path_for_wrapper.as_deref(),
            interpreter_path,
            args,
        )
        .unwrap_or_else(|err| panic!("{err}"));

        out.extend(wrapper.tool_binds);
        out.push("--".to_string());
        out.extend(wrapper.final_command);
    } else {
        out.push("--".to_string());
        out.push(interpreter_path.to_string_lossy().into_owned());
        out.extend(args.iter().cloned());
    }

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

    /// T006 (feature `009-sandbox-hardening`, US1): `allow_exec = false` MUST
    /// resultar num bind read-only do `.so` de `farol-seccomp-preload` e num
    /// `--setenv LD_PRELOAD <caminho>` correspondente — a mediação real de
    /// exec via seccomp (issue #12). Depende do `.so` já estar compilado
    /// (`cargo build -p farol-seccomp-preload`); sem ele,
    /// `locate_seccomp_preload_library()` devolve `Err` e o `.expect(...)`
    /// abaixo falha com uma mensagem explicando o pré-requisito, em vez de
    /// deixar `build_bwrap_args` panicar de forma menos clara.
    #[test]
    fn exec_denied_binds_seccomp_preload_library_and_sets_ld_preload() {
        let preload_path = crate::sandbox_seccomp::locate_seccomp_preload_library()
            .expect(
                "libfarol_seccomp_preload.so MUST estar compilado para este teste — rode \
                 `cargo build -p farol-seccomp-preload` antes de `cargo test -p farol-core`",
            )
            .to_string_lossy()
            .into_owned();

        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, false, vec![]),
            "python3",
            &[],
        );

        let bind_flag_index = args
            .iter()
            .position(|a| a == &preload_path)
            .map(|i| i - 1)
            .unwrap_or_else(|| {
                panic!("esperava bind do .so de farol-seccomp-preload ({preload_path:?}) em args={args:?}")
            });
        assert_eq!(
            args[bind_flag_index], "--ro-bind",
            "o .so de farol-seccomp-preload MUST ser bindado read-only (não --ro-bind-try, já \
             que sua ausência é fail-closed via panic!, não um bind opcional)"
        );

        let setenv_index = args
            .iter()
            .position(|a| a == "--setenv")
            .expect("esperava --setenv LD_PRELOAD <caminho> presente quando allow_exec=false");
        assert_eq!(args[setenv_index + 1], "LD_PRELOAD");
        assert_eq!(args[setenv_index + 2], preload_path);
    }

    /// T006: o caso contrário — `allow_exec = true` NÃO aplica o filtro
    /// seccomp (D2 de `plan.md`), então nenhum bind do `.so` nem
    /// `--setenv LD_PRELOAD` deve aparecer.
    #[test]
    fn exec_allowed_does_not_bind_seccomp_preload_library_nor_set_ld_preload() {
        let args = build_bwrap_args(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, true, vec![]),
            "python3",
            &[],
        );

        assert!(
            !args.contains(&"--setenv".to_string()),
            "allow_exec=true não deveria emitir nenhum --setenv; args={args:?}"
        );
        assert!(
            !args.iter().any(|a| a.ends_with("libfarol_seccomp_preload.so")),
            "allow_exec=true não deveria bindar o .so de farol-seccomp-preload; args={args:?}"
        );
    }

    /// T008: fail-closed (FR-004) — com o escape-hatch de teste de
    /// `sandbox_seccomp` forçando o `.so` a parecer ausente, `allow_exec =
    /// false` MUST fazer `build_bwrap_args` entrar em `panic!` em vez de
    /// devolver um `Vec<String>` que monta o sandbox sem a proteção real de
    /// exec. Serializado via `FORCE_LIBRARY_MISSING_ENV_VAR_TEST_LOCK`
    /// (mesmo lock compartilhado com `sandbox_seccomp::tests`, doc do lock
    /// em `sandbox_seccomp.rs` explica por quê precisa ser único).
    #[test]
    fn exec_denied_panics_when_seccomp_preload_library_cannot_be_located() {
        let _guard = crate::sandbox_seccomp::FORCE_LIBRARY_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::set_var(crate::sandbox_seccomp::FORCE_LIBRARY_MISSING_ENV_VAR, "1");
        let result = std::panic::catch_unwind(|| {
            build_bwrap_args(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &profile(false, false, vec![]),
                "python3",
                &[],
            )
        });
        std::env::remove_var(crate::sandbox_seccomp::FORCE_LIBRARY_MISSING_ENV_VAR);

        assert!(
            result.is_err(),
            "esperava panic! quando o .so de farol-seccomp-preload não pode ser localizado \
             (FR-004, fail-closed) — build_bwrap_args NUNCA deve montar o sandbox sem a \
             proteção real de exec"
        );
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

    /// T012 [US2] Regressão: `network_hosts` vazia MUST produzir exatamente
    /// o mesmo `Vec<String>` que `build_bwrap_args` (a função pública
    /// original, sem o parâmetro novo) produzia antes desta feature —
    /// garante que nenhum dos 4 perfis reais de
    /// `plugin_worker::known_plugins()` (que só chamam `build_bwrap_args`,
    /// nunca a variante com hosts) sofre nenhuma mudança de comportamento.
    ///
    /// Deliberadamente NÃO varia `allow_exec=false` aqui (ficaria sujeito ao
    /// mesmo tipo de corrida de variável de ambiente global de processo já
    /// documentado em `exec_denied_panics_when_seccomp_preload_library_cannot_be_located`,
    /// que só se protege via lock contra OUTROS testes que também tomam o
    /// mesmo lock — um teste como este, que não toma lock nenhum, pode ler
    /// `FAROL_SANDBOX_TEST_FORCE_SECCOMP_PRELOAD_MISSING` no meio de uma
    /// janela em que aquele outro teste a define, sob execução paralela de
    /// `cargo test`; isso não é um bug do mecanismo de rede desta feature —
    /// é uma limitação preexistente do padrão de escape-hatch via env var
    /// global, fora do escopo desta task consertar). Cobrir só a dimensão
    /// `allow_network` já é suficiente para provar a equivalência de
    /// `network_hosts` vazia — a dimensão `allow_exec` já tem sua própria
    /// cobertura de regressão em `exec_allowed_does_not_bind_seccomp_preload_library_nor_set_ld_preload`/
    /// `exec_denied_binds_seccomp_preload_library_and_sets_ld_preload`.
    #[test]
    fn network_hosts_empty_is_byte_for_byte_identical_to_build_bwrap_args() {
        for network in [false, true] {
            let via_public_fn = build_bwrap_args(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &profile(network, true, vec![]),
                "python3",
                &["plugins/git-local/main.py".to_string()],
            );
            let via_hosts_fn_empty = build_bwrap_args_with_network_hosts(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &profile(network, true, vec![]),
                "python3",
                &["plugins/git-local/main.py".to_string()],
                &[],
            );
            assert_eq!(
                via_public_fn, via_hosts_fn_empty,
                "network={network}: network_hosts vazia deveria ser 100% equivalente a \
                 build_bwrap_args (regressão T012)"
            );
        }
    }

    /// T012 [US2] `network_hosts` não vazia com `allow_network=false`
    /// (combinação sem sentido, nunca produzida por `known_plugins()`) é
    /// tratada como se `network_hosts` estivesse vazia — a rede continua
    /// completamente desligada (FR-007), sem `--uid 0`/`--cap-add
    /// CAP_NET_ADMIN` nenhum.
    #[test]
    fn network_hosts_present_but_network_disabled_has_no_effect() {
        let args = build_bwrap_args_with_network_hosts(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(false, true, vec![]),
            "python3",
            &[],
            &["127.0.0.1".to_string()],
        );

        assert!(!args.contains(&"--share-net".to_string()));
        assert!(!args.contains(&"--cap-add".to_string()));
        assert!(!args.iter().any(|a| a == "0" ));
    }

    /// T012 [US2] Com `allow_network=true` e ao menos um host declarado, o
    /// `Vec<String>` resultante: (1) NÃO contém `--share-net` (desvio
    /// deliberado de T011, ver doc de `build_bwrap_args_with_network_hosts`
    /// e do módulo `sandbox_network`); (2) contém `--uid 0 --gid 0
    /// --cap-add CAP_NET_ADMIN`; (3) o comando final depois de `--` não é
    /// mais o interpretador direto, e sim `sh -c <script>` com o host
    /// declarado embutido no script, seguido do interpretador/args
    /// originais. Depende de `nft` ou `iptables` estarem instalados nesta
    /// máquina (Assumptions de `spec.md` — mesma categoria de dependência já
    /// aceita por `exec_denied_binds_seccomp_preload_library_and_sets_ld_preload`,
    /// acima, para o `.so` de `farol-seccomp-preload`).
    #[test]
    fn network_hosts_present_swaps_share_net_for_private_netns_wrapper_reflecting_hosts() {
        let args = build_bwrap_args_with_network_hosts(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(true, true, vec![]),
            "python3",
            &["plugins/git-local/main.py".to_string()],
            &["127.0.0.1".to_string()],
        );

        assert!(
            !args.contains(&"--share-net".to_string()),
            "network_hosts não vazia MUST omitir --share-net (D3/FR-008: namespace de rede \
             recém-criado pelo bwrap, não o do host); args={args:?}"
        );
        assert!(args.contains(&"--uid".to_string()));
        assert!(args.contains(&"--cap-add".to_string()));
        assert!(args.contains(&"CAP_NET_ADMIN".to_string()));

        let separator_index = args
            .iter()
            .position(|a| a == "--")
            .expect("esperava um separador -- nos argumentos");
        assert!(
            args[separator_index + 1].ends_with("/sh"),
            "comando final MUST ser `<sh> -c <script> ...`, não mais o interpretador direto; \
             args={args:?}"
        );
        assert_eq!(
            args[separator_index + 2], "-c",
            "segundo argumento posicional depois de -- MUST ser -c; args={args:?}"
        );
        let script = &args[separator_index + 3];
        assert!(
            script.contains("127.0.0.1"),
            "script do wrapper deveria conter o IP do host declarado; script={script:?}"
        );
        // O interpretador original e seus args continuam presentes, agora
        // como argumentos posicionais do `sh -c` (viram $0/$@ dentro do
        // script, consumidos pelo `exec "$0" "$@"` final).
        assert!(args.contains(&"/usr/bin/python3".to_string()));
        assert!(args.contains(&"plugins/git-local/main.py".to_string()));
    }

    /// T012 [US2] Mesma combinação real de `uptime-kuma`
    /// (`allow_network=true`, `allow_exec=false`) com um host declarado: o
    /// `--setenv LD_PRELOAD` do `bwrap` NÃO deve aparecer (vazaria pro
    /// próprio wrapper — ver doc de `build_bwrap_args_with_network_hosts`);
    /// em vez disso, o `export LD_PRELOAD=` MUST estar embutido dentro do
    /// próprio script do wrapper.
    #[test]
    fn network_hosts_present_with_exec_denied_defers_ld_preload_into_the_wrapper_script() {
        crate::sandbox_seccomp::locate_seccomp_preload_library().expect(
            "libfarol_seccomp_preload.so MUST estar compilado para este teste — rode `cargo \
             build -p farol-seccomp-preload` antes de `cargo test -p farol-core`",
        );

        let args = build_bwrap_args_with_network_hosts(
            Path::new("/repo"),
            Path::new("/usr/bin/python3"),
            &profile(true, false, vec![]),
            "python3",
            &[],
            &["127.0.0.1".to_string()],
        );

        assert!(
            !args.contains(&"--setenv".to_string()),
            "--setenv LD_PRELOAD nunca deve ser emitido pelo bwrap quando network_hosts não é \
             vazia (vazaria pro processo do wrapper); args={args:?}"
        );

        let separator_index = args.iter().position(|a| a == "--").unwrap();
        let script = &args[separator_index + 3];
        assert!(
            script.contains("export LD_PRELOAD="),
            "script do wrapper deveria exportar LD_PRELOAD antes do exec final quando \
             allow_exec=false; script={script:?}"
        );
        assert!(
            script.contains("libfarol_seccomp_preload.so"),
            "script={script:?}"
        );
    }

    /// T014 [US2] Fail-closed (FR-004 por analogia): com o escape-hatch de
    /// teste de `sandbox_network` forçando nft/iptables a parecerem
    /// ausentes, `network_hosts` não vazia MUST fazer
    /// `build_bwrap_args_with_network_hosts` entrar em `panic!` em vez de
    /// devolver um `Vec<String>` que monta o sandbox sem a allowlist real —
    /// nunca degrada silenciosamente para o comportamento antigo de
    /// liga/desliga total via `--share-net`. Serializado via
    /// `FORCE_FIREWALL_MISSING_ENV_VAR_TEST_LOCK` (mesmo padrão do lock
    /// equivalente de `sandbox_seccomp`, doc lá explica por quê precisa ser
    /// único).
    #[test]
    fn network_hosts_present_panics_when_no_firewall_tool_is_available() {
        let _guard = crate::sandbox_network::FORCE_FIREWALL_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::set_var(crate::sandbox_network::FORCE_FIREWALL_MISSING_ENV_VAR, "1");
        let result = std::panic::catch_unwind(|| {
            build_bwrap_args_with_network_hosts(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &profile(true, true, vec![]),
                "python3",
                &[],
                &["127.0.0.1".to_string()],
            )
        });
        std::env::remove_var(crate::sandbox_network::FORCE_FIREWALL_MISSING_ENV_VAR);

        assert!(
            result.is_err(),
            "esperava panic! quando nem nft nem iptables estão disponíveis e network_hosts não \
             é vazia (FR-004 por analogia, fail-closed) — build_bwrap_args_with_network_hosts \
             NUNCA deve montar o sandbox sem a allowlist real de rede"
        );
    }

    /// T014 [US2] Fail-closed também para um host que não resolve (T009,
    /// edge case de `spec.md`): `panic!`, nunca uma allowlist parcial
    /// silenciosa.
    #[test]
    fn network_hosts_present_panics_when_a_declared_host_cannot_be_resolved() {
        let result = std::panic::catch_unwind(|| {
            build_bwrap_args_with_network_hosts(
                Path::new("/repo"),
                Path::new("/usr/bin/python3"),
                &profile(true, true, vec![]),
                "python3",
                &[],
                &["host-que-nao-existe.invalid".to_string()],
            )
        });

        assert!(
            result.is_err(),
            "esperava panic! quando um host declarado não resolve para IP (T009, fail-closed)"
        );
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

    /// Perfil sem exec: `subprocess.run(['/usr/bin/true'])` MUST falhar —
    /// originalmente (feature 006, D3/D11) sempre com `FileNotFoundError`
    /// ("No such file or directory"), já que `/usr/bin` nunca é bindado sob
    /// `allow_exec=false`. Desde a feature `009-sandbox-hardening` (T005), o
    /// filtro seccomp real (`LD_PRELOAD` de `farol-seccomp-preload`) também
    /// está ativo sob `allow_exec=false` e intercepta a própria syscall
    /// `execve` ANTES de o kernel sequer resolver o caminho — a exceção
    /// observada passa a ser `PermissionError` (`EACCES`, a ação do filtro
    /// BPF), não mais `FileNotFoundError` (`ENOENT`, a ausência de bind).
    /// Aceita as duas para continuar válido independente de qual dos dois
    /// mecanismos de mediação (visibilidade de filesystem da feature 006, ou
    /// seccomp da feature 009) é o primeiro a barrar a tentativa — a
    /// conclusão relevante ("BLOCKED") é a mesma nos dois casos. Ver
    /// `seccomp_preload_blocks_execve_of_a_binary_written_inside_the_sandboxed_tmpfs`
    /// (T007, abaixo) para o teste que isola especificamente o mecanismo
    /// seccomp, provando que é ele (e não a ausência de bind) que bloqueia.
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
                 except (FileNotFoundError, PermissionError):\n\
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

    /// [US1, T007] Garante que `libfarol_seccomp_preload.so` está compilado
    /// antes de um teste de integração que dependa da mediação real de
    /// `exec` via seccomp rodar — evita que o resultado do teste dependa da
    /// ordem de execução ou de um `cargo build -p farol-seccomp-preload`
    /// manual prévio. Idempotente: só dispara o build se
    /// `sandbox_seccomp::locate_seccomp_preload_library()` ainda não
    /// encontrar o artefato; devolve o caminho localizado (usado pelos
    /// testes para comparar contra `--setenv LD_PRELOAD`).
    fn ensure_seccomp_preload_library_is_built() -> PathBuf {
        if let Ok(path) = crate::sandbox_seccomp::locate_seccomp_preload_library() {
            return path;
        }

        let status = Command::new("cargo")
            .args(["build", "-p", "farol-seccomp-preload"])
            .status()
            .expect("cargo MUST estar disponível para compilar farol-seccomp-preload");
        assert!(
            status.success(),
            "`cargo build -p farol-seccomp-preload` falhou — não é possível testar a mediação \
             real de exec via seccomp sem o .so compilado"
        );

        crate::sandbox_seccomp::locate_seccomp_preload_library().expect(
            "libfarol_seccomp_preload.so ainda não encontrado depois do build — verifique o \
             nome do artefato/target dir (sandbox_seccomp::locate_seccomp_preload_library)",
        )
    }

    /// [US1, T007] Prova que É O FILTRO SECCOMP (via `LD_PRELOAD`), não a
    /// ausência de bind de `/usr/bin`, que bloqueia `exec` sob `allow_exec =
    /// false` — diferente de `exec_denied_blocks_external_binary` (acima),
    /// que só prova que um binário do HOST nunca bindado (`/usr/bin/true`)
    /// não existe do ponto de vista do sandbox (mediação da feature 006,
    /// visibilidade de filesystem). Aqui o plugin escreve um binário
    /// executável DENTRO do `tmpfs` gravável que ele já enxerga (`--tmpfs
    /// /tmp`, passo 2 de `build_bwrap_args`) e tenta executá-lo por caminho
    /// absoluto: o arquivo existe de fato, tem permissão de execução, e
    /// mesmo assim a tentativa de `execve` MUST falhar, porque o filtro
    /// seccomp aplicado pelo `.so` de `farol-seccomp-preload` (via
    /// `LD_PRELOAD`) intercepta a própria syscall antes de o kernel sequer
    /// tentar resolver o shebang/interpretador do arquivo.
    #[test]
    fn seccomp_preload_blocks_execve_of_a_binary_written_inside_the_sandboxed_tmpfs() {
        ensure_seccomp_preload_library_is_built();

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
                "import os, subprocess\n\
                 payload = '/tmp/farol-sandbox-seccomp-payload'\n\
                 with open(payload, 'w') as f:\n\
                 \tf.write('#!/bin/sh\\necho SHOULD_NOT_RUN\\n')\n\
                 os.chmod(payload, 0o755)\n\
                 try:\n\
                 \tsubprocess.run([payload], check=True)\n\
                 \tprint('LEAKED')\n\
                 except (PermissionError, OSError):\n\
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
            "esperava BLOCKED — o filtro seccomp (LD_PRELOAD de farol-seccomp-preload) deve \
             negar o execve do binário escrito dentro do tmpfs gravável do sandbox, mesmo com \
             o arquivo existindo e executável de verdade (issue #12); \
             stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(
            !stdout.contains("LEAKED"),
            "NUNCA deveria conseguir executar um binário escrito dentro do tmpfs sob \
             allow_exec=false; stdout={stdout:?}"
        );
    }

    /// [US1, T007] Segundo cenário exigido pela task: operação normal do
    /// plugin (sem nenhuma tentativa de `execve`/`execveat`) continua
    /// funcionando sem erro com o filtro seccomp ativo — o filtro só nega
    /// `exec`, não deve interferir em nada mais do processo.
    #[test]
    fn seccomp_preload_does_not_break_normal_plugin_operation_without_exec() {
        ensure_seccomp_preload_library_is_built();

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
            &["-c".to_string(), "print('OK')\n".to_string()],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(
            output.status.success(),
            "operação normal do plugin (sem exec) NÃO deveria falhar com o filtro seccomp \
             (LD_PRELOAD) ativo; stdout={stdout:?} stderr={:?}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(stdout.contains("OK"), "stdout={stdout:?}");
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
    /// mas atrelado à configuração real do plugin. Mesma nota sobre
    /// `PermissionError`/`FileNotFoundError` desde a feature
    /// `009-sandbox-hardening` (T005) se aplica aqui — ver doc daquele teste.
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
                 except (FileNotFoundError, PermissionError):\n\
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
            ("GIT_CONFIG_GLOBAL", "/dev/null"),
            ("GIT_CONFIG_SYSTEM", "/dev/null"),
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

    /// [US2, T013] Script Python auto-contido que roda DENTRO do sandbox e
    /// prova, num único processo, a allowlist de rede por host: como o
    /// mecanismo desta feature (D3/FR-008, ver doc de `sandbox_network`)
    /// exige um namespace de rede PRIVADO recém-criado pelo próprio `bwrap`
    /// (sem `--share-net`) para que `--cap-add CAP_NET_ADMIN` seja efetivo,
    /// esse namespace enxerga só `lo` — nenhum `TcpListener` do processo de
    /// teste (fora do sandbox) seria alcançável de dentro dele, diferente de
    /// `uptime_kuma_real_profile_allows_local_tcp_connection` (que depende
    /// de `--share-net`). Por isso os dois "servidores de fixture" exigidos
    /// pela task (host declarado vs. não declarado) sobem como threads DENTRO
    /// do próprio script sandboxado, em `127.0.0.1` (declarado na allowlist)
    /// e `127.0.0.2` (não declarado) — o range inteiro `127.0.0.0/8` fica
    /// disponível via `lo` mesmo num netns privado, uma vez que a interface é
    /// levantada pelo wrapper de T010 (`ip link set lo up`). Cada listener
    /// usa porta efêmera (`bind(ip, 0)`), lida no mesmo processo via
    /// `getsockname()`, então não há nenhuma coordenação entre processos.
    const NETWORK_ALLOWLIST_FIXTURE_SCRIPT: &str = "import socket, threading, time\n\
         def serve(bind_ip):\n\
         \tsrv = socket.socket(socket.AF_INET, socket.SOCK_STREAM)\n\
         \tsrv.setsockopt(socket.SOL_SOCKET, socket.SO_REUSEADDR, 1)\n\
         \tsrv.bind((bind_ip, 0))\n\
         \tport = srv.getsockname()[1]\n\
         \tsrv.listen(1)\n\
         \tsrv.settimeout(5)\n\
         \tdef accept_once():\n\
         \t\ttry:\n\
         \t\t\tconn, _ = srv.accept()\n\
         \t\t\tconn.close()\n\
         \t\texcept socket.timeout:\n\
         \t\t\tpass\n\
         \t\tfinally:\n\
         \t\t\tsrv.close()\n\
         \tthreading.Thread(target=accept_once, daemon=True).start()\n\
         \treturn port\n\
         declared_port = serve('127.0.0.1')\n\
         undeclared_port = serve('127.0.0.2')\n\
         time.sleep(0.3)\n\
         try:\n\
         \ts = socket.create_connection(('127.0.0.1', declared_port), timeout=3)\n\
         \ts.close()\n\
         \tprint('DECLARED_OK')\n\
         except OSError:\n\
         \tprint('DECLARED_BLOCKED')\n\
         try:\n\
         \ts = socket.create_connection(('127.0.0.2', undeclared_port), timeout=3)\n\
         \ts.close()\n\
         \tprint('UNDECLARED_LEAKED')\n\
         except OSError:\n\
         \tprint('UNDECLARED_BLOCKED')\n";

    /// [US2, T013] Cenário principal exigido pela task: plugin com allowlist
    /// declarando um host consegue conectar a ele e falha ao conectar a
    /// outro. Perfil sintético `allow_network=true, allow_exec=true`, com
    /// `network_hosts=["127.0.0.1"]` — só `127.0.0.1` (o host "declarado")
    /// deve ser alcançável; `127.0.0.2` (não declarado) deve ser bloqueado
    /// pelas regras `nft`/`iptables` aplicadas pelo wrapper de T010. Depende
    /// de `nft` ou `iptables` estarem instalados nesta máquina (Assumptions
    /// de `spec.md`), além de `bwrap`.
    #[test]
    fn network_hosts_allowlist_permits_declared_host_and_blocks_undeclared_host() {
        let interpreter = python3_path();
        let profile = SandboxProfile {
            allow_network: true,
            allow_exec: true,
            extra_binds: vec![],
        };
        let args = build_bwrap_args_with_network_hosts(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &["-c".to_string(), NETWORK_ALLOWLIST_FIXTURE_SCRIPT.to_string()],
            &["127.0.0.1".to_string()],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("DECLARED_OK"),
            "esperava que a conexão ao host declarado (127.0.0.1) funcionasse; \
             stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stdout.contains("UNDECLARED_BLOCKED"),
            "esperava que a conexão ao host NÃO declarado (127.0.0.2) fosse bloqueada pela \
             allowlist de rede (T009-T011); stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            !stdout.contains("UNDECLARED_LEAKED"),
            "NUNCA deveria conseguir conectar a um host fora da allowlist declarada; \
             stdout={stdout:?}"
        );
    }

    /// [US2, T013] Cenário combinado: prova que a correção do bug "wrapper
    /// bloqueia o próprio exec" (`export LD_PRELOAD` adiado para dentro do
    /// script do wrapper, ver doc de `sandbox_network::build_wrapper_script`)
    /// funciona de ponta a ponta com `bwrap` real — perfil real de
    /// `uptime-kuma` teria `allow_exec=false`; este teste usa um perfil
    /// sintético equivalente (`allow_network=true, allow_exec=false`) com um
    /// host declarado, confirmando DUAS coisas ao mesmo tempo no mesmo
    /// processo sandboxado: (1) a allowlist de rede continua funcionando
    /// (prova que o wrapper conseguiu aplicar `nft`/`iptables` e fazer seu
    /// próprio `exec` final do interpretador, mesmo com `LD_PRELOAD`
    /// endereçado); (2) o filtro seccomp de US1 continua ativo no processo
    /// final do plugin (prova que o `LD_PRELOAD` adiado realmente chegou ao
    /// interpretador, não se perdeu) — sem essa combinação, um regression no
    /// meio do caminho poderia silenciosamente desativar US1 OU US2 sem que
    /// nenhum teste existente (que testa os dois mecanismos separadamente)
    /// percebesse.
    #[test]
    fn network_hosts_allowlist_combined_with_exec_denied_still_blocks_exec_and_reaches_declared_host()
    {
        ensure_seccomp_preload_library_is_built();

        let interpreter = python3_path();
        let profile = SandboxProfile {
            allow_network: true,
            allow_exec: false,
            extra_binds: vec![],
        };
        let script = format!(
            "{NETWORK_ALLOWLIST_FIXTURE_SCRIPT}\
             import os, subprocess\n\
             payload = '/tmp/farol-sandbox-network-seccomp-payload'\n\
             with open(payload, 'w') as f:\n\
             \tf.write('#!/bin/sh\\necho SHOULD_NOT_RUN\\n')\n\
             os.chmod(payload, 0o755)\n\
             try:\n\
             \tsubprocess.run([payload], check=True)\n\
             \tprint('EXEC_LEAKED')\n\
             except (PermissionError, OSError):\n\
             \tprint('EXEC_BLOCKED')\n"
        );
        let args = build_bwrap_args_with_network_hosts(
            &repo_root(),
            &interpreter,
            &profile,
            "python3",
            &["-c".to_string(), script],
            &["127.0.0.1".to_string()],
        );

        let output = Command::new("bwrap")
            .args(&args)
            .output()
            .expect("bwrap MUST estar instalado e executável nesta máquina");

        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stdout.contains("DECLARED_OK"),
            "com allow_exec=false E allowlist de rede juntos, a conexão ao host declarado ainda \
             deveria funcionar (prova que o wrapper conseguiu exec'ar o interpretador real \
             mesmo com o LD_PRELOAD endereçado); stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stdout.contains("UNDECLARED_BLOCKED"),
            "allowlist de rede deveria continuar bloqueando o host não declarado mesmo \
             combinada com allow_exec=false; stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            stdout.contains("EXEC_BLOCKED"),
            "o filtro seccomp de US1 (LD_PRELOAD) deveria continuar ativo no processo final do \
             plugin mesmo quando adiado para dentro do script do wrapper de rede — prova que o \
             LD_PRELOAD adiado (fix desta sessão) realmente chega ao interpretador; \
             stdout={stdout:?} stderr={stderr:?}"
        );
        assert!(
            !stdout.contains("EXEC_LEAKED") && !stdout.contains("UNDECLARED_LEAKED"),
            "nenhum dos dois mecanismos (US1 seccomp, US2 rede) deveria vazar quando \
             combinados; stdout={stdout:?}"
        );
    }
}
