# Contrato: Workflow de CI (US3)

**Feature**: `003-automated-testing-infrastructure` | Decisão de origem: `research.md` D6

**Local**: `.github/workflows/ci.yml` (repositório não tem nenhum workflow hoje).

## Gatilhos (FR-007)

```yaml
on:
  push:
  pull_request:
```

## Jobs (paralelos, `runs-on: ubuntu-latest`)

| Job | Comando principal | Depende de | Cobre |
|---|---|---|---|
| `rust-test` | `cargo test --workspace` | — | Suíte existente (71 testes) + `contracts/contract-boundary-testing.md` (US2) + `contracts/e2e-harness-contract.md` Camada 1 (US1) + `contracts/visual-snapshot-contract.md` (US4) — todos rodam sob o mesmo comando, sem harness externo. |
| `rust-lint` | `cargo clippy --workspace --all-targets -- -D warnings` | — | Checagem de estilo Rust (FR-007), mesmo padrão "sem warning nenhum" de `AGENTS.md`. |
| `rust-smoke` | `apt-get install -y xvfb` + `cargo build --bin farol` + `tests/integration/harness.sh` | — | `contracts/e2e-harness-contract.md` Camada 2 (US1, smoke de processo real). |
| `python-lint` | `ruff check` em cada `plugins/*/` | — | Checagem de estilo Python (FR-007); roda em todo diretório de plugin, mesmo os sem `pyproject.toml` próprio (usa defaults do `ruff` nesse caso — nota de `research.md` D6). |
| `python-test` | `pytest` em cada `plugins/*/` que tiver testes | — | Testes Python existentes (hoje só `plugins/uptime-kuma/`). |

Nenhum job depende de outro (`needs:` vazio) — maximiza paralelismo dentro do orçamento de 10
minutos (SC-004).

## Saída/visibilidade (FR-008)

Cada job aparece como um *check* individual da mudança (mecanismo nativo do GitHub Actions/GitHub
PR UI) — falha de qualquer um sinaliza a mudança como não pronta antes de revisão manual (Acceptance
Scenario 2 de US3); todos verdes é a confirmação positiva de Acceptance Scenario 3, sem exigir rodar
nada localmente.

## Extensibilidade (FR-013)

- Novo plugin de referência: mais um subdiretório sob `plugins/*/`, automaticamente coberto por
  `python-lint`/`python-test` (glob, não lista hardcoded de diretórios) sem editar o workflow.
- Nova versão de schema de protocolo: `rust-test` já cobre qualquer novo teste de contrato
  adicionado em `crates/farol-protocol/tests/`, sem mudança de workflow.
- Novo `screen_id` de verificação visual: coberto por `rust-test` sem mudança de workflow (contrato
  de `visual-snapshot-contract.md`).

## Fora do escopo deste contrato

Publicação de artefato de release, deploy, notificação externa (Slack/e-mail) — nenhum FR desta
feature pede isso; `## Out of Scope` de `spec.md` não menciona, e não há necessidade técnica
identificada nesta sessão de planejamento.
