# Testes de integração

Verificação de ponta a ponta do Farol com o binário `farol` **real** e processos de plugin
**reais** conversando por stdin/stdout.

Desde a feature `003-automated-testing-infrastructure`, o harness existe em **duas camadas**
(`specs/003-automated-testing-infrastructure/contracts/e2e-harness-contract.md`). Este diretório é a
Camada 2; a Camada 1 mora dentro do crate.

| | Camada 1 — in-process | Camada 2 — smoke de processo real |
|---|---|---|
| Onde | `crates/farol-core/src/e2e_tests.rs` | `tests/integration/harness.sh` (este diretório) |
| Como roda | `cargo test --package farol-core` | `./tests/integration/harness.sh` |
| O que executa | o `Program` real (`crate::program`) sob `iced_test::Emulator` | o binário `target/debug/farol` sob `xvfb-run` |
| Precisa de display | não | sim (`xvfb`) |
| Cobre | `Subscription` → spawn → handshake → `widget/get` → dados no widget | `fn main()`, backend de janela (winit), encerramento por sinal |

As duas são complementares: a Camada 1 enxerga o modelo (`PluginState`, itens do widget) e é rápida
o bastante para rodar em toda mudança; a Camada 2 é a única que exercita o binário de verdade — o
caminho que, na feature 002, foi o único a revelar os dois panics de `Subscription::map`
(`AGENTS.md` § Armadilha).

> **Nota histórica**: até a feature 003 este README descrevia um `harness.sh` que nunca chegou a ser
> escrito (débito herdado da feature 001). O script agora existe e roda — o que está descrito abaixo
> é o comportamento real, não uma intenção.

## Camada 2 — `harness.sh`

```bash
./tests/integration/harness.sh                 # compila e roda
./tests/integration/harness.sh --skip-build    # usa o target/debug/farol já compilado
./tests/integration/harness.sh --window 10     # janela de observação maior (default: 5s)
```

Pré-requisitos: `cargo`, `python3`, `git`, `xvfb-run` (pacote `xvfb`), `pgrep`.

O script confirma **seis** condições e diz qual falhou (`[FALHA] ...`), sem exigir leitura de log
bruto:

1. o binário sobe e sobrevive à janela de observação;
2. `uptime-kuma` alcança `PluginState::Ready`;
3. `git-local` alcança `PluginState::Ready` (migrado para protocolo `"0.3"` na feature 004,
   antes era `"0.2"` — débito técnico #4 resolvido);
4. `openfortivpn-vpn` alcança `PluginState::Ready` (feature 004, fixture determinística sem
   depender de `openfortivpn-gui` instalado);
5. o processo encerra ao receber `SIGTERM` (status `0` ou `143`);
6. nenhum processo remanescente — core, Xvfb ou plugin.

Saída: `0` sucesso, `1` alguma condição falhou, `2` pré-requisito de ambiente ausente.

### Fixture

Inteiramente sintética e hermética (`spec.md` § Clarifications da feature 003): um `mktemp -d` vira
o `$XDG_CONFIG_HOME` da execução, com um repositório git real como `scan_root` do `git-local`, um
`base_url` apontando para `http://127.0.0.1:1` (porta sempre fechada) e uma API key sintética. O
`~/.config/farol` da máquina nunca é lido nem escrito, e nenhuma credencial real é usada.

### Como o estado dos plugins é observado sem display

A janela renderiza preta sob Xvfb sem GPU, então ela não serve de sonda. Em vez de instrumentar o
código de produção com um log de diagnóstico, o script observa o **protocolo na linha**: um shim de
`python3`, gerado no diretório temporário e posto à frente do `$PATH`, relaia stdin/stdout entre o
core e o `main.py` real de cada plugin e grava a transcrição NDJSON de cada direção. Como o core só
emite `widget/get` a partir de `update.rs::handle_refresh_tick`, que retorna cedo se o estado não
for `Ready`, ver um `widget/get` na transcrição **prova** a transição — sem nenhuma linha de código
de produção sabendo que está sendo observada. O raciocínio completo (e a alternativa recusada) está
no cabeçalho do próprio script.

### Encerramento e processos órfãos

`kill_on_drop` (`plugin_worker.rs`) só encerra os processos de plugin quando o `Child` do Rust é
dropado num *unwind* normal — o que **não acontece** quando o core é abatido por sinal. Por isso o
script roda o binário em seu próprio grupo de processos e sinaliza o **grupo inteiro**, alcançando
Xvfb, core e plugins de uma vez; depois reafirma a limpeza com `pgrep -g`, em vez de presumi-la.

## Camada 1 — `crates/farol-core/src/e2e_tests.rs`

```bash
cargo test --package farol-core e2e_tests
```

Roda sem display. Orçamento de tempo (`spec.md` § Clarifications): 30s por verificação individual,
120s por cenário — ambos aplicados no código, não só documentados. Cada cenário confirma também que
nenhum processo filho remanesce ao final.

> O módulo mora em `src/` (e não em `crates/farol-core/tests/`) porque `farol-core` é um crate
> só-`bin`: um teste de integração em `tests/` compila como crate separado e não conseguiria
> importar nada dele. Ver a docstring do módulo.

## Referências

- `specs/003-automated-testing-infrastructure/contracts/e2e-harness-contract.md` — contrato das duas camadas
- `specs/003-automated-testing-infrastructure/quickstart.md` § Cenário 1 — validação manual de US1
- `specs/001-walking-skeleton-git-plugin/quickstart.md` — os 7 cenários originais do walking skeleton
- `specs/001-walking-skeleton-git-plugin/data-model.md` § State Transitions — máquina de `PluginState`
