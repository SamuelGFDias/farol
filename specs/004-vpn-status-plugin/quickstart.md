# Quickstart: Validação do Plugin de Status de VPN (openfortivpn-gui)

**Feature**: `004-vpn-status-plugin` | **Data**: 2026-09-03

Guia para validar manualmente, ponta a ponta, as três User Stories da spec depois que a feature
estiver implementada (`/speckit-tasks` + `/speckit-implement` — não cobertos por este plano). Não
contém código de implementação — apenas comandos e resultados esperados, referenciando `contracts/`
e `data-model.md`. Segue o mesmo padrão de `specs/002-uptime-kuma-plugin/quickstart.md`.

**Nota (sessão de automação, 2026-09-03)**: a feature está totalmente implementada (User Stories 1,
2 e 3 completas — `cargo test --workspace`: 109 testes, 0 falhas; `python3 -m unittest` do plugin:
15 testes, 0 falhas). A maioria dos 7 Cenários abaixo já tem equivalente automatizado permanente,
que não precisa ser reproduzido à mão para validar a feature:

- **Cenário 2** (já conectado por fora do Farol, com perfil e duração de sessão populados) e
  **Cenário 5** (duração da sessão visível, `elapsed_seconds`): cobertos por
  `crates/farol-core/src/e2e_tests.rs::openfortivpn_vpn_reaches_ready_and_populates_the_vpn_widget`
  (T026b) — `Emulator` real, processo Python real, fixture `tests/fixtures/fake-openfortivpn-gui/`
  simulando `status --json` com estado `connected`, perfil ativo, dois perfis conhecidos e 125s de
  sessão — e por `crates/farol-core/src/visual_snapshot_tests.rs::vpn_widget_connected_screen_
  matches_snapshot` (T027), que fixa a tela renderizada desse mesmo estado (perfil ativo, botão de
  desconectar, duração formatada).
- **Cenário 4** (tradução de erro de ação para PT-BR): coberto por
  `plugins/openfortivpn-vpn/test_vpn_cli.py`, que exercita os 6 `error.code`/`data.detail.cli_code`
  de `contracts/openfortivpn-cli-mapping.md` (perfil não encontrado, já conectado, timeout, sudo
  negado em connect/disconnect, erro interno em disconnect) na tradução feita por
  `vpn_cli.py::query_status`/`connect`/`disconnect`, nível da CLI wrapper.
- **Cenário 6** (`openfortivpn-gui` ausente do `PATH`): coberto por
  `plugins/openfortivpn-vpn/test_vpn_cli.py::test_binary_missing_returns_exec_unavailable_marker`
  — confirma que a ausência do binário produz o marcador `exec_unavailable` (`-32003`), distinto de
  "desconectado", no nível da CLI wrapper. **Não** há um teste equivalente em `e2e_tests.rs` que
  exercite esse caminho através do app `farol-core` real (`PATH` reduzido + widget renderizado) —
  o único teste e2e do plugin (T026b, acima) simula apenas o caminho feliz (`status`
  `connected`), então a exibição desse erro específico na tela do Farol depende de validação
  manual completa deste Cenário, ou de uma extensão futura de `e2e_tests.rs`.
- **Cenário 7** (regressão `git-local`/`uptime-kuma` continuam chegando a `Ready`): coberto pelos
  testes e2e já existentes dessas duas features (`git_local_reaches_ready...`,
  `uptime_kuma_reaches_ready_and_populates_the_monitor_grid`, em `e2e_tests.rs`), agora rodando sob
  protocolo `"0.3"`, mais a quinta/sexta condição de `tests/integration/harness.sh` (Camada 2,
  T039) confirmando `openfortivpn-vpn` chegando a `Ready` sob Xvfb.

Os Cenários que **dependem de validação manual completa**, sem equivalente automatizado permanente
no app real:
- **Cenário 1** (ver estado desconectado ao abrir, sem nenhuma conexão ativa): o único teste e2e do
  plugin (T026b) simula estado `connected`, não `disconnected` — o caminho "populado, mas
  desconectado, com lista de perfis" não tem cobertura automatizada equivalente no nível do app.
- **Cenário 3** (conectar e desconectar pelo próprio widget, via `vpn.connect`/`vpn.disconnect`):
  nenhum teste e2e invoca essas ações através do `Emulator` contra um subprocess real (a fixture
  `tests/fixtures/fake-openfortivpn-gui/` cobre o *protocolo* JSON-RPC do plugin, não o fluxo
  completo de clique → `action/invoke` → estado "conectando" → conclusão, e muito menos a
  integração real com uma instalação verdadeira de `openfortivpn-gui`). Exige uma instalação real
  do `openfortivpn-gui` com ao menos um perfil VPN configurado para validação manual completa —
  este é o Cenário que mais depende de execução manual ponta a ponta.
- **Cenário 6**, na parte específica de exibição na UI do Farol (ver acima — a tradução do erro em
  si está coberta por `test_vpn_cli.py`, mas não sua renderização na tela real).

## Pré-requisitos

- Rust estável e `cargo`, `farol-core`/`farol-protocol` já falando `protocol_version = "0.3"`
  (`plan.md` § Project Structure, `research.md` D2) — incluindo `git-local`/`uptime-kuma` migrados
  para `"0.3"` (mecânico, sem mudança de wire da parte deles).
- Python 3 no `PATH` (plugin `plugins/openfortivpn-vpn`).
- `openfortivpn-gui` instalado, com a interface CLI da feature `001-add-cli-interface` daquele
  projeto (`openfortivpn-gui status|connect|disconnect --json`) disponível no `PATH` da mesma
  máquina.
- Ao menos um perfil VPN já configurado no `openfortivpn-gui` (para exercitar os Cenários 2-4); um
  segundo perfil ajuda a exercitar o seletor de múltiplos perfis (Cenário 2, FR-008).
- **Nenhuma configuração prévia de arquivo é necessária** — `openfortivpn-vpn` não tem
  `required_config` (`research.md` D6); não passa por tela de setup, fica `Ready` assim que o
  handshake completa.

## Setup (uma vez, após implementação)

```bash
cargo build --workspace
```

(o plugin `openfortivpn-vpn` é Python puro — nenhum passo de build necessário).

## Cenário 1 — Ver o estado ao abrir, sem nenhum perfil conectado (User Story 1, P1)

```bash
openfortivpn-gui disconnect 2>/dev/null   # garante estado desconectado antes do teste
cargo run -p farol-core
```

**Esperado** (Acceptance Scenario 1 de US1, SC-001):
1. O widget "VPN" aparece populado sem tela de setup nenhuma.
2. Estado mostrado: "desconectado".
3. A lista de perfis disponíveis aparece (ou uma indicação explícita de "nenhum perfil configurado"
   se `profiles: []`, FR-003).

## Cenário 2 — Ver o estado com uma conexão já ativa por fora do Farol (User Story 1, P1)

```bash
openfortivpn-gui connect <perfil>          # fora do Farol, via CLI ou GUI
cargo run -p farol-core
```

**Esperado** (Acceptance Scenario 2 de US1, SC-001/SC-002):
1. Ao abrir, o widget já mostra "conectado" e o nome do perfil ativo — sem precisar de nenhuma ação
   do usuário no Farol (o Farol nunca iniciou essa conexão).
2. Desconectar pela CLI/GUI (fora do Farol) e aguardar um ciclo de refresh (`suggested_
   refresh_interval_ms`, default 30000ms): o widget volta a "desconectado" sozinho (Acceptance
   Scenario 3 de US1).

## Cenário 3 — Conectar e desconectar pelo próprio widget (User Story 2, P2)

Pré-condição: estado "desconectado", ao menos um perfil disponível.

**Esperado** (Acceptance Scenarios 1-2 de US2):
1. Clicar em conectar num perfil (`vpn.connect`, `research.md` D4): o widget mostra "conectando"
   imediatamente (`VpnWidgetViewModel.connect_in_flight`, D7) e, ao concluir, "conectado" com o
   perfil correto — sem travar a janela do Farol durante a espera (~20s, `contracts/
   openfortivpn-cli-mapping.md`).
2. Clicar em desconectar: o widget volta a "desconectado".

## Cenário 4 — Falhas de ação traduzidas (User Story 2, Edge Cases)

Reproduzir ao menos dois dos seis `error.code` da CLI (`contracts/openfortivpn-cli-mapping.md`):

```bash
# already_connected: conectar de novo enquanto já conectado (fora do Farol, então tentar pelo widget)
openfortivpn-gui connect <perfil>
# no widget do Farol: tentar conectar ao mesmo (ou outro) perfil
```

**Esperado** (Acceptance Scenario 3 de US2, SC-003, FR-007): o widget mostra uma mensagem legível em
PT-BR (tabela de `contracts/openfortivpn-cli-mapping.md`), preserva o último estado conhecido, e o
Farol continua respondendo normalmente — sem travar nem exigir reiniciar.

Repetir para `profile_not_found` (tentar conectar a um nome inexistente, se a UI permitir digitar) e
`connect_timeout` (mais difícil de reproduzir de propósito — validar ao menos por leitura de código
que o `data.detail.cli_code` chega intacto até a UI).

## Cenário 5 — Duração da sessão visível (User Story 3, P3)

```bash
openfortivpn-gui connect <perfil>
sleep 120
cargo run -p farol-core
```

**Esperado** (Acceptance Scenario 1 de US3): o widget mostra há quanto tempo a sessão está ativa
(≥ 2 minutos), sem exigir nenhuma ação adicional do usuário — `VpnStatusItem.elapsed_seconds`
(`data-model.md` §1.3).

## Cenário 6 — `openfortivpn-gui` ausente do `PATH` (Edge Case)

```bash
PATH=/usr/bin:/bin cargo run -p farol-core   # PATH reduzido, sem o diretório do openfortivpn-gui
```

**Esperado** (FR-009, `research.md` D5): o widget mostra um erro claro e distinto de "desconectado"
(`-32003`/`exec_unavailable`) — não confunde ausência da ferramenta com falta de conexão.

## Cenário 7 — Regressão: widgets existentes continuam funcionando (SC-004)

```bash
cargo run -p farol-core
```

**Esperado**: `git-local` e `uptime-kuma` chegam a `Ready` normalmente (agora falando `"0.3"`,
`research.md` D2) — nenhuma mudança de comportamento observável nesses dois widgets.

## Automação equivalente (a preencher durante `/speckit-implement`)

Como nas features 001-002, os Cenários acima devem, quando possível, ganhar equivalentes
automatizados em `crates/farol-core/src/e2e_tests.rs` (Camada 1, `iced_test::Emulator`, mesmo padrão
de `uptime_kuma_reaches_ready_and_populates_the_monitor_grid`) e uma condição adicional em
`tests/integration/harness.sh` (Camada 2) confirmando que `openfortivpn-vpn` chega a `Ready` — a
diferença é que aqui a "fixture" não é um servidor HTTP local (como `MetricsFixtureServer` do
uptime-kuma), e sim um `openfortivpn-gui` real ou um binário `openfortivpn-gui` de teste no `PATH`
que simula as respostas JSON do contrato (`contracts/openfortivpn-cli-mapping.md`) sem abrir um
túnel de verdade — decisão de como simular isso fica para `research.md`/`tasks.md` da fase de
implementação, não deste `quickstart.md`.
