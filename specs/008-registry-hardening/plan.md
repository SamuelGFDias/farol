# Implementation Plan: Registry — Índice Central, Instalação com Build e Capability de Filesystem Genérica

**Branch**: `008-registry-hardening` | **Date**: 2026-09-07 | **Spec**: [spec.md](./spec.md)

**Input**: Feature specification from `/specs/008-registry-hardening/spec.md`

## Summary

Fecha o débito técnico das issues #15, #16, #17 e #18 registrado como Out of Scope da feature 007.
O core ganha resolução de nome→`owner/repo` via um repositório-índice GitHub real (novo, publicado
nesta feature, com CI de validação de PR), o manifesto `PluginManifest` ganha um campo `build`
(comando executado localmente pelo instalador após extrair o código-fonte, antes de mover para o
diretório final) e campos de capability de filesystem genérica (`filesystem_read`/
`filesystem_read_write`, caminhos absolutos, validados contra uma denylist de caminhos sensíveis), e
a instalação (hoje só `farol install <owner>/<repo>` via CLI) ganha um fluxo equivalente in-app na UI
do `iced`, reaproveitando a mesma função `install::run`.

## Technical Context

**Language/Version**: Rust (workspace já em uso); TOML para o manifesto (extensão do formato já
existente, mesma crate `toml`)

**Primary Dependencies**: `curl`/`tar` (binários externos já usados pela feature 007, sem crate Rust
nova) para download/extração; o índice central é buscado como um arquivo TOML estático via `curl`
(mesmo padrão), sem dependência de API/paginação. Execução do comando de `build` via
`std::process::Command::new("sh").arg("-c").arg(&manifest.build)`, dentro do `staging_dir`.

**Storage**: mesmo diretório `$XDG_DATA_HOME/farol/plugins/<nome>/` da feature 007. Denylist de
caminhos sensíveis (`~/.ssh`, `/etc`, o diretório de `secrets_store`/`config_store` do próprio Farol)
como constante estática em `farol-core`, sem arquivo de configuração externo nesta fase.

**Testing**: `cargo test --workspace`. Novo: testes de unidade para `validate_filesystem_paths`
(caminho relativo rejeitado, caminho na denylist rejeitado, caminho absoluto fora da denylist aceito);
testes de unidade/integração para execução do `build` (sucesso, falha com `exit code != 0`, rollback
atômico do `staging_dir` em caso de falha) usando comandos de fixture (`true`/`false`/`exit 1`) em vez
de toolchains reais; teste de resolução de índice contra servidor HTTP local sintético (mesmo padrão
de `MetricsFixtureServer`/D8 da feature 007), nunca contra o GitHub real.

**Target Platform**: Linux (Princípio I).

**Project Type**: Aplicação desktop nativa (core, `iced` 0.14 com feature `tokio`) + plugins como
processos separados. Esta feature adiciona um fluxo de UI (US4) que reaproveita a mesma lógica de
instalação da CLI — nenhuma mudança de tipo de projeto.

**Performance Goals**: sem meta numérica dura (mesma decisão da feature 007) — instalação não é
operação de alta frequência. A UI in-app (US4) MUST manter a interface responsiva durante o download/
build (execução assíncrona via `Command::perform` do `iced`, aproveitando a feature `tokio` já
habilitada em `Cargo.toml`), nunca travando a thread de UI.

**Constraints**: o comando de `build` roda sem sandboxing dedicado (decisão de escopo, ver Out of
Scope do spec.md) — o usuário que instala um plugin de terceiro com passo de build está
implicitamente confiando no autor do plugin tanto quanto já confia ao rodar o binário instalado.
Rate limit não-autenticado do GitHub (60 req/hora por IP, D9 da feature 007) também se aplica ao
fetch do índice.

**Scale/Scope**: as 4 User Stories (US1 índice, US2 build, US3 filesystem, US4 UI in-app) são
razoavelmente independentes entre si e podem ser implementadas em paralelo por executores distintos,
desde que todas convirjam sobre `plugin_manifest.rs`/`install.rs` sem conflito de edição simultânea
(risco de conflito de arquivo — ver Complexity Tracking).

## Constitution Check

*GATE: Must pass before Phase 0 research. Re-check after Phase 1 design.*

- **Princípio VII (Registry Federado sem Infra Própria)**: esta feature cria um repositório-índice
  GitHub — infraestrutura de descoberta, não de hospedagem de plugins (os plugins continuam
  hospedados nos próprios repositórios de seus autores, via release do GitHub, como na feature 007).
  O índice é só uma lista pública `owner/repo` por nome, versionada por PR com CI de validação —
  consistente com "sem infra própria" (o índice é um repositório Git público, não um serviço).
  PASS.
- Nenhuma mudança de protocolo `farol-protocol` (o manifesto de instalação é um artefato local TOML,
  não faz parte do JSON-RPC entre core e plugin) — sem impacto de compatibilidade retroativa. PASS.
- Débito técnico remanescente desta feature (se houver, ex.: UI de aprovação/revisão de capabilities
  antes da instalação) MUST ser registrado como issue nova ao final, seguindo a disciplina já
  praticada pelas features 005/006/007.

## Project Structure

### Documentação desta feature

```
specs/008-registry-hardening/
├── spec.md              # já existente
├── plan.md              # este arquivo
├── checklists/
│   └── requirements.md  # já existente
└── tasks.md             # gerado a seguir por /speckit-tasks (ou equivalente do arquiteto)
```

Nota de escopo: esta feature não gera `research.md`/`data-model.md`/`contracts/`/`quickstart.md`
separados como a feature 007 gerou — as decisões de design já investigadas estão consolidadas neste
`plan.md` (Technical Context acima e Key Design Decisions abaixo), proporcional ao tamanho da mudança
(extensão de um manifesto já existente, não um formato novo do zero).

### Código afetado

```
crates/farol-core/src/
├── plugin_manifest.rs   # ESTENDER: PluginManifest/RawManifest ganham `build: Option<String>`,
│                        # `filesystem_read: Vec<String>`, `filesystem_read_write: Vec<String>`
│                        # (default vazio); nova fn `validate_filesystem_paths` (denylist)
├── registry_index.rs    # NOVO: busca/parseia o índice central (TOML via curl), resolve nome→
│                        # (owner, repo); struct `RegistryIndexError`
├── install.rs           # ESTENDER: `run_by_name(name)` (resolve via registry_index, chama `run`);
│                        # execução do `build` após parse do manifesto, antes do rename atômico;
│                        # popular `extra_binds` do `SandboxProfile` a partir de
│                        # `filesystem_read`/`filesystem_read_write` (hoje sempre vazio em
│                        # `discover_installed_plugins`, `plugin_worker.rs:266-271`)
├── plugin_worker.rs     # AJUSTAR: `discover_installed_plugins` passa a repassar os binds de
│                        # filesystem do manifesto para o `SandboxProfile`, validados
├── main.rs              # ESTENDER: novas variantes de `Message` para instalação in-app
│                        # (ex.: `InstallByNameSubmitted`, `InstallOutcomeReceived`), roteamento em
│                        # `update.rs`
├── model.rs             # ESTENDER: estado da tela/formulário de instalação in-app (nome digitado,
│                        # status: idle/em progresso/sucesso/erro)
└── view.rs              # ESTENDER: `view_install_form` (reaproveita o padrão visual de
                          # `view_setup_form`, `view.rs:562-582`, para consistência)
```

### Infraestrutura externa (fora do monorepo)

```
<owner>/farol-plugin-index (repositório GitHub NOVO, nome/owner a confirmar antes de criar)
├── index.toml                       # lista `[[plugin]] name / owner / repo`
├── .github/workflows/validate.yml   # CI: valida formato do PR (nome único, owner/repo bem-formado,
│                                     # manifesto do repo referenciado é alcançável e parseável)
└── README.md                        # instruções de como submeter um plugin ao índice
```

**Checkpoint sensível**: criar e publicar este repositório é uma ação visível externamente (GitHub
público). O arquiteto MUST confirmar com o usuário o nome/owner exatos antes de criar, mesmo que o
restante da feature 008 já esteja implementado e verificado — não presumir autorização implícita
para esta ação especificamente.

## Key Design Decisions (Phase 0+1 consolidado)

- **D1 (US1)**: o índice é um único arquivo `index.toml` na raiz do repositório-índice — mais simples
  de validar por CI (parse + checagem de unicidade de `name`) e de buscar (`curl` de uma URL raw do
  GitHub) do que uma API paginada. Convergente com D5/D9 da feature 007 (sem dependência HTTP nova).
- **D2 (US1)**: colisão de nome dentro do índice é responsabilidade da CI do repositório-índice
  (rejeita PR que introduza um `name` já existente) — não é responsabilidade do core em tempo de
  instalação, que sempre resolve um nome para exatamente um `owner/repo` ou falha com "nome não
  encontrado".
- **D3 (US2)**: o comando de `build` roda via `sh -c` no `staging_dir` (mesmo diretório onde o
  manifesto já é validado, antes do `rename` atômico para o diretório final, `install.rs:82-100`) —
  qualquer falha (`exit code != 0`) aborta a instalação sem mover nada para o diretório final,
  reaproveitando o mecanismo de limpeza que já existe para `ManifestInvalid`.
- **D4 (US3)**: `filesystem_read`/`filesystem_read_write` do manifesto viram `BindMount` (tipo já
  existente em `sandbox.rs`, hoje só populado manualmente para os 4 plugins de referência) — a
  validação de denylist ocorre em `plugin_manifest.rs` (mesma camada que já valida o resto do
  manifesto), não em `sandbox.rs` (que continua sem saber a origem dos binds, só os consome).
- **D5 (US4)**: a UI in-app chama a mesma `install::run`/`install::run_by_name` já usada pela CLI —
  nenhuma lógica de instalação duplicada. A chamada é envolvida em `iced::Task`/`Command::perform`
  (a decidir o nome exato da API conforme a versão 0.14 do `iced`) rodando a função bloqueante
  (`curl`/`tar`/`sh -c`) via `tokio::task::spawn_blocking`, já que a feature `tokio` do `iced` está
  habilitada em `Cargo.toml:13-14`.

## Complexity Tracking

| Risco | Mitigação |
|---|---|
| US1/US2/US3/US4 convergem sobre `plugin_manifest.rs`/`install.rs` | Implementar em sequência (não em paralelo) para esses dois arquivos: Foundational (extensão do manifesto, US1 D1/D2) primeiro, depois US2/US3 (ambas tocam `install.rs`, sequenciais), US4 por último (só consome as funções já prontas) |
| Criação de repositório GitHub novo é ação externa visível | Checkpoint explícito de confirmação com o usuário antes de criar (ver Project Structure acima) — não bloqueia US2/US3/US4, que não dependem do índice existir de fato para serem implementadas e testadas (US1 usa servidor HTTP de fixture local no teste automatizado) |
| API exata de `Command::perform`/`Task` pode ter mudado entre versões do `iced` | Executor de US4 MUST confirmar a API atual lendo a documentação/exemplos já usados em `main.rs` antes de implementar, não assumir a nomenclatura de versões antigas do `iced` |
