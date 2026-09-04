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

## Automação equivalente (a preencher durante `/speckit-implement`)

Como nas features anteriores, os Cenários acima ganham equivalentes automatizados: testes de
unidade de `parse_manifest`/`discover_installed_plugins` (fixtures em diretório temporário, sem
dependência de rede), testes de integração do fluxo de instalação contra um servidor HTTP local
sintético (D8 de `research.md`, mesmo padrão de `MetricsFixtureServer` da feature 002) cobrindo
sucesso e cada falha de Edge Case, e um cenário `e2e_tests.rs`/`Emulator` confirmando que um plugin
descoberto chega a `Ready` pela máquina de estados real — só o Cenário 3 (rede real contra o GitHub)
permanece validável apenas manualmente, mesma ressalva já registrada nos quickstarts das features
004/006 para os casos que dependem de rede/serviço externo genuíno.
