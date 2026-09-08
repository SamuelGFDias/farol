# Implementation Plan: Sandbox — Mediação Real de Exec via Seccomp e Allowlist de Rede por Host

**Branch**: `009-sandbox-hardening` | **Date**: 2026-09-07 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/009-sandbox-hardening/spec.md`

## Summary

Fecha o débito técnico das issues #12 e #13 (feature 006). `allow_exec=false` passa a ser garantido
por um filtro `seccomp` real (via crate `seccompiler`, gerado em Rust puro) aplicado via
`bwrap --seccomp FD`, bloqueando `execve`/`execveat`/`fexecve` no kernel — fechando o vetor hoje
documentado de escrever e executar um binário por caminho absoluto num `tmpfs` gravável.
`network=true` com hosts declarados passa a restringir o tráfego de saída aos IPs desses hosts via
regras `nftables` (fallback `iptables`) aplicadas dentro do namespace de rede que o `bwrap` já cria —
em vez do atual liga/desliga total via `--share-net`.

## Technical Context

**Language/Version**: Rust (workspace já em uso).

**Primary Dependencies**: NOVA dependência Rust `seccompiler` (crate pura, sem exigir
`libseccomp-dev` no sistema) em `crates/farol-core/Cargo.toml`, usada só para gerar o filtro BPF do
`exec`. NOVA dependência de sistema: binário `nft` (nftables) com fallback para `iptables` se `nft`
não estiver disponível, invocados via `std::process::Command` (mesmo padrão de `curl`/`tar` já usado
no projeto) — nenhuma crate Rust nova para a parte de rede.

**Storage**: nenhuma mudança.

**Testing**: estender `sandbox_unit_tests` (`crates/farol-core/src/sandbox.rs:252-509`) com asserções
sobre a presença do argumento `--seccomp <fd>` no `Vec<String>` retornado por `build_bwrap_args`
quando `allow_exec=false`, e sobre a lista de hosts/IPs que seriam liberados quando `network=true`
com hosts declarados. Estender `sandbox_integration_tests` (`sandbox.rs:510+`, exigem `bwrap`
instalado) com dois novos cenários reais: (1) plugin com `exec=false` que escreve e tenta executar
um binário em `/tmp` — deve falhar; (2) plugin com `network` declarando um host específico — conexão
ao host declarado funciona, conexão a outro host falha (usar servidores HTTP de fixture locais em
portas/loopback distintas para simular "host declarado" vs. "host não declarado", sem depender de
rede externa real).

**Target Platform**: Linux (Princípio I) — kernel com suporte a `seccomp-bpf` (praticamente universal
em kernels Linux modernos) e a namespaces de rede sem privilégio (`unshare(CLONE_NEWNET)` já usado
hoje por `--unshare-net`/`--share-net` do próprio `bwrap`).

**Performance Goals**: sem meta numérica dura — a geração do filtro BPF e a aplicação de regras
`nftables` ocorrem uma vez por spawn de plugin, não em caminho quente.

**Constraints**: FR-004/edge cases exigem fail-closed — se o mecanismo de `exec` ou de rede não puder
ser aplicado no sistema atual (kernel sem suporte, `nft`/`iptables` ausentes), o sandbox MUST recusar
montar o ambiente do plugin com mensagem de erro clara, nunca degradar silenciosamente para o
comportamento antigo (só filesystem / liga-desliga total).

**Scale/Scope**: User Story 1 (`exec`/seccomp) e User Story 2 (`network`/allowlist) são
independentes uma da outra (arquivos e mecanismos distintos) — podem ser implementadas em paralelo
por executores diferentes, desde que ambos editem `sandbox.rs` de forma coordenada (ver Complexity
Tracking).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- Nenhuma mudança de protocolo `farol-protocol` — `KnownCapability::Exec`/`Network` já existem e não
  mudam de forma; só a camada de enforcement em `farol-core::sandbox` muda. PASS.
- Reforça diretamente o princípio de sandboxing por capability já estabelecido na feature 006 —
  fecha débito técnico já formalmente rastreado (issues #12/#13), sem introduzir escopo novo além do
  que essas issues descrevem. PASS.

## Project Structure

### Documentação desta feature

```
specs/009-sandbox-hardening/
├── spec.md
├── plan.md              # este arquivo
├── checklists/requirements.md
└── tasks.md             # gerado a seguir
```

### Código afetado

```
Cargo.toml (raiz)         # ADICIONAR `crates/farol-seccomp-preload` aos `members` do workspace

crates/farol-seccomp-preload/   # NOVO crate `cdylib` (revisão de D1 — ver acima): gera e aplica,
├── Cargo.toml                  # via construtor ELF (`#[ctor]`) rodado no carregamento do `.so`
└── src/lib.rs                  # por `LD_PRELOAD`, o filtro `seccomp-bpf` que bloqueia
                                 # `execve`/`execveat` DENTRO do processo do plugin já `exec`ado
                                 # pelo `bwrap` (não mais no processo do `bwrap`/Farol)

crates/farol-core/
└── src/
    ├── sandbox_seccomp.rs   # (revisado) só localiza o `.so` já compilado de
    │                         # `farol-seccomp-preload` em `target/{debug,release}/` — não gera
    │                         # mais filtro seccomp neste processo (ver doc do módulo)
    └── sandbox.rs           # ESTENDER `build_bwrap_args`: quando `allow_exec=false`, localizar o
                              # `.so` via `sandbox_seccomp::locate_seccomp_preload_library`,
                              # bind-montá-lo read-only dentro do sandbox e anexar `--setenv
                              # LD_PRELOAD <caminho>` (posicionado DEPOIS do bind da raiz do
                              # repo/`code_root`, D5 — ordem de shadowing); gerar/aplicar regras
                              # `nftables`/`iptables` para hosts declarados quando
                              # `allow_network=true` com allowlist não vazia (via novo módulo
                              # auxiliar, ex. `sandbox_network.rs`, ou script wrapper invocado
                              # dentro do namespace de rede antes do `exec` do processo do plugin)
```

## Key Design Decisions (Phase 0+1 consolidado)

- **D1 (US1) — REVISADO (achado empírico durante a implementação de T004/T005)**: a versão original
  desta decisão (abaixo, riscada) gerava o filtro seccomp com `seccompiler`, serializava para um
  `memfd`, e passava o descritor de arquivo ao `bwrap` via `--seccomp FD`. Essa abordagem se mostrou
  **inviável**: `bwrap --seccomp FD` instala o filtro seccomp NO PRÓPRIO PROCESSO `bwrap`, antes do
  `execvp()` final que ele mesmo faz para lançar o comando do plugin — um filtro que bloqueia
  `execve` bloqueia justamente esse `execvp` interno, e o plugin nunca chega a iniciar (confirmado
  empiricamente: `bwrap: execvp /usr/bin/python3: Permission denied`).
  Correção adotada: o filtro passa a ser aplicado DENTRO do processo do plugin, depois que ele já foi
  `exec`ado com sucesso pelo `bwrap` — via `LD_PRELOAD` de uma biblioteca compartilhada
  (`crates/farol-seccomp-preload`, novo crate `cdylib` do workspace) com um construtor ELF (`#[ctor]`,
  crate `ctor`) que roda depois que o dynamic linker termina de carregar o `.so`, mas antes do
  `main()` do processo alvo — nesse ponto o processo já está de pé e não precisa de nenhum
  `execve`/`execveat` adicional para continuar, então o filtro só nega tentativas SUBSEQUENTES do
  próprio plugin. `sandbox::build_bwrap_args` (quando `allow_exec=false`) localiza o `.so` já
  compilado (`sandbox_seccomp::locate_seccomp_preload_library`), bind-monta-o read-only dentro do
  sandbox (posicionado depois do bind da raiz do repo/`code_root`, nunca antes — mesma classe de bug
  de sombreamento de ordem já documentada em D5 da feature 006), e passa `--setenv LD_PRELOAD
  <caminho>` ao `bwrap`. Não há mais `memfd`/FD nenhum repassado entre processos:
  `seccompiler::apply_filter` aplica o filtro diretamente no processo ATUAL (o do plugin, de dentro
  do próprio `.so`), sem precisar de nenhum FD vindo de fora. Fail-closed (FR-004): quando o `.so` não
  pode ser localizado, `build_bwrap_args` entra em pânico em vez de montar o sandbox sem a proteção —
  ver `sandbox_seccomp.rs` para o detalhe completo.
  ~~D1 original (obsoleto): o filtro seccomp é gerado com `seccompiler`, serializado para um arquivo
  temporário (ou `memfd`), e o descritor de arquivo correspondente é passado ao `bwrap` via
  `--seccomp FD` — exige que o processo Rust abra o FD antes de invocar `bwrap`
  (`std::process::Command` com `std::os::unix::io::AsRawFd`/`std::os::unix::process::CommandExt` para
  garantir que o FD sobrevive ao `exec` do `bwrap`, sem `O_CLOEXEC`).~~
- **D2 (US1)**: quando `allow_exec=true`, o filtro seccomp NÃO é aplicado (mantém o comportamento
  atual de bind read-only de `/usr/bin`/`/bin`/`/usr/local/bin`, FR-002) — o filtro só entra quando
  `allow_exec=false`, fechando exclusivamente o vetor residual descrito na issue #12.
  Fora de escopo desta feature: um filtro seccomp adicional para quando `exec=true` (ex. restringir
  a syscalls "seguras" mesmo com exec liberado) — não pedido pela issue #12.
- **D3 (US2)**: como `bwrap` não aplica `nftables`/`iptables` nativamente, a regra é aplicada por um
  processo auxiliar que roda DENTRO do namespace de rede recém-criado pelo `bwrap` (que já tem
  `CAP_NET_ADMIN` efetivo sobre seu próprio namespace, por ser dono dele via user namespace não
  privilegiado) — antes de `exec`ar o comando real do plugin. Padrão: `bwrap [...] -- <wrapper que
  aplica nft e depois faz exec do comando original>`.
- **D4 (US2)**: a resolução de host→IP ocorre uma vez, no processo pai (fora do sandbox), no momento
  de montar os argumentos do `bwrap` — os IPs resolvidos (não os hostnames) são passados ao wrapper
  de dentro do namespace, evitando exigir que o plugin sandboxed tenha acesso a DNS antes de as
  regras `nftables` estarem em vigor.
- **D5 (US2 — limitação descoberta na implementação)**: o mecanismo de allowlist por host exige que o
  `bwrap` monte um namespace de rede PRIVADO (sem `--share-net`) para que `CAP_NET_ADMIN` seja efetivo
  dentro do sandbox — confirmado empiricamente durante a implementação de T009-T014. Um namespace de
  rede privado, sem uma camada adicional de roteamento externo (ex.: `slirp4netns` ou veth+NAT —
  nova dependência de sistema, fora do escopo autorizado por este `plan.md`), só enxerga `lo`
  (loopback). A allowlist funciona corretamente para destinos de loopback (validada com testes reais),
  mas **hosts externos declarados na allowlist não são alcançáveis** no estado atual. Uma decisão de
  design futura será necessária para resolver o roteamento externo (introduzindo nova dependência de
  sistema ou alternativa arquitetural); este trabalho fecha a mediação de allowlist em si (fail-closed,
  sem regressão).

## Complexity Tracking

| Risco | Mitigação |
|---|---|
| ~~Passar um FD vivo para o `bwrap` via `std::process::Command` não é trivial na API estável do Rust~~ — **obsoleto**: a abordagem `--seccomp FD` foi abandonada (D1 revisado acima) por bloquear o próprio `execvp` interno do `bwrap`, não só o do plugin | Resolvido via `LD_PRELOAD` + construtor ELF (crate `farol-seccomp-preload`) — filtro aplicado dentro do processo do plugin, depois do `exec` do `bwrap`, sem nenhum FD repassado entre processos |
| `build_bwrap_args` (`sandbox.rs`) é uma função infalível (`Vec<String>`) chamada por `plugin_worker::worker()` sem tratamento de `Result` — não dá para propagar `SandboxMountError::SeccompUnavailable` como erro tipado até lá sem mudar essa assinatura (fora do escopo de US1) | Fail-closed via `panic!` no ponto exato em que o `.so` de `farol-seccomp-preload` não pode ser localizado — nunca monta o sandbox sem a proteção. Risco residual documentado: um `plugin_worker.rs` futuro poderia capturar esse caso de forma mais graciosa (`WorkerEvent::SpawnFailed` em vez de pânico), mas isso é uma mudança nesse arquivo, fora do escopo desta task |
| `nftables` pode não estar instalado no sistema do usuário | FR-004 (fail closed) cobre isso — falha explícita, nunca fallback silencioso; testar ambos os casos (`nft` presente e ausente) |
| Wrapper de rede dentro do namespace pode exigir um binário/script adicional embutido no Farol | Preferir um script inline gerado em runtime (heredoc/string Rust) sobre um binário separado, para não exigir passo de build extra no próprio Farol |
