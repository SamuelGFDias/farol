# Quickstart: Validação do Walking Skeleton

**Feature**: `001-walking-skeleton-git-plugin` | **Data**: 2026-08-31

Guia para validar manualmente, ponta a ponta, as três User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando
`contracts/` e `data-model.md`.

## Pré-requisitos

- Rust estável ≥ 1.75 e `cargo` (para `farol-core` e `farol-protocol`, ver `plan.md` § Project
  Structure).
- Python 3.11+ no `PATH` (para o plugin de referência `plugins/git-local`, D3 de `research.md`).
- Binário `git` no `PATH`.
- Um diretório de teste com pelo menos:
  - um repositório git com remoto configurado e branch atrás do upstream (para exercitar
    `ahead`/`behind` não-zero e a ação de fetch habilitada);
  - um repositório git **sem** remoto configurado (para exercitar o estado `no_remote` e a ação de
    fetch desabilitada — FR-014);
  - um repositório git com working tree suja (mudança não commitada);
  - opcionalmente, um subdiretório sem `.git` nenhum (para confirmar que não aparece na lista).
- Arquivo de configuração do plugin apontando para esse diretório de teste (ver
  `contracts/git-local-plugin.md` § Configuração):

  ```bash
  mkdir -p ~/.config/farol/plugins/git-local
  cat > ~/.config/farol/plugins/git-local/config.toml <<'EOF'
  scan_root = "/caminho/para/diretorio-de-teste"
  EOF
  ```

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

(o plugin `git-local` é Python puro — nenhum passo de build necessário além de `chmod +x` no
entrypoint, se aplicável).

## Cenário 1 — Ver o estado dos repositórios ao abrir (User Story 1, P1)

```bash
cargo run -p farol-core
```

**Esperado** (Acceptance Scenarios 1–3 de US1, SC-001):
1. Uma única janela nativa abre (FR-001).
2. O widget "Repositórios Git" aparece populado com todos os repositórios do diretório de teste,
   cada um mostrando estado de working tree (suja/limpa) e ahead/behind — ou "sem remoto" para o
   repositório sem remote configurado, de forma visualmente distinguível de "0 ahead / 0 behind"
   (FR-014).
3. Sem tocar em nada, após ~30s (ou o intervalo sugerido pelo plugin — `contracts/handshake.md`),
   os números se atualizam sozinhos (FR-011, SC-002) — confirmar alterando o estado de um repo
   (ex.: `git commit` num repo antes limpo) e observando o próximo ciclo refletir `dirty: true` sem
   reiniciar o Farol.

## Cenário 2 — Handshake com versão incompatível (Acceptance Scenario 4 de US1, SC-005)

Simular declarando, num plugin de teste (ou editando temporariamente a resposta do plugin de
referência), `protocol_version: "9.9"` no `HandshakeHelloResult`.

**Esperado**: nenhum widget desse plugin é renderizado; o core exibe mensagem legível citando as
duas versões (`contracts/framing-and-versioning.md`); o core não trava nem cai.

## Cenário 3 — Disparar `git fetch` pela UI (User Story 2, P2, SC-003)

1. No widget, escolher o repositório com remoto configurado e branch atrás do upstream.
2. Acionar a ação "Fetch" exposta para aquele repositório (visível porque
   `fetch_action.enabled == true` — FR-015).
3. **Esperado**: o core invoca `action/invoke` (`contracts/action-protocol.md`); ao concluir, o
   `ahead`/`behind` exibido para aquele repositório reflete o resultado do fetch, sem o usuário
   sair do Farol (FR-016, FR-018).
4. Repetir com o repositório **sem** remoto: confirmar que a ação de fetch aparece **desabilitada**
   (não omitida) para esse repositório — não há como dispará-la pela UI (FR-014, FR-015).
5. Simular falha de rede (ex.: desconectar a rede, ou apontar um remote inválido) e disparar fetch
   num repositório com remoto: **esperado** — erro estruturado exibido associado ao repositório
   (FR-017, FR-018), sem travar a janela nem derrubar o core.

## Cenário 4 — Plugin morre (User Story 3, P3, SC-004)

```bash
pgrep -f "plugins/git-local" | xargs kill -9
```

**Esperado** (Acceptance Scenarios 1–2 de US3):
1. A janela do Farol continua aberta e responsiva.
2. O widget/estado do plugin passa a exibir "indisponível" (`PluginState::Unavailable{Crashed}` —
   `data-model.md` § 2.1), de forma distinguível de "carregando"/"sem dados".

## Cenário 5 — Plugin travado, vivo mas sem resposta (Acceptance Scenario 3 de US3)

Simular travamento (ex.: enviar `SIGSTOP` ao processo do plugin):

```bash
pgrep -f "plugins/git-local" | xargs kill -STOP
```

**Esperado**: o core não fica bloqueado esperando indefinidamente; após o `RPC_TIMEOUT_CONTROL`
(default 5s, D6 de `research.md` — orçamento de controle, usado por `widget/get`/handshake, não o
orçamento de ação `RPC_TIMEOUT_ACTION`) não receber resposta no próximo ciclo de refresh, o plugin
é sinalizado como indisponível (`Unavailable{Unresponsive}`), e o restante da janela segue
respondendo. Limpar com `kill -CONT` ao final do teste.

## Cenário 6 — Binário do plugin ausente / falha ao iniciar (Edge Case da spec)

Apontar temporariamente a configuração de spawn do core para um caminho de plugin inexistente (ou
renomear o entrypoint do plugin) e iniciar `farol-core`.

**Esperado**: o core não cai; o plugin é sinalizado como indisponível desde o início
(`Unavailable{FailedToStart}`), sem nenhum widget renderizado para ele.

## Cenário 7 — `git` ausente do sistema (Edge Case da spec)

Renomear/remover temporariamente `git` do `PATH` visível ao plugin (ex.: rodar com um `PATH`
restrito).

**Esperado**: o plugin reporta erro `exec_unavailable` (`-32003`, `contracts/error-model.md`) nas
operações que dependem de `git`, sem que o processo do plugin morra — o core continua enxergando o
plugin como `Ready`, só a operação específica falha.

## Critério de "walking skeleton provado"

Todos os 7 cenários acima passam ⟹ os contratos estruturais do Farol (handshake + versionamento,
widget declarativo, ação round-trip, isolamento de falha nas suas duas formas) estão validados com
um consumidor real e não-Rust do protocolo — objetivo desta feature (`spec.md`, Input).
