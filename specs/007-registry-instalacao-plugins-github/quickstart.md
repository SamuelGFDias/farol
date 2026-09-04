# Quickstart: Validação do Registry de Instalação de Plugins

**Feature**: `007-registry-instalacao-plugins-github` | **Data**: 2026-09-04

Guia para validar manualmente, ponta a ponta, as três User Stories da spec depois que a feature
estiver implementada. Mesmo padrão de `specs/006-sandbox-permissoes-bubblewrap/quickstart.md`.

## Pré-requisitos

- `curl`/`tar` instalados (padrão em qualquer distro Linux).
- `cargo build --workspace` bem-sucedido.
- Acesso de rede de saída para `github.com`/`api.github.com` (só para o Cenário 3, validação manual
  contra o GitHub real — os testes automatizados são herméticos, D8).

## Cenário 1 — Um plugin de terceiro instalado manualmente é descoberto (User Story 1, P1)

```bash
mkdir -p ~/.local/share/farol/plugins/exemplo
cat > ~/.local/share/farol/plugins/exemplo/farol-plugin.toml << 'EOF'
plugin_name = "exemplo"
command = "python3"
args = ["main.py"]

[capabilities]
exec = false
network = false
EOF
cat > ~/.local/share/farol/plugins/exemplo/main.py << 'EOF'
import json, sys
line = sys.stdin.readline()
req = json.loads(line)
result = {"protocol_version": "0.4", "plugin_name": "exemplo",
          "capabilities": {"capabilities": []}, "required_config": [],
          "widgets": [], "actions": []}
print(json.dumps({"jsonrpc": "2.0", "id": req["id"], "result": result}))
sys.stdout.flush()
EOF
cargo run -p farol-core
```

**Esperado** (SC-001): um quinto plugin, "exemplo", aparece na janela do Farol junto dos 4 de
referência, e chega a `Ready` (sem widget nenhum, já que declarou zero — comportamento válido).

## Cenário 2 — Manifesto malformado não derruba o Farol (User Story 1, Edge Case)

```bash
mkdir -p ~/.local/share/farol/plugins/quebrado
echo 'isto não é toml válido {{{' > ~/.local/share/farol/plugins/quebrado/farol-plugin.toml
cargo run -p farol-core
```

**Esperado** (SC-004, FR-004): o Farol abre normalmente, os 4 plugins de referência (mais
"exemplo", se ainda presente do Cenário 1) funcionam normalmente; um aviso aparece no terminal
citando o diretório `quebrado` e o erro de parse — nenhum crash, nenhuma tela de erro bloqueando o
resto.

## Cenário 3 — Instalar um plugin real via `farol install` (User Story 2, P2, validação manual contra o GitHub real)

Pré-requisito: um repositório GitHub público de teste, com ao menos uma release, contendo um
`farol-plugin.toml` válido na raiz (criar um repositório de fixture pessoal para este teste, ou
publicar um dos 4 plugins de referência como repositório próprio só para validar o mecanismo — não
documentado aqui por não ser parte do código deste repositório).

```bash
cargo build -p farol-core --bin farol
target/debug/farol install <seu-usuario>/<repo-de-teste>
```

**Esperado** (SC-003): mensagem de sucesso citando o `plugin_name` instalado e o caminho em
`~/.local/share/farol/plugins/<nome>/`; código de saída `0`; rodar o Cenário 1 (sem o setup manual,
já que agora o plugin já está instalado de verdade) confirma que ele é descoberto na próxima
abertura do Farol.

## Cenário 4 — Falhas de instalação traduzidas (User Story 2, Edge Cases)

```bash
target/debug/farol install <usuario>/<repo-sem-releases>
# Esperado: "nenhuma release encontrada", código de saída 1

target/debug/farol install <usuario>/<repo-inexistente>
# Esperado: mensagem clara de repositório não encontrado/inacessível, código de saída 1
```

## Cenário 5 — Colisão de nome com plugin de referência (Edge Case)

```bash
mkdir -p ~/.local/share/farol/plugins/git-local-falso
cat > ~/.local/share/farol/plugins/git-local-falso/farol-plugin.toml << 'EOF'
plugin_name = "git-local"
command = "python3"
args = ["main.py"]
EOF
cargo run -p farol-core
```

**Esperado**: o `git-local` de referência continua sendo o único ativo com esse nome; um aviso no
terminal cita a colisão; nenhum comportamento observável muda em relação a rodar sem esse diretório
presente.

## Cenário 6 — Template de plugin completa handshake (User Story 3, P3)

```bash
cp -r templates/plugin-template /tmp/meu-plugin-novo
# editar /tmp/meu-plugin-novo/farol-plugin.toml e main.py com o nome real do novo plugin
mkdir -p ~/.local/share/farol/plugins/meu-plugin-novo
cp -r /tmp/meu-plugin-novo/* ~/.local/share/farol/plugins/meu-plugin-novo/
cargo run -p farol-core
```

**Esperado** (Acceptance Scenario 1 de US3): o plugin copiado do template chega a `Ready` — prova
que o template sozinho já satisfaz o contrato mínimo do protocolo.

## Automação equivalente

Como nas features anteriores, os Cenários acima ganharam equivalentes automatizados. Nomes reais dos
testes escritos durante a implementação (`crates/farol-core/src/`):

- **Cenário 1** (plugin de terceiro descoberto, SC-001) — `plugin_worker::tests::
  discover_returns_one_config_for_one_valid_plugin` (unidade, `discover_installed_plugins()` sobre
  um diretório temporário, sem rede) e `e2e_tests::discovered_plugin_reaches_ready_through_the_real_
  state_machine` (o plugin descoberto chega a `Ready` pela mesma máquina de estados real,
  `iced_test::Emulator`).
- **Cenário 2** (manifesto malformado não derruba o Farol, SC-004/FR-004) —
  `plugin_manifest::tests::invalid_toml_is_invalid_toml_error` (`parse_manifest` sozinho) e
  `plugin_worker::tests::discover_skips_a_malformed_manifest_without_blocking_the_valid_one`
  (confirma que um manifesto quebrado não impede a descoberta dos demais).
- **Cenário 3** (`farol install` contra release real, SC-003) — automatizado hermeticamente contra um
  servidor HTTP local sintético (D8, `install.rs`) em `install::tests::
  install_succeeds_and_publishes_the_plugin` (fluxo completo: release → download → extração →
  publicação atômica) e `install::tests::reinstalling_over_a_previous_install_replaces_it_cleanly`
  (FR-008). O cenário contra o GitHub real permanece validável apenas manualmente, mesma ressalva já
  registrada nos quickstarts das features 004/006 para os casos que dependem de rede/serviço externo
  genuíno.
- **Cenário 4** (falhas de instalação traduzidas, Edge Cases/FR-010) — `install::tests::
  install_without_release_returns_no_release`, `install::tests::
  install_with_failing_tarball_download_returns_download_failed`, `install::tests::
  install_with_missing_manifest_returns_manifest_invalid` e `install::tests::
  install_with_malformed_manifest_returns_manifest_invalid` (uma falha distinta por causa, cada uma
  contra o mesmo servidor de fixture).
- **Cenário 5** (colisão de nome com plugin de referência) — `install::tests::
  install_with_name_colliding_with_a_reference_plugin_returns_name_collision` (checado também na
  instalação, D7) e, do lado da descoberta, `plugin_worker::tests::
  all_plugins_filters_a_discovered_plugin_colliding_with_a_reference_plugin` +
  `plugin_worker::tests::all_plugins_keeps_only_the_first_of_two_colliding_discovered_plugins` (D6,
  cobrindo também a colisão entre dois plugins descobertos entre si) e
  `plugin_worker::tests::discover_does_not_filter_colliding_names_between_two_discovered_plugins`
  (confirma que a filtragem de colisão entre descobertos acontece em `all_plugins()`, não em
  `discover_installed_plugins()`).
- **Cenário 6** (template completa handshake, US3) — `e2e_tests::
  emulator_takes_plugin_template_through_a_real_handshake_to_ready` (spawna
  `templates/plugin-template/main.py` diretamente e confirma `Ready` pela máquina de estados real).

Suíte completa (`plugin_manifest::tests`, 10 testes; `plugin_worker::tests`, 6 testes;
`install::tests`, 7 testes; mais os 2 cenários `e2e_tests.rs` acima — 25 no total) roda com
`cargo test --package farol-core`, incluída em `cargo test --workspace` (ver `AGENTS.md` § Testes).
