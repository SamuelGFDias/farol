# AGENTS.md — contexto do projeto Farol

Referência para trabalho futuro nesta base — arquitetura, convenções e decisões que não são óbvias
só lendo o código. Mantido atualizado a cada mudança relevante (ver `CLAUDE.md` global do usuário).

## O que é

Farol é um "plano de controle pessoal" nativo Linux (GUI, sem navegador) que agrega **Ver / Agir /
Lembrar** sobre o estado técnico do dev (serviços monitorados, repos git, VPN, issues...) através de
plugins como processos separados. Visão completa em `README.md`; princípios formais e imutáveis em
`.specify/memory/constitution.md` (7 princípios: Nativo sem navegador, Plugins isolados via
JSON-RPC, Widgets declarativos, Permissões explícitas por manifesto, Espaços por contexto, Paleta
de comandos, Registry federado).

## Metodologia: spec-driven development (spec-kit)

O projeto usa o workflow **speckit** (skills `speckit-*`, diretório `.specify/`). Cada feature vive
em `specs/NNN-nome-da-feature/` com `spec.md` (requisitos), `plan.md` (design), `research.md`
(decisões `D1`, `D2`...), `data-model.md`, `contracts/`, `quickstart.md` (cenários de validação
manual) e `tasks.md` (lista de tasks `TNNN` com checkbox `[ ]`/`[X]`, dependências explícitas e
notas de execução). **Antes de implementar qualquer coisa numa feature, ler o `tasks.md`
correspondente** — ele é a fonte de verdade de "o que falta" e de por que cada task existe.

Features existentes:
- `specs/001-walking-skeleton-git-plugin/` — protocolo v0.1, plugin `git-local` de referência.
- `specs/002-uptime-kuma-plugin/` — protocolo v0.2 (bump deliberado, quebra `git-local` de
  propósito — débito técnico #4 rastreado como issue, migração fora de escopo desta feature),
  arquitetura multi-plugin, config/secrets geridos pelo core, plugin `uptime-kuma`.
- `specs/003-automated-testing-infrastructure/` — harness de testes em duas camadas (ver § Testes).
- `specs/004-vpn-status-plugin/` — **completa**. Plugin `openfortivpn-vpn` com três User Stories
  implementadas: (US1) Ver o estado da VPN em tempo real via `openfortivpn-gui status --json`;
  (US2) Conectar/desconectar pelo widget com tradução de erros; (US3) Duração de sessão visível.
  Protocolo `0.2` → `0.3` (aditivo: novo `kind` de widget `"vpn-status"`, `ActionInvokeResult`
  generalizado para `oneOf`). `git-local`/`uptime-kuma` migrados para `"0.3"` na mesma feature
  (decisão D2 de `research.md`, evitando repetir dívida técnica da migração `0.1→0.2`, débito #4).
  Achado de generalização durante a feature: `Message::FetchRequested` renomeado para
  `Message::ActionInvokeRequested` (o mecanismo de `action/invoke` passava a ser reutilizado também
  por `vpn.connect`/`vpn.disconnect`, não era mais específico de `git.fetch`). Nova seção de
  teste: `tests/fixtures/fake-openfortivpn-gui/` — CLI determinística para e2e sem depender de
  instalação real de `openfortivpn-gui`.
- `specs/005-docker-containers-plugin/` — **completa**. Plugin `docker-containers` com duas User
  Stories implementadas: (US1) Ver o estado de todos os containers locais em tempo real (nome,
  imagem, estado, incluindo parados); (US2) Iniciar, parar e reiniciar um container pelo widget
  com feedback de operação em curso e tradução de erros por container. Protocolo `0.3` → `0.4`
  (aditivo: novo `kind` de widget `"container-status-grid"`, terceira variante de `ActionInvokeResult`
  denominada `Container`, dois novos códigos de erro `-32010`/`docker_unavailable` e `-32011`/
  `container_action_failed`). `git-local`/`uptime-kuma`/`openfortivpn-vpn` migrados para `"0.4"`
  na mesma feature (reutilizando padrão decisão D2 da feature 004, evitando reacumular débito de
  migração). Generalização real executada nesta feature: `merge_widget_items`/`normalize_widget_items`
  (`update.rs`) ampliadas para enxergar `&PluginConnection` completo em vez de só `&[RepositoryViewModel]`,
  permitindo o merge por `id` de `ContainerViewModel` preservar `action_in_flight`/`last_action_error`
  de containers individuais durante um refresh concorrente (FR-017). `ContainerInvokeResult::Container`
  em `ActionInvokeResult` utiliza `Box<ContainerStatusItem>` para manter o tamanho do enum sob
  limite do `clippy::large_enum_variant` sem necessidade de `#[allow]`. Fixture de teste determinística:
  `tests/fixtures/fake-docker/` — binário `docker` CLI sintético, cobrindo todos os cenários de
  sucesso/falha sem depender de instalação ou daemon Docker na máquina de CI. Bug descoberto e
  registrado durante a implementação (fora de escopo): issue #11 (erro de `widget/get` do widget
  VPN nunca alcança a UI, rastreado como débito de feature 004).

- `specs/006-sandbox-permissoes-bubblewrap/` — **completa**. Isolamento real de processo via
  `bwrap` (bubblewrap) para os 4 plugins de referência, com três User Stories implementadas: (US1)
  rede negada por padrão a todo plugin (`--unshare-all` sem `--share-net`) — só liberada para quem
  declara a capability `network`; (US2) capability `exec` mediada por visibilidade seletiva de
  filesystem (sem `exec`, `/usr/bin`/`/bin`/`/usr/local/bin` não são bindados, só o interpretador e
  suas libs) mais dois casos especiais de acesso a filesystem além do próprio código do plugin,
  nomeados em vez de virarem capability genérica nova: `git-local`/`scan_root` (bind read-write do
  diretório configurado, mesmo default/caminho de config que o plugin já lê) e
  `docker-containers`/socket Docker (bind do socket Unix `/var/run/docker.sock` ou equivalente
  rootless); (US3) segredos nunca vazam por bind — `~/.config/farol` nunca entra na lista de binds
  de nenhum plugin, então `secrets.toml`/`config.toml` de qualquer plugin (inclusive o próprio) são
  inexistentes do ponto de vista do processo sandboxed, confirmado tentando localizá-los de dentro do
  sandbox. Decisão central, D1 de `research.md`: a fonte de verdade do perfil de sandbox de cada
  plugin é o registro estático `known_plugins()` (novo campo de `PluginSpawnConfig`), decidido **antes**
  do spawn — não o `CapabilityManifest` que o próprio plugin declara no `handshake/hello`, por dois
  motivos: (a) ordem lógica — o manifesto só chega depois que o processo já foi spawnado e o sandbox
  já precisa estar em vigor (chicken-and-egg); (b) autorrelato do próprio processo sobre seu isolamento
  não é fronteira de segurança real (um plugin malicioso declararia o que quisesse). O
  `CapabilityManifest` do handshake continua existindo/exibido na UI como antes, só deixa de ser,
  sozinho, quem decide o sandbox. Novo módulo `crates/farol-core/src/sandbox.rs`: constrói o
  `Vec<String>` de argumentos do `bwrap` a partir de um `SandboxProfile` (rede on/off, exec on/off,
  binds extras). Correção de manifesto nesta feature (D7): `git-local` e `openfortivpn-vpn` passam a
  declarar `network` também — achado do planejamento, não scope creep: `git.fetch` contra um remote
  real e `vpn.connect` contra um servidor real sempre dependeram de rede, o manifesto nunca refletiu
  isso porque nunca tinha sido de fato aplicado antes desta feature. Três bugs reais encontrados e
  corrigidos durante a implementação: (1) o `repo_root` bindado read-only pelo sandbox precisava vir
  de `env!("CARGO_MANIFEST_DIR")` em tempo de compilação, não de `std::env::current_dir()` em
  runtime — o `cwd` do processo do Farol já muda depois que o `--chdir` do próprio `bwrap` entra em
  jogo; (2) `find_uptime_kuma_pid` (usado pelos testes de `Crashed`/`Unresponsive`) precisou passar a
  caminhar descendentes **transitivos**, não só filhos diretos, porque a árvore de processos sob
  `bwrap` ganhou uma camada a mais (`bwrap` → processo intermediário → `python3` real); (3) ordem dos
  argumentos do `bwrap` — os mounts sintéticos genéricos (`--proc`/`--dev`/`--tmpfs /tmp`) MUST vir
  logo depois das flags de namespace e **antes de qualquer bind de caminho real** (interpretador,
  DNS/TLS, `/usr/bin`, raiz do repo, `extra_binds`), não só antes de `extra_binds` como a primeira
  versão do contrato prescrevia — um bind posicionado antes desses mounts genéricos fica invisível se
  o caminho bindado estiver aninhado sob um deles (ex.: um `scan_root` de teste ou o shim de
  interpretador do `harness.sh`, ambos sob `/tmp`, somem depois que `--tmpfs /tmp` monta por cima).
  Escape-hatch só de teste (D14): variável de ambiente `FAROL_SANDBOX_TEST_EXTRA_BIND`, lida por
  `worker()`, adiciona um bind extra gravável só quando setada — usada exclusivamente por
  `tests/integration/harness.sh` para o shim que grava a transcrição JSON-RPC sob `/tmp` (a `/tmp`
  privada por sandbox é uma propriedade de segurança desejada, não algo a generalizar); nenhum código
  de produção (nenhum dos 4 plugins, nenhuma entrada de `known_plugins()`) depende dela. Quatro
  issues de débito técnico abertas nesta feature: #12 (mediação de `exec` é só por visibilidade de
  filesystem, não por um filtro `seccomp` real contra a syscall `execve` — um plugin hostil poderia em
  teoria escrever um payload num `tmpfs` gravável e executá-lo por caminho absoluto sem depender de
  `$PATH`); #13 (capability `network` tratada como liga/desliga pelo sandbox, sem allowlist real por
  host apesar de `allowed_hosts` já existir no tipo Rust e ser exibido na UI); #14 (duas linhas E501
  do `ruff` pré-existentes em `openfortivpn-vpn`/`uptime-kuma` — débito **da feature 005**, achado só
  durante a verificação final desta feature, não introduzido por ela).

- `specs/007-registry-instalacao-plugins-github/` — **completa**. Registry de instalação de plugins
  de terceiros via GitHub (Princípio VII, Registry Federado sem Infra Própria), com três User Stories
  implementadas: (US1) descoberta dinâmica — o core escaneia `$XDG_DATA_HOME/farol/plugins/` a cada
  início (`plugin_worker::discover_installed_plugins()`), lê o manifesto de cada plugin instalado e o
  spawna pela mesma máquina de estados dos 4 plugins de referência, sem recompilar o core; (US2)
  `farol install <owner>/<repo>` (`install::run`), subcommand de CLI tratada em `main()` antes de
  montar a `iced::application` — baixa a release/tag mais recente via `curl` contra a API do GitHub,
  extrai o tarball de código-fonte com `tar --strip-components=1`, valida o manifesto e publica com
  rename atômico em `installed_plugin_dir(nome)` (sem cliente HTTP Rust novo, disciplina de
  minimalismo de dependência das features 001-006); (US3) `templates/plugin-template/` — esqueleto
  mínimo (`farol-plugin.toml` de exemplo + `main.py` respondendo só `handshake/hello`, zero widgets)
  para quem for escrever um plugin novo sem precisar ler os 4 de referência inteiros. Novo manifesto
  `farol-plugin.toml` (módulo `plugin_manifest.rs`, `parse_manifest`): TOML plano espelhando
  `sandbox::SandboxProfile` diretamente (`plugin_name`/`command`/`args` + `[capabilities]` com
  `network`/`exec` booleanos, em vez da forma aninhada lista-de-enum do `CapabilityManifest` do
  protocolo) — campo desconhecido é ignorado na desserialização (mesma tolerância já praticada pelo
  protocolo JSON-RPC). Novo diretório de dados `$XDG_DATA_HOME/farol/plugins/<nome>/`
  (`plugin_manifest::farol_data_base_dir()`/`installed_plugin_dir()`), distinto do
  `$XDG_CONFIG_HOME/farol` já usado por `config_store`/`secrets_store` desde a feature 002 —
  separação deliberada entre configuração do usuário (pequena, editável à mão) e código-fonte
  instalado de terceiro (maior, gerado por download). `PluginSpawnConfig` ganha `code_root: PathBuf`:
  generaliza o bind de filesystem do sandbox (feature 006) de "sempre a raiz do repo Farol" para
  "por-plugin" — os 4 de referência continuam apontando pra raiz do repo (mesmo comportamento de
  antes), um plugin descoberto aponta para o próprio `installed_plugin_dir`, então nunca enxerga,
  dentro do sandbox, o código de outro plugin nem do core. Filtragem de colisão de nome
  (`plugin_worker::all_plugins()`): um `plugin_name` descoberto que colida com um dos 4 de referência
  é descartado com aviso (`eprintln!`) — o de referência sempre vence; entre dois descobertos
  colidindo entre si, mantém o primeiro pela ordem de `std::fs::read_dir` (caso patológico, não
  otimizado). A fonte de verdade do `SandboxProfile` de um plugin instalado é sempre o manifesto lido
  do disco na descoberta — nunca o `CapabilityManifest` que o processo declara no handshake em
  runtime (mesma disciplina de D1 da feature 006, generalizada). Testes automatizados herméticos
  (sem rede externa): fluxo de instalação validado contra um servidor HTTP local sintético
  (`install.rs`, mesmo padrão de `MetricsFixtureServer` da feature 002), com `base_url` da API do
  GitHub configurável via `FAROL_GITHUB_API_BASE` só para teste. Quatro issues de débito técnico
  abertas nesta feature: #15 (instalação inteiramente dentro da UI gráfica do iced — MVP é só a
  subcommand de CLI); #16 (plugin que exija passo de build ou asset binário próprio — MVP cobre só
  extração direta de código-fonte interpretado); #17 (capability de filesystem genérica declarável
  por um plugin de terceiro, equivalente aos casos especiais nomeados `scan_root`/socket Docker da
  feature 006 — plugin instalado via registry só recebe `network`/`exec` nesta fase); #18 (criar e
  publicar de verdade um repositório-índice GitHub central com CI de validação de PRs — decisão de
  infraestrutura externa fora de modo silencioso, ver `spec.md`/Clarifications).

- `specs/008-registry-hardening/` — **quase completa (16/17 tasks; T017 pendente, ação de
  infraestrutura externa)**. Fecha as quatro issues de débito abertas na feature 007 (#15, #16,
  #17, #18), com quatro User Stories: (US1, issue #18) repositório-índice GitHub central — novo
  módulo `crates/farol-core/src/registry_index.rs`, `RegistryIndexEntry{name, owner, repo}` e
  `resolve_name(name, index_url_base) -> Result<RegistryIndexEntry, RegistryIndexError>`
  (`NotFound`/`FetchFailed`/`MalformedIndex`, distinção deliberada para que "índice inacessível"
  nunca seja interpretado como "plugin não existe") busca `index.toml` via `curl` (mesmo padrão
  `install.rs`), com override de teste `FAROL_REGISTRY_INDEX_URL` (mesmo espírito de
  `FAROL_GITHUB_API_BASE` da feature 007); `install::run_by_name(name) -> InstallByNameOutcome`
  (tipo **novo**, não uma variante a mais em `InstallOutcome` — decisão de escopo documentada na
  nota de T006: evitar tocar o `match` exaustivo de `main.rs::handle_install_subcommand` fora do
  Foundational) repassa `owner/repo` resolvido para o `install::run` já existente da feature 007,
  sem duplicar lógica de instalação (FR-003); `default_index_url_base()` aponta hoje para um
  placeholder (`https://raw.githubusercontent.com/farol-registry/farol-plugin-index/main`) que
  falha com `FetchFailed` visível até T017 publicar o repositório de verdade. (US2, issue #16)
  comando de `build` opcional no manifesto (`PluginManifest::build: Option<String>`) — quando
  presente, `install::run` o executa via `sh -c` no `staging_dir`, depois de `parse_manifest` e
  antes do rename atômico; `exit code != 0` aborta a instalação sem publicar nada (reaproveita
  `InstallOutcome::DownloadFailed`, mensagem prefixada "comando de build falhou: ..." — outra
  variante nova de `InstallOutcome` exigiria o mesmo `match` exaustivo de `main.rs` fora de escopo,
  mesmo raciocínio de US1). (US3, issue #17) capability de filesystem genérica — manifesto ganha
  `filesystem_read`/`filesystem_read_write: Vec<String>` (caminhos absolutos), validados por
  `validate_filesystem_paths` contra `filesystem_capability_denylist()` (`~/.ssh`, `/etc`,
  `$XDG_CONFIG_HOME/farol` — resolvendo env vars, nunca caminho literal hardcoded) via
  `Path::starts_with` (rejeita tanto o caminho denylistado exato quanto qualquer descendente, ex.
  `~/.ssh/id_rsa`); caminho relativo vira `ManifestError::RelativeFilesystemPath`, caminho na
  denylist vira `ManifestError::DenylistedFilesystemPath` — ambos rejeitados já no `parse_manifest`
  (nunca chegam a `plugin_worker::discover_installed_plugins`). `discover_installed_plugins`
  converte cada caminho já validado em um `sandbox::BindMount` (`extra_binds`), somente-leitura
  para `filesystem_read` e leitura/escrita para `filesystem_read_write` — mesma disciplina
  "negado por padrão, concedido explicitamente" de `network`/`exec` (feature 006) e mesma fonte de
  verdade "manifesto lido do disco na descoberta, nunca o `CapabilityManifest` do handshake"
  (D1 da feature 006, generalizada de novo na feature 007). (US4, issue #15) tela de instalação
  in-app: `Farol::install_form: model::InstallForm` (`model.rs`, mesmo padrão de desvio de local
  já documentado para `SetupForm` — vive direto em `Farol`, não numa `PluginConnection`, porque a
  instalação não pertence a nenhuma conexão de plugin específica), `InstallFormStatus`
  (`Idle`/`InProgress`/`Success(String)`/`Error(String)`), `Message::InstallFormNameChanged`/
  `InstallByNameSubmitted`/`InstallOutcomeReceived{success, message}`, `view_install_form`
  (`view.rs`, reaproveita o padrão visual de `view_setup_form`) e
  `Farol::handle_install_by_name_submitted` (`update.rs`) que dispara `install::run_by_name` dentro
  de `tokio::task::spawn_blocking` via `iced::Task::perform`, mantendo a UI responsiva durante
  download/build (FR-012) — resultado chega como `Message::InstallOutcomeReceived`, já traduzido
  para `(bool, String)` por `describe_install_by_name_outcome` porque `InstallByNameOutcome`/
  `InstallOutcome` não são `Clone` e `Message` precisa ser. **Único item pendente: T017** —
  publicar de verdade o repositório-índice (`index.toml` + `.github/workflows/validate.yml` de CI)
  no GitHub; marcado em `tasks.md` como checkpoint sensível que exige confirmação explícita do
  usuário sobre nome/owner antes de disparar, não presumível a partir de T001-T016 completas.

- `specs/009-sandbox-hardening/` — **parcial (8/14 tasks, só User Story 1 completa)**. Fecha as
  duas issues de débito abertas na feature 006 (#12, #13). **US1 (issue #12, mediação real de
  `execve`/`execveat` via seccomp) — completa**: antes desta feature, `allow_exec=false` só
  escondia binários por visibilidade seletiva de filesystem (bind: sem `exec`, `/usr/bin`/`/bin`/
  `/usr/local/bin` não são bindados), deixando um vetor residual — um plugin hostil podia escrever
  um payload executável num `tmpfs` gravável (`/tmp`) e executá-lo por caminho absoluto, sem
  depender de `$PATH`. **D1, revisado por achado empírico durante a implementação**: a abordagem
  original de `plan.md`/`tasks.md` (gerar o filtro com `seccompiler` no processo do Farol,
  serializar num `memfd`, passar ao `bwrap` via `--seccomp FD`) se mostrou **inviável** —
  `bwrap --seccomp FD` instala o filtro NO PRÓPRIO PROCESSO `bwrap`, antes do `execvp()` final que
  ele mesmo faz para lançar o comando do plugin, bloqueando esse exec interno (confirmado
  empiricamente: `bwrap: execvp /usr/bin/python3: Permission denied`). Correção adotada: o filtro
  passa a ser aplicado DENTRO do processo do plugin, depois que ele já foi `exec`ado com sucesso
  pelo `bwrap`, via `LD_PRELOAD` de uma biblioteca compartilhada com um construtor ELF. Novo membro
  do workspace, crate `cdylib` `crates/farol-seccomp-preload/`: gera o filtro BPF
  (`build_exec_deny_filter`, via `seccompiler`, bloqueando `execve`/`execveat`, cobrindo `fexecve`
  por consequência) e o aplica ao próprio processo a partir de um `#[ctor::ctor]` que roda quando o
  `.so` é carregado via `LD_PRELOAD`, depois que o dynamic linker termina de carregar mas antes do
  `main()` do processo alvo — nesse ponto o processo já está de pé e não precisa de nenhum exec
  adicional para continuar, então o filtro só nega tentativas SUBSEQUENTES do próprio plugin.
  `crates/farol-core/src/sandbox_seccomp.rs`, depois da revisão, só localiza o `.so` já compilado
  (`SECCOMP_PRELOAD_LIB_FILENAME = "libfarol_seccomp_preload.so"` em
  `target/{debug,release}/`) via `locate_seccomp_preload_library()` — não gera mais nenhum filtro
  do lado do Farol; escape-hatch só de teste `FAROL_SANDBOX_TEST_FORCE_SECCOMP_PRELOAD_MISSING`
  (mesmo padrão de `FAROL_SANDBOX_TEST_EXTRA_BIND` da feature 006), usado só por
  `sandbox_unit_tests`/`sandbox_integration_tests` para exercitar o caminho fail-closed. Novo
  `pub enum SandboxMountError` em `sandbox.rs` (`SeccompUnavailable`/`NetworkFirewallUnavailable`,
  ambos com `Display`/`Error`); `build_bwrap_args`, quando `allow_exec=false`, bind-monta o `.so`
  read-only (posicionado DEPOIS do bind da raiz do repo/`code_root`, nunca antes — mesma classe de
  bug de sombreamento de ordem já documentada para `--tmpfs /tmp` na feature 006) e anexa
  `--setenv LD_PRELOAD <caminho>`; quando `locate_seccomp_preload_library()` devolve
  `Err(SandboxMountError::SeccompUnavailable)`, `build_bwrap_args` entra em `panic!` com a mensagem
  do erro — fail-closed real, nunca monta o sandbox sem a proteção (risco residual documentado em
  `plan.md`: `build_bwrap_args` é infalível/`Vec<String>`, chamada por `plugin_worker::worker()`
  sem `Result`, então propagar o erro tipado até lá exigiria mudar essa assinatura, fora do escopo
  desta US). **Nota de processo, vale como lição**: a integração em `sandbox.rs` desta feature foi
  implementada, depois perdida por um `git checkout` acidental que descartou trabalho não
  commitado numa sessão anterior (sem rastro recuperável em `git reflog` nem em objetos órfãos), e
  reconstruída do zero nesta sessão a partir da especificação em `tasks.md`/`plan.md` — o crate
  `farol-seccomp-preload` e `sandbox_seccomp.rs` sobreviveram intactos porque já estavam
  commitados. **US2 (issue #13, allowlist de rede por host via `nftables`) — não iniciada**:
  `crates/farol-core/src/sandbox_network.rs` existe como esqueleto vazio (T009-T014 pendentes).
  Design já decidido em `plan.md`: D3, como `bwrap` não aplica `nftables`/`iptables` nativamente, a
  regra é aplicada por um processo auxiliar rodando DENTRO do namespace de rede recém-criado pelo
  `bwrap` (que já tem `CAP_NET_ADMIN` efetivo sobre seu próprio namespace, por ser dono dele via
  user namespace não privilegiado), antes de `exec`ar o comando real do plugin — padrão
  `bwrap [...] -- <wrapper que aplica nft e faz exec do comando original>`, gerado como script
  inline (heredoc/string Rust) para não exigir binário/passo de build extra no Farol; D4, a
  resolução host→IP acontece uma vez no processo pai, fora do sandbox, antes de montar os
  argumentos do `bwrap` — só os IPs resolvidos (não os hostnames) são passados ao wrapper de
  dentro do namespace, evitando exigir DNS disponível ao plugin sandboxed antes de as regras
  `nftables` estarem em vigor.

- `specs/010-widget-detail-surface/` — **parcial (14/20 tasks, só User Story 1 completa)**. Fecha
  as duas issues de débito abertas nas features 002/005 (#9, #10). **US1 (issue #9, discriminador
  explícito de `WidgetItems`) — completa, protocolo `"0.4"` → `"0.5"`**: antes desta feature,
  `WidgetItems` (`#[serde(untagged)]`, `farol-protocol/src/messages.rs`) desambiguava a variante
  (`Git`/`Monitor`/`Vpn`/`Container`) por uma disjunção incidental de campos obrigatórios entre os
  quatro tipos de item — invariante informal, nunca garantida pelo compilador nem pelo schema
  (débito documentado em `research.md` D12 da feature 005/002; um `items: []` em particular sempre
  desserializava como a primeira variante declarada, `Git`, corrigido só do lado consumidor por
  `update::normalize_widget_items`, débito #5/issue #7). Resolvido com novo `enum WidgetItemKind`
  (`Git`/`Monitor`/`Vpn`/`Container`, `Copy`/`Eq`, serializa como o nome de variante puro sem
  `rename_all`) e um campo `kind: WidgetItemKind` obrigatório em `WidgetGetResult`, com
  `Deserialize` **implementado manualmente** (não `derive`): lê `kind` primeiro via uma struct
  `WidgetGetResultWire` intermediária (`items` como `serde_json::Value`), e só então desserializa
  `items` diretamente para a variante de `WidgetItems` correspondente — a consistência entre `kind`
  e o formato real de `items` é garantida pela própria construção do parsing, não por checagem
  posterior; não existe caminho para produzir um `WidgetGetResult` cujo `kind` minta sobre a
  variante de `items`. `WidgetItems` em si continua `#[serde(untagged)]` (usado "solto" fora deste
  envelope). Protocolo bump `"0.4"` → `"0.5"` (`crates/farol-protocol/src/version.rs`); os 4
  plugins de referência (`git-local`, `uptime-kuma`, `openfortivpn-vpn`, `docker-containers`)
  migrados para `"0.5"` na mesma feature, cada um emitindo o `kind` correspondente
  (`"Git"`/`"Monitor"`/`"Vpn"`/`"Container"`) — mesma disciplina de migração conjunta já praticada
  nas features 004/005 (D2, evitando reacumular débito #4). Novo diretório de schema
  `protocol/schema/v0.5/` (cópia de `v0.4` com `widget.schema.json` trocando o `anyOf` puro por
  `if`/`then` sobre `kind`, tornando o campo obrigatório no schema também). **US2 (issue #10,
  painel de detalhe/drill-down genérico) — parcial, implementação adiantada em relação a
  `tasks.md`**: `tasks.md` marca T015 (estado em `model.rs`) e T016 (variantes de `Message` +
  roteamento em `update.rs`) como `[ ]` não feitas, mas o código já as tem — `model.rs` ganhou
  `DetailPanelState{plugin_name: String, items: farol_protocol::messages::WidgetItems}` (por
  convenção, `items` sempre carrega exatamente um elemento, o item clicado; reaproveita o
  `WidgetItems` do protocolo em vez de um enum paralelo), `Farol::detail_panel:
  Option<model::DetailPanelState>` vive direto em `Farol` (`main.rs`, mesmo desvio de local já
  documentado para `SetupForm`/`InstallForm` — não pertence a nenhuma `PluginConnection`
  específica), `Message::ItemDetailRequested{plugin_name, items}`/`ItemDetailClosed`, e
  `Farol::handle_item_detail_requested`/`handle_item_detail_closed` (`update.rs`) que
  substituem/limpam `detail_panel` incondicionalmente. O que falta de fato (T017-T020, confirmado
  pelo warning do compilador "variants `ItemDetailRequested` and `ItemDetailClosed` are never
  constructed" — nenhum código em `view.rs` produz essas mensagens ainda): `format_item_detail(&
  WidgetItems) -> Vec<(String, String)>` (par label/valor, genérico por tipo de item),
  `view_detail_panel` consumindo esse formato e composto sobre a view principal via
  `iced::widget::stack!` (API a confirmar na versão `iced 0.14` em uso antes de implementar), o
  gatilho de abertura em `view_container_row` (reaproveitando o padrão visual de
  `view_container_action_control` mas disparando `Message::ItemDetailRequested` em vez de
  `ActionInvokeRequested`), e o teste de integração via `iced_test::Emulator` confirmando
  clique-abre/fecha-restaura sem perder o estado da lista.

`.specify/memory/constitution.md` é normativo e versionado (SemVer próprio, atualmente v1.0.0).
Mudança de princípio exige emenda formal (skill `speckit-constitution`) — não editar a constitution
diretamente fora desse processo.

## Arquitetura do core (`crates/farol-core`, binário `farol`)

Padrão Elm/Model-Update-View via `iced 0.14`. Um `cargo run --bin farol` (ou o binário
`target/debug/farol`) **precisa rodar com cwd = raiz do repo** — `plugin_worker::known_plugins()`
usa caminhos relativos (`plugins/git-local/main.py`, `plugins/uptime-kuma/main.py`) para o
`command`/`args` de cada plugin spawnado.

- `main.rs` — entrypoint `iced::application`; `Farol { plugins: Vec<PluginSlot> }`, uma entrada por
  plugin conhecido (registro fixo em `plugin_worker::known_plugins()`), cada uma com seu próprio
  `PluginConnection` e canal de worker.
- `model.rs` — tipos de estado puro: `PluginState` (`Starting → Handshaking → Ready`, ou
  `Unavailable{reason}` — `FailedToStart`/`VersionIncompatible`/`Crashed`/`Unresponsive` são
  terminais; `NotConfigured` é a **única exceção não-terminal**, com caminho de volta via
  `SetupForm` + `Message::SetupSubmitted`, que incrementa `PluginConnection::setup_attempt` e força
  a reconexão do worker — feature 002, T029-T035, completo).
- `plugin_worker.rs` — spawn do processo filho, I/O assíncrona via `iced::stream::channel` +
  `tokio::process`, handshake, ciclo `widget/get`/`action/invoke`. Uma `Subscription` por plugin via
  `Subscription::run_with_id("{plugin_name}-{setup_attempt}", stream)` — o `setup_attempt` no `id`
  é o que faz o `iced` encerrar o processo filho antigo (`kill_on_drop`) e iniciar um novo quando a
  tela de setup é submetida (`Subscription::run` simples não aceita capturar `config` nem esse
  contador).
- `update.rs` — transições `Message → Farol`. `subscription()` monta uma `Subscription` por plugin
  (worker) + uma `Subscription` de timer de refresh por conexão `Ready` (`refresh_tick_stream`, ver
  armadilha abaixo — **não** usa `iced::time::every` diretamente).
- `view.rs` — renderização condicionada a `PluginState`, incluindo `view_setup_form` (tela de setup,
  `TextInput`/botão ligados a `Message::SetupFieldChanged`/`SetupSubmitted`) quando
  `Unavailable{NotConfigured}`.
- `config_store.rs`/`secrets_store.rs` — leitura/escrita de
  `~/.config/farol/plugins/<nome>/config.toml` e `~/.config/farol/secrets.toml`, injetados no spawn
  do plugin como variável de ambiente `FAROL_PLUGIN_<NOME>_<CAMPO>` (maiúsculo, `-`→`_`) — **não**
  system keyring (ver nota de decisão abaixo). Escrita (`save_plugin_config`/`save_plugin_secrets`)
  chamada por `Farol::handle_setup_submitted` (update.rs) ao confirmar a tela de setup.

### Armadilha real já corrigida: `iced::Subscription::map` exige closure não-capturante

Em `iced 0.13`, `Subscription::map` fazia `debug_assert!(size_of::<F>() == 0, ...)` — um closure
`move |x| ...` que captura qualquer variável do ambiente **panicava em runtime** assim que a
`Subscription` era montada (mensagem: `"the closure ... is capturing"`), e isso não aparecia em
nenhum teste unitário (eles testavam `update.rs` chamando `handle_worker_event` diretamente, nunca
`Farol::subscription()` de verdade rodando sob o runtime `iced`). Só um `cargo run` real revelava o
panic — motivo pelo qual `tasks.md` da feature 002 tem uma task dedicada (T023) só para rodar o
binário de verdade antes de seguir para as próximas fases.

**Sob `iced 0.14` (feature 003) essa classe de bug virou erro de compilação**: o mesmo check hoje é
`const { check_zero_sized::<F>() }` dentro de `Subscription::map`, avaliado em tempo de compilação
— não existe mais binário compilável com um closure capturante nesse ponto, então não há mais nada
a "revelar em runtime" para esse caso específico. O relato histórico abaixo (dois pontos distintos
de `Farol::subscription()`, cada um só alcançável sob uma condição de runtime diferente) continua
valendo como lição sobre cobertura de teste — só o mecanismo de detecção mudou de `debug_assert!`
de runtime para erro `E0080` de compilação. Achado completo (N1/N2/N3) e a regressão deliberada que
comprova o `E0080` atual: `specs/003-automated-testing-infrastructure/research.md` D1 (entrada de
2026-09-01) e `specs/003-automated-testing-infrastructure/quickstart.md` § Cenário 1.

Padrão correto quando um valor por-plugin (como `plugin_name`) precisa ir dentro da `Message`
produzida por uma `Subscription`: embuti-lo no **stream** via `futures::StreamExt::map`/dentro de um
`iced::stream::channel` (sem essa restrição, por não passar pelo `Subscription::map` do `iced`)
antes de envolver em `Subscription`, e só então usar `Subscription::map` com um closure que só usa
seu próprio parâmetro (zero-sized), ou nem usar `.map()` nenhum se o stream já produz o tipo final
diretamente. Ver `plugin_worker::subscription` (devolve `Subscription<(String, WorkerEvent)>`) e
`update.rs::subscription` (`.map(|(plugin_name, event)| Message::Worker { plugin_name, event })`)
para o padrão de referência.

**Esse bug apareceu duas vezes na mesma feature (002), em dois pontos diferentes de
`Farol::subscription()`** — cada um só alcançável sob uma condição de runtime distinta que nenhum
teste unitário nem execução manual anterior tinha exercitado:
1. A `Subscription` do worker de cada plugin (`.map(move |event| Message::Worker { plugin_name:
   worker_plugin_name.clone(), event })`) — pega em **qualquer** execução real, corrigido durante a
   task T023.
2. O timer de refresh periódico (`iced::time::every(interval).map(move |_instant| Message::
   RefreshTick { plugin_name: tick_plugin_name.clone() })`), dentro de `if slot.connection.state ==
   PluginState::Ready { ... }` — só é alcançado quando **algum plugin de fato chega a `Ready`**, o
   que só passou a acontecer depois que a feature 002 implementou o handshake/widget real do
   `uptime-kuma` (T024-T035); corrigido substituindo `iced::time::every(...).map(...)` por um stream
   próprio (`refresh_tick_stream`, via `iced::stream::channel` + `tokio::time::interval`, primeiro
   tick descartado para não duplicar o "fetch imediato ao ficar Ready" já existente).

**Lição para revisão de código nesta base**: `grep -n "Subscription::map\|\.map(move \|"` em
`update.rs`/`plugin_worker.rs` não basta como checklist estático — qualquer `Subscription` nova
precisa ser exercitada de verdade (`cargo run`, não só `cargo test`) sob a condição de runtime que a
constrói, não só revisada por leitura. Um teste unitário que chama `Farol::subscription()`
diretamente só prova algo se o `Farol` de teste estiver no estado (`PluginState`, contadores, etc.)
que ativa o branch em questão — `Farol::default()` não ativa nenhum dos dois casos acima.

## Protocolo (`crates/farol-protocol`, `protocol/`)

JSON-RPC 2.0 sobre NDJSON em stdin/stdout (mesmo modelo do LSP/MCP) — core sempre inicia a
requisição, plugin nunca escreve nada antes de responder `handshake/hello`. `protocol/SPEC.md` é a
especificação normativa; `protocol/schema/v0.{1,2}/*.schema.json` são os JSON Schemas
correspondentes, versionados lado a lado (v0.1 histórico, mantido para o cenário deliberado de
incompatibilidade). Versão atual: `"0.2"`. Comparação de versão é `MAJOR.MINOR` (D7 de
`research.md` da feature 001) — `crates/farol-protocol/src/version.rs`.

Plugins de referência (`plugins/`) são implementações Python **independentes**, que não importam
`farol-protocol` — leem só a spec/schema/contracts, para provar que o protocolo é genuinamente
agnóstico de linguagem (`git-local`: stdlib só, `uptime-kuma`: idem, ver docstring de
`plugins/uptime-kuma/main.py`).

## Decisão de arquitetura: segredos geridos pelo core, não system keyring

A constitution original (v0.3.0, Princípio IV) exigia keyring do sistema para segredos de plugin.
Emendada para v1.0.0 (2026-09-01) após auditoria da feature 002: segredos agora são geridos pelo
**core** (`secrets_store.rs`, arquivo `~/.config/farol/secrets.toml` com permissão `0600`) e
injetados como variável de ambiente no spawn — não há dependência de keyring do sistema (D8 de
`specs/002-uptime-kuma-plugin/research.md`). Ver `git log` da emenda para o raciocínio completo.

## Testes

- `cargo test --workspace` — 208 testes Rust passando (0 `#[ignore]`d, contagem confirmada
  2026-09-08, inclui os testes novos das features 008/009/010; a lista detalhada abaixo ainda
  reflete o estado até a feature 007 e não foi reconciliada linha a linha): unit/e2e/snapshot/sandbox/registry
  de `farol-core` (107, incluindo novos cenários de features 004–005, os 15 testes de sandbox da
  feature 006 — 7 em `sandbox_unit_tests`, construção pura do `Vec<String>` de argumentos do `bwrap`
  a partir de um `SandboxProfile`, sem spawnar nada de verdade; 8 em `sandbox_integration_tests`,
  spawn real de `bwrap`/`python3`/`docker ps` cobrindo rede negada/concedida, exec negado, o bind do
  `scan_root` de `git-local` e do socket Docker, e a confirmação de que `secrets.toml` é inacessível
  de dentro do sandbox — ambos os módulos dentro de `crates/farol-core/src/sandbox.rs` — e os 25
  novos testes da feature 007: 7 em `install::tests` (`install_succeeds_and_publishes_the_plugin`,
  `install_without_release_returns_no_release`,
  `install_with_failing_tarball_download_returns_download_failed`,
  `install_with_missing_manifest_returns_manifest_invalid`,
  `install_with_malformed_manifest_returns_manifest_invalid`,
  `install_with_name_colliding_with_a_reference_plugin_returns_name_collision`,
  `reinstalling_over_a_previous_install_replaces_it_cleanly`, todos contra o servidor HTTP local
  sintético de `install.rs`); 10 em `plugin_manifest::tests` cobrindo `parse_manifest` (arquivo
  ausente, TOML inválido, cada campo obrigatório ausente, nome vazio, lista de args vazia,
  capabilities ausentes default `false`, campo desconhecido ignorado, manifesto válido completo); 6
  em `plugin_worker::tests` cobrindo `discover_installed_plugins()`/`all_plugins()`
  (`discover_returns_empty_when_no_plugins_directory_exists`,
  `discover_returns_one_config_for_one_valid_plugin`,
  `discover_skips_a_malformed_manifest_without_blocking_the_valid_one`,
  `discover_does_not_filter_colliding_names_between_two_discovered_plugins`,
  `all_plugins_filters_a_discovered_plugin_colliding_with_a_reference_plugin`,
  `all_plugins_keeps_only_the_first_of_two_colliding_discovered_plugins`); 2 em `e2e_tests.rs`
  (`emulator_takes_plugin_template_through_a_real_handshake_to_ready`,
  `discovered_plugin_reaches_ready_through_the_real_state_machine`)) + contrato/
  unit de `farol-protocol` (25 `contract_schema_validation` + 18 `schema_boundaries` + 18 unit) +
  fixtures automatizadas do plugin `openfortivpn-vpn` (15 testes Python integrados ao harness) e
  `docker-containers` (23 testes Python).
  Suites Python: `python3 -m unittest discover -p "test_*.py"` dentro de `plugins/openfortivpn-vpn/`
  (15 testes) ou `plugins/docker-containers/` (23 testes).
- `cargo clippy --workspace --all-targets` — deve ficar limpo, sem warning nenhum.
- Harness de execução real em **duas camadas** (feature 003, `research.md` D1/D5,
  `contracts/e2e-harness-contract.md`) — a mesma máquina de estados real (`Program`/`Subscription`/
  handshake), verificada de duas formas complementares:
  - **Camada 1** — in-process, sem display, via `iced_test::Emulator`:
    `crates/farol-core/src/e2e_tests.rs` (módulo `#[cfg(test)]` dentro do bin — `farol-core` não tem
    target `lib`, então não existe `--test e2e_harness`; rodar com `cargo test --package farol-core
    e2e_tests`). `uptime-kuma` alcança `Ready` e popula `monitor-status-grid`
    (`uptime_kuma_reaches_ready_and_populates_the_monitor_grid`); `git-local`, migrado para o
    protocolo `"0.2"` (débito técnico #4, resolvido — commit `9d2fe77`, issue #4 fechada), percorre
    um handshake real até `Ready` como qualquer outro plugin
    (`emulator_takes_git_local_through_a_real_handshake_to_ready`) — deixou de terminar em
    `Unavailable{VersionIncompatible}`. Timeouts de 30s/120s por cenário (`## Clarifications` do
    `spec.md`).
    Mais 7 cenários automatizados na mesma sessão (T036-T042 da feature 002, commit `dbae06c`): tela
    de setup preenchida via UI (`Selector`/click/type do `iced_test`) leva a `Ready` com dados reais
    e persiste após reabertura
    (`setup_form_filled_via_ui_reaches_ready_with_real_data_and_persists_across_restart`); tela de
    setup não preenchida fica `NotConfigured` (`setup_form_left_unfilled_stays_not_configured`);
    `base_url` inválido gera `metrics_unreachable` sem sair de `Ready`
    (`uptime_kuma_reports_metrics_unreachable_for_an_invalid_base_url_but_stays_ready`); recuperação
    após a instância ficar inacessível e voltar, sem reiniciar o Farol
    (`uptime_kuma_recovers_after_instance_becomes_unreachable_without_restarting_farol`); resposta
    `/metrics` não reconhecível como Uptime Kuma vira `metrics_parse_error` sem derrubar o core
    (`uptime_kuma_reports_metrics_parse_error_for_a_non_metrics_response_without_crashing`); processo
    do plugin morto via `kill -9` vira `Crashed` sem derrubar o core
    (`uptime_kuma_process_killed_becomes_crashed_without_taking_down_the_core`); processo travado via
    `kill -STOP` vira `Unresponsive` sem derrubar o core
    (`uptime_kuma_process_frozen_becomes_unresponsive_without_taking_down_the_core`). Um oitavo
    cenário (T039, Cenário 6 de `quickstart.md` — instância acessível sem nenhum monitor cadastrado,
    `uptime_kuma_widget_reports_empty_items_when_instance_has_no_monitors`) não é mais `#[ignore]`d
    (débito técnico #5, issue #7 fechada, `specs/002-uptime-kuma-plugin/tasks.md` T051, 2026-09-02) —
    dois bugs reais corrigidos: (1) `plugins/uptime-kuma/metrics_parser.py::parse_metrics` não
    distinguia "zero monitores" de "resposta inválida" (corrigido reconhecendo a declaração `# HELP`/
    `# TYPE monitor_status` como sinal de instância real, mesmo sem amostras); (2) achado só depois
    de corrigir (1) — `farol_protocol::messages::WidgetItems` (`#[serde(untagged)]`) desserializa um
    `items: []` sempre como a primeira variante (`Git`), mesmo vindo de `monitor-status-grid`,
    fazendo `handle_widget_outcome` (`crates/farol-core/src/update.rs`) rotear o sucesso vazio para o
    campo errado de `PluginConnection` e nunca limpar `monitor_widget.last_error` — corrigido com
    `update::normalize_widget_items`, que usa o `kind` já conhecido do `widget_id` para resolver só o
    caso ambíguo (array vazio).
  - **Camada 2** — smoke do binário `farol` real (`fn main()`, backend de janela winit de verdade),
    via `tests/integration/harness.sh` — precisa de `xvfb` (`Xvfb`). Confirma 7 condições: (1) subir
    e sobreviver à janela de observação; (2) `uptime-kuma` chega a `Ready`; (3) `git-local` chega a
    `Ready` (migração para `"0.4"` na feature 005, antes era `"0.3"` — débito técnico #4 resolvido);
    (4) `openfortivpn-vpn` chega a `Ready` (feature 004, fixture determinística sem depender de
    `openfortivpn-gui` instalado); (5) `docker-containers` chega a `Ready` (feature 005, fixture
    determinística sem depender de Docker instalado/rodando na máquina de CI); (6) encerra em
    `SIGTERM`; (7) nenhum processo remanescente. Imprime `SUCESSO — 7/7 condições confirmadas em Ns`
    ou `[FALHA] ...` apontando a condição que caiu, saída `0`/`1`. `tests/integration/README.md`
    documenta o contrato; o script em si é a Camada 2, não mais um stub. Desde a feature 006, os 4
    plugins não sobem mais como subprocess direto do processo do Farol — rodam dentro de `bwrap`
    (`crates/farol-core/src/sandbox.rs`), e o script exporta o escape-hatch só de teste
    `FAROL_SANDBOX_TEST_EXTRA_BIND` (D14 de `specs/006-sandbox-permissoes-bubblewrap/research.md`)
    apontando para o diretório de trabalho onde grava a transcrição JSON-RPC, senão o shim de teste
    (sob `/tmp`, coberto pela `--tmpfs /tmp` privada do sandbox) fica invisível de dentro do processo
    sandboxed.
  - Sob `iced 0.14`, um closure capturante em `Subscription::map` (a armadilha histórica acima) não
    chega a rodar — vira erro `E0080` de compilação, apanhado por `cargo test`/`cargo clippy
    --all-targets` (código de teste) ou já no primeiro passo do `harness.sh` (código de produção).
- Gerador de casos de borda de contrato — `crates/farol-protocol/tests/schema_boundaries.rs`,
  `numeric_and_null_boundary_cases()`: deriva de cada JSON Schema (`protocol/schema/v0.2/*`) os
  valores de fronteira que o schema permite mas a implementação Rust pode rejeitar (`Option<T>` vs.
  `"type": [..., "null"]`, ausência de `minimum`/`maximum`, etc.), sem hardcodar caso por caso —
  `contracts/contract-boundary-testing.md` é o contrato normativo. `MonitorStatusItem.response_time_ms`
  (`widget.schema.json`) costumava permitir `-1` (sem `minimum`) enquanto `Option<u32>`
  (`messages.rs`) rejeitava — débito técnico que ficava provado por um teste `#[ignore]`d, rastreado
  como issue #5. Fechado declarando `minimum: 0` no schema (não alterando o tipo Rust — nenhum
  plugin real precisa de valor negativo, e o Uptime Kuma já converte seu sentinela `-1` para `null`
  do lado Python em `metrics_parser.py`); o teste foi reescrito como
  `widget_monitor_status_item_response_time_ms_minimum_boundaries` (sem `#[ignore]`), no mesmo padrão
  de `widget_remote_status_tracked_ahead_and_behind_minimum_boundaries` para um campo com `minimum`
  declarado.
- Verificação visual declarativa — `crates/farol-core/src/visual_snapshot_tests.rs` +
  `crates/farol-core/src/snapshots/*.snap` (via `insta`, `cargo test --package farol-core
  visual_snapshot_tests`): compara `extract_visible_text(app.view())` contra um snapshot textual por
  `screen_id` (`data-model.md` §3, extensível por FR-013 de `visual-snapshot-contract.md`) para os
  quatro estados cobertos — `DashboardReady`, `SetupForm`, `VersionIncompatible`
  (`dashboard_ready_state`/`setup_form_state`/`version_incompatible_state`) e, desde T045
  (`specs/002-uptime-kuma-plugin/tasks.md`, 2026-09-02), `MonitorWidgetError`
  (`monitor_widget_error_state`) — erro pontual do widget `monitor-status-grid`
  (`monitor_widget.last_error`) com uma lista de uma leitura anterior preservada, distinto tanto de
  "0 monitores, sem erro" quanto de "lista populada, sem erro". Revisar/aceitar um snapshot alterado
  intencionalmente: `cargo insta review` (requer `cargo install cargo-insta`, não é pré-requisito pra
  rodar a suíte).
- `tests/contract/` (raiz do repo, fora de qualquer crate) permanece só documentação — `cargo test`
  não o descobre (Cargo só compila `tests/*.rs` dentro de cada crate).
- Validação manual via `eprintln!` de diagnóstico temporário (usada até a feature 002) foi
  substituída pela infraestrutura acima — não é mais o caminho recomendado para validar um cenário
  de `quickstart.md`. Rodar o binário `farol` manualmente (Camada 2 ou fora do harness) ainda precisa
  de um `DISPLAY` X11 funcional; sob Xvfb sem WM/GPU real a janela não renderiza visualmente (fica
  preta), mas o ciclo `update`/handshake/transição de estado roda normalmente — é isso que
  `harness.sh` observa sem instrumentar código de produção (ver cabeçalho do script para o método).

## Python (plugins)

`plugins/*/pyproject.toml` configura `ruff` (lint). Rodar `ruff check` dentro do diretório do
plugin antes de considerar uma mudança Python pronta.

- `git-local` (feature 001): testes em `tests/unit/test_git_local_scan.py` (raiz do repo,
  `unittest`, ver `tests/unit/README.md`) — repositórios git reais em diretórios temporários, não
  mocks de subprocess.
- `uptime-kuma` (feature 002): testes em `tests/unit/test_uptime_kuma_*.py` (`unittest`, D7 de
  `research.md`) — mesmo padrão de layout que `git-local` (issue #8; movidos de
  `plugins/uptime-kuma/test_*.py`, ver a nota de T046 em `specs/002-uptime-kuma-plugin/tasks.md`).
  Cada arquivo insere `plugins/uptime-kuma/` em `sys.path` no import, seguindo o padrão de
  `tests/unit/test_git_local_scan.py` para `git-local`. Rodar tudo junto (a partir da raiz do repo):
  `pytest tests/unit/test_uptime_kuma_*.py -v` (ou `python3 -m unittest discover -s tests/unit -p
  "test_uptime_kuma_*.py"`) — 21 testes — `test_uptime_kuma_metrics_parser.py` (parsing Prometheus,
  mapeamento de status FR-012), `test_uptime_kuma_poller.py` (cache/erro do poller,
  `metrics_client.fetch_metrics` mockado via `unittest.mock`),
  `test_uptime_kuma_config.py`/`test_uptime_kuma_secrets.py` (leitura de
  `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL`/`_API_KEY` via `unittest.mock.patch.dict(os.environ, ...)`,
  nunca segredo real).
- `openfortivpn-vpn` (feature 004): testes colocados junto do código (`plugins/openfortivpn-vpn/
  test_vpn_cli.py`, `unittest`, mesmo padrão de feature 002). Rodar: `python3 -m unittest discover
  -p "test_*.py"` dentro de `plugins/openfortivpn-vpn/` (15 testes) — `test_vpn_cli.py` cobre
  `query_status()` (conectado/desconectado/vazio/binário ausente/erro), `connect()`
  (sucesso/seis codes de erro com tradução), `disconnect()` (sucesso/três codes de erro).
