//! Allowlist de rede por host via `nftables`/`iptables` dentro do namespace
//! de rede do sandbox (feature `009-sandbox-hardening`, User Story 2, issue
//! #13).
//!
//! # Desvio deliberado do texto literal de T011 (`tasks.md`) — "preservando
//! `--share-net`"
//!
//! `tasks.md` (T011) pede, em prosa curta, para "usar o wrapper... enquanto
//! preserva o `--share-net`". Isso conflita com D3/FR-008 (`plan.md`/
//! `spec.md`), que são explícitos: o processo auxiliar que aplica as regras
//! roda "DENTRO do namespace de rede **recém-criado** pelo `bwrap`" (D3) —
//! FR-008 repete "dentro do namespace de rede criado pelo `bwrap`". Um
//! namespace de rede "recém-criado"/"criado pelo bwrap" só existe quando
//! `--share-net` **não** é passado (`--share-net` reusa o namespace do host,
//! não cria nenhum). Empiricamente (testado extensivamente nesta sessão, com
//! `capsh --print`, em várias combinações de `--uid 0`/`--cap-add
//! CAP_NET_ADMIN`, com e sem `--share-net`): com `--share-net`, nenhuma
//! combinação de flags concede `CAP_NET_ADMIN` efetivo sobre o namespace de
//! rede real do host — porque esse namespace é dono por `init_user_ns` (ou
//! um user namespace ancestral), não pelo user namespace novo e não
//! privilegiado que o próprio `bwrap` cria; capacidades concedidas dentro
//! desse user namespace novo não se aplicam a um namespace de rede
//! *estrangeiro* que ele não é dono. Sem `--share-net` (namespace de rede
//! criado do zero pelo próprio `bwrap`, e portanto de propriedade do user
//! namespace que o `bwrap` também acabou de criar), `--uid 0 --gid 0
//! --cap-add CAP_NET_ADMIN` concede `CAP_NET_ADMIN` efetivo de verdade
//! (confirmado com `capsh --print` mostrando o capability set correto).
//!
//! Conclusão adotada aqui (mesma classe de revisão empírica já documentada
//! para D1 em `sandbox_seccomp.rs`/`plan.md`): quando `allow_network=true` e
//! há hosts declarados, [`crate::sandbox::build_bwrap_args_with_network_hosts`]
//! usa um namespace de rede PRIVADO (nunca passa `--share-net`) + `--uid 0
//! --gid 0 --cap-add CAP_NET_ADMIN`, em vez de preservar `--share-net`
//! literalmente como o resumo de T011 sugere — a leitura de D3/FR-008 é a
//! autoridade normativa aqui, não a paráfrase curta de `tasks.md`.
//!
//! # Limitação séria e deliberadamente não resolvida nesta rodada
//!
//! Um namespace de rede privado criado só por `--unshare-all` (sem
//! `--share-net`) não tem NENHUMA interface além de `lo` — sem
//! `slirp4netns`/veth+NAT (nova dependência de sistema/infra, fora do
//! escopo desta feature: `plan.md` só autoriza `nft`/`iptables` como nova
//! dependência de sistema), esse namespace não tem rota nenhuma para
//! nenhum host externo real, declarado ou não. Isso significa que, do jeito
//! que este módulo está implementado, a allowlist por host aplicada aqui só
//! é observável para destinos dentro do próprio `127.0.0.0/8` (o range
//! inteiro de loopback fica disponível uma vez que `lo` é levantado, mesmo
//! num namespace privado — validado empiricamente nesta sessão). Um plugin
//! real declarando um host de internet de verdade (ex. `api.github.com`)
//! ficaria sem NENHUMA conectividade de rede sob este mecanismo, mesmo para
//! o host declarado — regressão funcional grave frente ao `--share-net`
//! atual. Ver `Riscos/pendências` no relatório final desta subtarefa: exige
//! uma decisão de arquitetura (aceitar a limitação, ou introduzir
//! `slirp4netns`/equivalente como nova dependência) antes de esta allowlist
//! poder ser ligada a um plugin real via `plugin_worker.rs` (T009-T014
//! cobrem só o mecanismo dentro de `sandbox.rs`/`sandbox_network.rs`; essa
//! integração é explicitamente Fora de Escopo desta subtarefa).
//!
//! # Por que `SandboxProfile` NÃO ganhou um campo `network_hosts`
//!
//! O design original desta task delegava a mim escolher como
//! [`crate::sandbox::build_bwrap_args`] passaria a distinguir "network=true
//! sem allowlist" de "network=true com allowlist", sugerindo como exemplo
//! um novo campo em `SandboxProfile`. Isso é **incompatível** com a
//! restrição de não tocar `plugin_worker.rs`: esse arquivo constrói
//! `SandboxProfile { allow_network, allow_exec, extra_binds }` cinco vezes,
//! sem nenhum `..Default::default()`, e chama `build_bwrap_args(...)` com 5
//! argumentos posicionais — em Rust estável (confirmado nesta sessão:
//! `rustc 1.98.0`, `default_field_values`/RFC 3681 ainda é
//! `#[cfg(feature)]`-gated/nightly, `E0658` ao testar em um crate
//! descartável), um novo campo obrigatório ou um novo parâmetro posicional
//! QUEBRARIA a compilação desses 5 call sites, que estou proibido de editar.
//! Solução adotada: `SandboxProfile` permanece com seus 3 campos originais
//! (nenhuma mudança, nenhum risco de quebra); a lista de hosts é passada
//! como um parâmetro adicional e explícito
//! (`network_hosts: &[String]`) numa NOVA função,
//! [`crate::sandbox::build_bwrap_args_with_network_hosts`], que
//! `build_bwrap_args` (assinatura pública original, intocada) simplesmente
//! chama com `&[]` — 100% aditivo, sem nenhum call site quebrado. A
//! integração real (extrair hosts de `KnownCapability::Network` do
//! manifesto de um plugin e popular esse parâmetro a partir de
//! `plugin_worker.rs`) continua Fora de Escopo desta subtarefa, exatamente
//! como já declarado nas instruções originais.

use std::net::ToSocketAddrs;
use std::path::{Path, PathBuf};

use crate::sandbox::SandboxMountError;

/// Escape-hatch só de teste (mesmo padrão de
/// `sandbox_seccomp::FORCE_LIBRARY_MISSING_ENV_VAR`): quando definida com um
/// valor não vazio, força [`detect_firewall_tool`] a falhar como se nem
/// `nft` nem `iptables` estivessem disponíveis neste sistema — usada só por
/// `sandbox::sandbox_unit_tests`/testes deste módulo para exercitar o
/// caminho fail-closed (T014, FR-004 por analogia) sem precisar desinstalar
/// de verdade `nftables`/`iptables` da máquina de desenvolvimento.
pub(crate) const FORCE_FIREWALL_MISSING_ENV_VAR: &str =
    "FAROL_SANDBOX_TEST_FORCE_NETWORK_FIREWALL_MISSING";

/// Lock compartilhado para serializar qualquer teste (deste módulo ou de
/// `sandbox::sandbox_unit_tests`) que mute [`FORCE_FIREWALL_MISSING_ENV_VAR`]
/// — mesmo raciocínio do lock equivalente em `sandbox_seccomp.rs` (doc lá
/// explica por quê precisa ser um único `Mutex` visível dos dois lugares).
#[cfg(test)]
pub(crate) static FORCE_FIREWALL_MISSING_ENV_VAR_TEST_LOCK: std::sync::Mutex<()> =
    std::sync::Mutex::new(());

fn forced_missing_reason() -> Option<String> {
    let value = std::env::var_os(FORCE_FIREWALL_MISSING_ENV_VAR)?;
    if value.is_empty() {
        return None;
    }
    Some(format!(
        "{FORCE_FIREWALL_MISSING_ENV_VAR} definida — indisponibilidade simulada só de teste \
         (FR-004 por analogia, ver Assumptions de spec.md), nenhum problema real neste sistema"
    ))
}

/// T009: resolução de host→IP no processo pai (fora do sandbox), antes de
/// montar os argumentos do `bwrap` (D4 do `plan.md`) — os IPs resolvidos
/// (nunca os hostnames originais) é que entram na regra de firewall
/// aplicada dentro do namespace do sandbox.
///
/// Cada entrada de `hosts` que já é um literal de IP (v4 ou v6) passa direto
/// (sem tocar em DNS); caso contrário, resolve via
/// `ToSocketAddrs` (mesmo resolver do sistema usado por qualquer código Rust
/// que abra um `TcpStream`). Deduplica IPs repetidos entre hosts diferentes.
///
/// Fail-closed: um host que não resolve para nenhum IP (edge case de
/// `spec.md`: "host declarado não resolve via DNS no momento em que o
/// sandbox é montado") devolve `Err` em vez de montar uma allowlist
/// incompleta silenciosamente — decisão de design tomada aqui porque
/// `spec.md` levanta esse cenário só como pergunta em "Edge Cases", sem uma
/// FR normativa resolvendo-o; dado o restante da feature ser
/// deliberadamente fail-closed (FR-004), a mesma filosofia foi replicada
/// aqui por consistência.
pub(crate) fn resolve_hosts_to_ips(hosts: &[String]) -> Result<Vec<String>, SandboxMountError> {
    let mut ips: Vec<String> = Vec::new();

    for host in hosts {
        if let Ok(ip) = host.parse::<std::net::IpAddr>() {
            push_unique(&mut ips, ip.to_string());
            continue;
        }

        let addrs = (host.as_str(), 0u16).to_socket_addrs().map_err(|err| {
            SandboxMountError::NetworkFirewallUnavailable(format!(
                "falha ao resolver host {host:?} declarado na allowlist de rede: {err} — a \
                 allowlist não pode ser aplicada com segurança sem o IP resolvido (FR-009)"
            ))
        })?;

        let mut resolved_any = false;
        for addr in addrs {
            push_unique(&mut ips, addr.ip().to_string());
            resolved_any = true;
        }

        if !resolved_any {
            return Err(SandboxMountError::NetworkFirewallUnavailable(format!(
                "host {host:?} declarado na allowlist de rede não resolveu para nenhum IP — a \
                 allowlist não pode ser aplicada com segurança sem o IP resolvido (FR-009)"
            )));
        }
    }

    Ok(ips)
}

fn push_unique(ips: &mut Vec<String>, ip: String) {
    if !ips.contains(&ip) {
        ips.push(ip);
    }
}

/// Ferramenta de firewall escolhida para aplicar a allowlist (T010): `nft`
/// é preferido; `iptables` é o fallback quando `nft` está ausente do `PATH`
/// (FR-008). Guarda o caminho já canonicalizado
/// (`std::fs::canonicalize`) do binário — necessário porque, no Fedora,
/// `/usr/bin/iptables` é um symlink para `/etc/alternatives/iptables`, que
/// por sua vez aponta para `/usr/bin/xtables-nft-multi`; `/etc` não é
/// bindado dentro do sandbox, então o caminho original (`/usr/bin/iptables`)
/// não resolve de dentro do sandbox — só o alvo final canonicalizado, que já
/// vive dentro de `/usr` (bindado). Quando o nome final canonicalizado
/// difere de `"iptables"` (caso `xtables-nft-multi`, um binário
/// "multi-call" no estilo busybox), `dispatch_arg` carrega `"iptables"` para
/// ser passado como primeiro argumento (`<binário> iptables -A ...`),
/// explorando o despacho por argumento desse binário — validado
/// manualmente nesta sessão (`IPTABLES_RULES_OK`).
#[derive(Debug)]
pub(crate) enum FirewallTool {
    Nft { binary: PathBuf },
    Iptables { binary: PathBuf, dispatch_arg: Option<&'static str> },
}

/// T010 (parte 1) + T014: localiza `nft` (preferido) ou `iptables`
/// (fallback) no `PATH` do processo do Farol — mesmo `PATH` que
/// [`crate::sandbox::resolve_interpreter_path`] já usa para o interpretador
/// do plugin. Fail-closed (FR-004 por analogia, `Assumptions` de
/// `spec.md`): `Err(SandboxMountError::NetworkFirewallUnavailable)` quando
/// nenhum dos dois está disponível — nunca um fallback silencioso para o
/// comportamento antigo de liga/desliga total.
pub(crate) fn detect_firewall_tool() -> Result<FirewallTool, SandboxMountError> {
    if let Some(reason) = forced_missing_reason() {
        return Err(SandboxMountError::NetworkFirewallUnavailable(reason));
    }

    if let Some(nft_path) = crate::sandbox::resolve_interpreter_path("nft") {
        let binary = std::fs::canonicalize(&nft_path).unwrap_or(nft_path);
        return Ok(FirewallTool::Nft { binary });
    }

    if let Some(iptables_path) = crate::sandbox::resolve_interpreter_path("iptables") {
        let binary = std::fs::canonicalize(&iptables_path).unwrap_or_else(|_| iptables_path.clone());
        let dispatch_arg = match binary.file_name().and_then(|name| name.to_str()) {
            Some("iptables") => None,
            _ => Some("iptables"),
        };
        return Ok(FirewallTool::Iptables { binary, dispatch_arg });
    }

    Err(SandboxMountError::NetworkFirewallUnavailable(
        "nem 'nft' (nftables) nem 'iptables' encontrados no PATH — nenhum dos dois mecanismos \
         de allowlist de rede por host (FR-008) pode ser aplicado neste sistema; instale \
         nftables (preferencial) ou iptables antes de declarar hosts na capability network de \
         um plugin (Assumptions de spec.md)"
            .to_string(),
    ))
}

/// Resultado de [`plan_network_wrapper`] (T009+T010+T011): as duas partes
/// que `sandbox::build_bwrap_args_with_network_hosts` precisa emendar nos
/// argumentos do `bwrap` — os binds read-only das ferramentas que o
/// wrapper precisa (`sh`, a ferramenta de firewall escolhida, e `ip` quando
/// disponível para levantar `lo`), e o comando final completo (`sh -c
/// <script> <interpreter_path> <args...>`) que substitui o exec direto do
/// interpretador do plugin.
pub(crate) struct NetworkWrapperOutcome {
    pub(crate) tool_binds: Vec<String>,
    pub(crate) final_command: Vec<String>,
}

/// T009+T010+T011: monta o plano completo do wrapper de allowlist de rede —
/// resolve os hosts para IP (T009), detecta a ferramenta de firewall
/// disponível (T010/T014), gera o script inline que aplica as regras e
/// então `exec`a o comando real do plugin (T010), e devolve os binds
/// necessários dentro do sandbox para que esse script funcione.
///
/// `ld_preload_path`, quando `Some` (i.e., `allow_exec=false` no profile do
/// plugin — combinação real de `uptime-kuma` hoje), MUST ser aplicado
/// (`export LD_PRELOAD=...`) só dentro do script, IMEDIATAMENTE antes do
/// `exec` final — nunca via `--setenv LD_PRELOAD` do próprio `bwrap`, que
/// vazaria para o AMBIENTE DO PRÓPRIO WRAPPER: o construtor ELF do `.so` de
/// `farol-seccomp-preload` aplicaria o filtro seccomp de bloqueio de
/// `execve` no processo do WRAPPER (o processo que o `bwrap` de fato
/// `exec`a primeiro), antes mesmo de ele conseguir chamar `nft`/`ip` ou
/// fazer o `exec` final do interpretador do plugin — mesma classe de bug já
/// documentada e corrigida como D1 (`bwrap --seccomp FD` bloqueando o
/// `execvp` interno do próprio `bwrap`), um nível acima: aqui é o `execve`
/// interno do WRAPPER (não mais do `bwrap`) que ficaria bloqueado pelo seu
/// próprio filtro antes da hora.
pub(crate) fn plan_network_wrapper(
    hosts: &[String],
    ld_preload_path: Option<&Path>,
    interpreter_path: &Path,
    args: &[String],
) -> Result<NetworkWrapperOutcome, SandboxMountError> {
    let resolved_ips = resolve_hosts_to_ips(hosts)?;
    let tool = detect_firewall_tool()?;

    let sh_path = crate::sandbox::resolve_interpreter_path("sh").ok_or_else(|| {
        SandboxMountError::NetworkFirewallUnavailable(
            "interpretador 'sh' não encontrado no PATH — necessário para aplicar o wrapper de \
             allowlist de rede (T010) antes de exec'ar o comando real do plugin"
                .to_string(),
        )
    })?;
    let ip_tool_path =
        crate::sandbox::resolve_interpreter_path("ip").and_then(|p| std::fs::canonicalize(&p).ok());

    // Binda cada ferramenta do wrapper num destino DEDICADO
    // (`WRAPPER_TOOLS_DEST_DIR`), nunca no próprio caminho de origem do
    // host: quando `allow_exec=true` no profile, o bloco anterior de
    // `build_bwrap_args_with_network_hosts` já bind-monta `/usr/bin` (e
    // `/bin`) inteiros dentro do sandbox — e em várias distros (esta
    // máquina de desenvolvimento incluída) `/usr/bin/sh` é um SYMLINK
    // (ex.: para `bash`). Tentar `--ro-bind <sh real> /usr/bin/sh` por
    // cima de um destino que já resolve, através desse bind anterior, para
    // um symlink faz o `bwrap` falhar com `Can't mount on symlink
    // destination` (achado empírico desta sessão, via
    // `network_hosts_allowlist_permits_declared_host_and_blocks_undeclared_host`).
    // Usar um destino que nenhum outro bind desta função toca evita esse
    // conflito inteiramente, independente da ordem entre os blocos (D5) e
    // independente de `allow_exec` ser `true` ou `false`.
    let sh_dest = PathBuf::from(WRAPPER_TOOLS_DEST_DIR).join("sh");
    let fw_dest = PathBuf::from(WRAPPER_TOOLS_DEST_DIR).join("fw");
    let ip_dest = PathBuf::from(WRAPPER_TOOLS_DEST_DIR).join("ip");

    let mut tool_binds: Vec<String> = Vec::new();
    push_ro_bind(&mut tool_binds, &sh_path, &sh_dest);
    let sandboxed_tool = match &tool {
        FirewallTool::Nft { binary } => {
            push_ro_bind(&mut tool_binds, binary, &fw_dest);
            FirewallTool::Nft { binary: fw_dest.clone() }
        }
        FirewallTool::Iptables { binary, dispatch_arg } => {
            push_ro_bind(&mut tool_binds, binary, &fw_dest);
            FirewallTool::Iptables { binary: fw_dest.clone(), dispatch_arg: *dispatch_arg }
        }
    };
    let sandboxed_ip_path = if let Some(ip_bin) = &ip_tool_path {
        push_ro_bind(&mut tool_binds, ip_bin, &ip_dest);
        Some(ip_dest.clone())
    } else {
        None
    };

    let script = build_wrapper_script(
        &resolved_ips,
        &sandboxed_tool,
        sandboxed_ip_path.as_deref(),
        ld_preload_path,
    );

    let mut final_command = vec![
        sh_dest.to_string_lossy().into_owned(),
        "-c".to_string(),
        script,
        interpreter_path.to_string_lossy().into_owned(),
    ];
    final_command.extend(args.iter().cloned());

    Ok(NetworkWrapperOutcome { tool_binds, final_command })
}

/// Diretório dedicado, dentro do sandbox, para os binds das ferramentas do
/// wrapper de rede (`sh`, `nft`/`iptables`, `ip`) — nunca sob `/usr/bin`,
/// `/bin` ou qualquer outro prefixo que outro bloco de
/// `build_bwrap_args_with_network_hosts` também possa montar (ver doc de
/// [`plan_network_wrapper`]). Nada mais neste módulo ou em `sandbox.rs`
/// monta nada sob `/run`, então esses três binds nunca colidem com nenhum
/// outro, em nenhuma ordem/combinação de `allow_exec`.
const WRAPPER_TOOLS_DEST_DIR: &str = "/run/farol-network-wrapper";

fn push_ro_bind(out: &mut Vec<String>, src: &Path, dest: &Path) {
    out.push("--ro-bind".to_string());
    out.push(src.to_string_lossy().into_owned());
    out.push(dest.to_string_lossy().into_owned());
}

/// T010: gera o script `sh` inline (string Rust, sem binário/passo de build
/// extra — `plan.md` § Complexity Tracking) que: (1) levanta `lo` (best
/// effort — sem isso a allowlist ainda é aplicada, só não haveria como
/// alcançar nem os IPs declarados dentro do namespace de rede privado); (2)
/// aplica a regra de firewall restringindo egress aos `ips` resolvidos,
/// com política padrão de DROP e aceite de `established,related` (para o
/// tráfego de retorno das conexões permitidas); (3) exporta `LD_PRELOAD`
/// (quando aplicável) só agora, imediatamente antes do `exec` final — nunca
/// antes, pelo motivo documentado em [`plan_network_wrapper`]; (4) faz
/// `exec "$0" "$@"` do comando real do plugin — `$0`/`$@` vêm dos
/// argumentos posicionais passados a `sh -c <script> <interpreter> <args...>`
/// (convenção POSIX: o primeiro argv depois do próprio script vira `$0`,
/// nunca entra em `"$@"`).
fn build_wrapper_script(
    ips: &[String],
    tool: &FirewallTool,
    ip_tool_path: Option<&Path>,
    ld_preload_path: Option<&Path>,
) -> String {
    let mut script = String::new();
    script.push_str("set -e\n");

    if let Some(ip_bin) = ip_tool_path {
        script.push_str(&shell_quote(&ip_bin.to_string_lossy()));
        script.push_str(" link set lo up 2>/dev/null || true\n");
    }

    let (v4_ips, v6_ips): (Vec<&String>, Vec<&String>) =
        ips.iter().partition(|ip| ip.parse::<std::net::Ipv4Addr>().is_ok());

    match tool {
        FirewallTool::Nft { binary } => {
            script.push_str(&shell_quote(&binary.to_string_lossy()));
            script.push_str(" -f - <<'FAROL_NFT_RULES_EOF'\n");
            script.push_str("table inet farol_sandbox {\n");
            script.push_str("  chain output {\n");
            script.push_str("    type filter hook output priority 0; policy drop;\n");
            script.push_str("    ct state established,related accept;\n");
            if !v4_ips.is_empty() {
                script.push_str(&format!(
                    "    ip daddr {{ {} }} accept\n",
                    v4_ips.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
            if !v6_ips.is_empty() {
                script.push_str(&format!(
                    "    ip6 daddr {{ {} }} accept\n",
                    v6_ips.iter().map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
                ));
            }
            script.push_str("  }\n");
            script.push_str("}\n");
            script.push_str("FAROL_NFT_RULES_EOF\n");
        }
        FirewallTool::Iptables { binary, dispatch_arg } => {
            let base = match dispatch_arg {
                Some(prefix) => format!("{} {prefix}", shell_quote(&binary.to_string_lossy())),
                None => shell_quote(&binary.to_string_lossy()),
            };
            script.push_str(&format!("{base} -P OUTPUT DROP\n"));
            script.push_str(&format!(
                "{base} -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT\n"
            ));
            for ip in &v4_ips {
                script.push_str(&format!("{base} -A OUTPUT -d {ip} -j ACCEPT\n"));
            }
            if !v6_ips.is_empty() {
                // Nunca colar "6" no fim do comando `iptables` já resolvido
                // (bug corrigido nesta revisão: isso produzia um comando
                // inválido, ex. `'/usr/bin/xtables-nft-multi'6`, que o
                // `sh` do wrapper tentaria executar como um único caminho
                // literal inexistente). `ip6tables` é um APPLET/BINÁRIO
                // DISTINTO: no caso "multi-call" (`dispatch_arg` presente),
                // é o mesmo binário despachado com o applet
                // `"ip6tables"` em vez de `"iptables"`; no caso de um
                // binário dedicado de verdade (`dispatch_arg: None`),
                // `ip6tables` é outro arquivo, convencionalmente no mesmo
                // diretório do `iptables` resolvido (mesmo padrão de
                // instalação observado em toda distro Linux comum).
                let base6 = match dispatch_arg {
                    Some(_) => {
                        format!("{} ip6tables", shell_quote(&binary.to_string_lossy()))
                    }
                    None => {
                        let sibling = binary
                            .parent()
                            .map(|dir| dir.join("ip6tables"))
                            .unwrap_or_else(|| PathBuf::from("ip6tables"));
                        shell_quote(&sibling.to_string_lossy())
                    }
                };
                script.push_str(&format!("{base6} -P OUTPUT DROP\n"));
                script.push_str(&format!(
                    "{base6} -A OUTPUT -m state --state ESTABLISHED,RELATED -j ACCEPT\n"
                ));
                for ip in &v6_ips {
                    script.push_str(&format!("{base6} -A OUTPUT -d {ip} -j ACCEPT\n"));
                }
            }
        }
    }

    if let Some(preload) = ld_preload_path {
        script.push_str("export LD_PRELOAD=");
        script.push_str(&shell_quote(&preload.to_string_lossy()));
        script.push('\n');
    }

    script.push_str("exec \"$0\" \"$@\"\n");
    script
}

/// Escapa um valor para uso seguro entre aspas simples num script `sh`
/// (`'`→`'\''`, a técnica POSIX padrão) — usado para todo caminho de host
/// (interpretador de ferramentas, `.so` do preload) embutido no script
/// gerado, já que esses caminhos vêm do sistema de arquivos real e não são
/// controlados por este código.
fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_hosts_to_ips_passes_through_ip_literals_without_dns() {
        let ips = resolve_hosts_to_ips(&["127.0.0.1".to_string(), "127.0.0.2".to_string()])
            .expect("literais de IP não devem depender de DNS");
        assert_eq!(ips, vec!["127.0.0.1".to_string(), "127.0.0.2".to_string()]);
    }

    #[test]
    fn resolve_hosts_to_ips_deduplicates_repeated_ips() {
        let ips = resolve_hosts_to_ips(&["127.0.0.1".to_string(), "127.0.0.1".to_string()])
            .expect("IP literal não deve falhar");
        assert_eq!(ips, vec!["127.0.0.1".to_string()]);
    }

    #[test]
    fn resolve_hosts_to_ips_fails_closed_for_a_host_that_cannot_resolve() {
        // Domínio reservado por RFC 2606 para documentação/teste — nunca
        // resolve de verdade, então este teste não depende de nenhuma
        // condição de rede externa instável para ser determinístico.
        let result = resolve_hosts_to_ips(&["host-que-nao-existe.invalid".to_string()]);
        assert!(
            matches!(result, Err(SandboxMountError::NetworkFirewallUnavailable(_))),
            "esperava Err(NetworkFirewallUnavailable(_)) para host .invalid não resolvível; \
             got={result:?}"
        );
    }

    #[test]
    fn detect_firewall_tool_honors_the_test_only_force_missing_escape_hatch() {
        let _guard = FORCE_FIREWALL_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::set_var(FORCE_FIREWALL_MISSING_ENV_VAR, "1");
        let result = detect_firewall_tool();
        std::env::remove_var(FORCE_FIREWALL_MISSING_ENV_VAR);

        assert!(
            matches!(result, Err(SandboxMountError::NetworkFirewallUnavailable(_))),
            "esperava Err(NetworkFirewallUnavailable(_)) com o escape-hatch de teste ativo; \
             got={result:?}"
        );
    }

    /// Sem a variável de ambiente definida, o resultado depende só de `nft`/
    /// `iptables` estarem de fato instalados nesta máquina (Assumptions de
    /// `spec.md` — não é requisito desta feature detectar isso em toda
    /// máquina). Só confirma que o escape-hatch de teste não vaza para esta
    /// chamada por engano (mesmo padrão do teste equivalente em
    /// `sandbox_seccomp.rs`).
    #[test]
    fn detect_firewall_tool_without_the_escape_hatch_does_not_report_the_forced_reason() {
        let _guard = FORCE_FIREWALL_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::remove_var(FORCE_FIREWALL_MISSING_ENV_VAR);
        if let Err(SandboxMountError::NetworkFirewallUnavailable(detail)) = detect_firewall_tool() {
            assert!(
                !detail.contains("indisponibilidade simulada só de teste"),
                "sem o escape-hatch definido, um Err real não deveria carregar a mensagem do \
                 caminho simulado; detail={detail:?}"
            );
        }
    }

    #[test]
    fn build_wrapper_script_embeds_declared_ips_and_final_exec() {
        let tool = FirewallTool::Nft { binary: PathBuf::from("/usr/bin/nft") };
        let script = build_wrapper_script(
            &["127.0.0.1".to_string()],
            &tool,
            None,
            None,
        );

        assert!(script.contains("127.0.0.1"), "script={script:?}");
        assert!(script.contains("policy drop"), "script={script:?}");
        assert!(script.trim_end().ends_with("exec \"$0\" \"$@\""), "script={script:?}");
    }

    #[test]
    fn build_wrapper_script_defers_ld_preload_export_to_just_before_final_exec() {
        let tool = FirewallTool::Nft { binary: PathBuf::from("/usr/bin/nft") };
        let script = build_wrapper_script(
            &["127.0.0.1".to_string()],
            &tool,
            None,
            Some(Path::new("/repo/target/debug/libfarol_seccomp_preload.so")),
        );

        let export_index = script
            .find("export LD_PRELOAD=")
            .expect("script deveria conter export LD_PRELOAD=; script={script:?}");
        let exec_index = script
            .find("exec \"$0\" \"$@\"")
            .expect("script deveria conter o exec final");
        assert!(
            export_index < exec_index,
            "export LD_PRELOAD MUST vir antes do exec final, mas depois de tudo mais no script \
             (nunca via --setenv do bwrap, que vazaria pro próprio wrapper); script={script:?}"
        );
    }
}
