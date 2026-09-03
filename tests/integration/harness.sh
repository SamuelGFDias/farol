#!/usr/bin/env bash
#
# harness.sh — Camada 2 do harness de execução real do Farol (feature 003, T008)
# ==============================================================================
#
# `research.md` D5 / `contracts/e2e-harness-contract.md` § "Camada 2": smoke do
# **processo real**. Complementa — não substitui — a Camada 1
# (`crates/farol-core/src/e2e_tests.rs`, T004/T006), que roda o mesmo `Program`
# in-process sob o `iced_test::Emulator`. O que só esta camada cobre é
# exatamente o que a Camada 1 recorta fora: o binário `farol` de verdade, o
# `fn main()` de verdade, o backend de janela (winit) de verdade — o caminho
# que, na feature 002, foi o único a revelar os dois panics de
# `Subscription::map` (`AGENTS.md` § Armadilha).
#
# O que este script afirma
# ------------------------
#   1. o binário sobe e sobrevive à janela de observação (não morre sozinho);
#   2. `uptime-kuma` alcança `PluginState::Ready`;
#   3. `git-local` alcança `PluginState::Ready` (migrado para o protocolo
#      "0.2" — débito técnico #4, resolvido; até então ficava preso em
#      `Unavailable{VersionIncompatible}` por falar "0.1");
#   4. `openfortivpn-vpn` alcança `PluginState::Ready` (feature 004,
#      `research.md` D6 — `required_config: []`, então `Ready` depende só do
#      handshake completar, não de nenhum `config.toml`/`secrets.toml`
#      provisionado nem de `openfortivpn-gui` estar instalado na máquina);
#   5. o processo encerra ao receber `SIGTERM`;
#   6. nenhum processo remanescente (core, Xvfb ou plugin) fica para trás.
#
# Como (2) e (3) são observados sem display gráfico — e sem instrumentar
# o código de produção
# ---------------------------------------------------------------------------
# O estado dos plugins vive dentro do processo do core, e a janela é inútil
# como sonda: sob Xvfb, sem GPU/WM, ela renderiza preta (`AGENTS.md` § Testes).
#
# **Alternativa avaliada e recusada**: um log de diagnóstico permanente no
# core, ativado por variável de ambiente (ex.: `FAROL_LOG=state` imprimindo
# cada transição de `PluginState` em stderr) — a evolução "permanente e
# condicionada" do `eprintln!` temporário que já foi usado à mão nesta base.
# Recusada por dois motivos:
#
#   - **Custo de produção por benefício de teste**: exigiria mudar
#     `update.rs`/`main.rs` e manter para sempre um caminho de código cujo
#     único consumidor é este script.
#   - **Sonda mais fraca**: um log é uma *afirmação do código sobre si mesmo*.
#     Se a máquina de estados estiver errada, o log fica errado junto — a
#     mesma classe de diagnóstico enganoso que esta feature existe para
#     eliminar.
#
# **Escolha**: observar o **protocolo na linha** (JSON-RPC/NDJSON), que é
# comportamento, não auto-relato. Um shim de `python3` (gerado neste script,
# dentro do diretório temporário, à frente no `$PATH`) relaia stdin/stdout
# entre o core e o `main.py` real de cada plugin e, de passagem, grava a
# transcrição de cada direção. A inferência é sólida porque o core só emite
# `widget/get` a partir de `update.rs::handle_refresh_tick`, que retorna cedo
# se `state != PluginState::Ready`:
#
#     `widget/get` na transcrição core→plugin  ⟺  aquele plugin chegou a Ready
#
# Nenhuma linha de código de produção sabe que está sendo observada, e o
# repositório não ganha nenhum arquivo novo: o shim existe só no `mktemp -d`
# desta execução.
#
# Encerramento e órfãos
# ---------------------
# `kill_on_drop` (`plugin_worker.rs`) mata os processos de plugin quando o
# `Child` do Rust é dropado num unwind normal — o que **não acontece** quando o
# próprio harness manda `SIGTERM`/`SIGKILL` no core: o core morre sem
# desenrolar a pilha e os `python3` filhos sobrariam. Por isso este script roda
# o binário em seu **próprio grupo de processos** (`set -m` dá ao job em
# background um pgid próprio) e sinaliza o **grupo inteiro** (`kill -- -$PGID`),
# que alcança Xvfb, o core e todo plugin de uma vez. A limpeza é reafirmada
# depois (`pgrep -g`), não presumida.
#
# Uso
# ---
#     ./tests/integration/harness.sh [--skip-build] [--window SEGUNDOS]
#
# Pré-requisitos: `cargo`, `python3`, `git`, `xvfb-run` (pacote `xvfb`).
#
# Códigos de saída
#   0  todas as condições confirmadas
#   1  alguma condição falhou (a linha `[FALHA]` diz qual)
#   2  pré-requisito de ambiente ausente

set -Eeuo pipefail

# --- Orçamento de tempo (T007, `## Clarifications` de `spec.md`) -------------
# 30s por verificação individual, 120s pelo cenário inteiro. A compilação fica
# fora do cronômetro do cenário: é pré-requisito documentado no contrato
# (`e2e-harness-contract.md` Camada 2), não parte do comportamento observado.
STATE_TIMEOUT_SECONDS=30
SCENARIO_TIMEOUT_SECONDS=120
# Janela mínima de observação: o binário precisa continuar de pé por este
# tempo *depois* de os plugins alcançarem seus estados, para "subiu e morreu
# logo em seguida" não passar como sucesso.
OBSERVATION_WINDOW_SECONDS=5

SKIP_BUILD=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --skip-build) SKIP_BUILD=1; shift ;;
        --window) OBSERVATION_WINDOW_SECONDS="$2"; shift 2 ;;
        -h|--help) sed -n '2,60p' "$0"; exit 0 ;;
        *) echo "[FALHA] argumento desconhecido: $1" >&2; exit 2 ;;
    esac
done

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$REPO_ROOT"

log()  { printf '[harness] %s\n' "$*"; }
fail() { printf '[FALHA] %s\n' "$*" >&2; FAILED=1; }

FAILED=0
FAROL_PID=""
WORK=""

# --- Pré-requisitos ---------------------------------------------------------
for tool in cargo python3 git xvfb-run pgrep; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "[FALHA] pré-requisito ausente: $tool" >&2
        exit 2
    fi
done
REAL_PYTHON="$(command -v python3)"

# --- Limpeza (sempre, inclusive em erro/interrupção) ------------------------
kill_process_group() {
    local pgid="$1" signal="$2"
    kill "-${signal}" -- "-${pgid}" 2>/dev/null || true
}

cleanup() {
    if [[ -n "$FAROL_PID" ]] && kill -0 "$FAROL_PID" 2>/dev/null; then
        kill_process_group "$FAROL_PID" TERM
        sleep 1
        kill_process_group "$FAROL_PID" KILL
    fi
    [[ -n "$WORK" && -d "$WORK" ]] && rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

# --- Compilação do binário real ---------------------------------------------
if [[ "$SKIP_BUILD" -eq 0 ]]; then
    log "compilando o binário farol (cargo build --bin farol)"
    if ! cargo build --bin farol; then
        fail "cargo build --bin farol falhou — nada a observar"
        exit 1
    fi
fi
FAROL_BIN="$REPO_ROOT/target/debug/farol"
if [[ ! -x "$FAROL_BIN" ]]; then
    fail "binário não encontrado em $FAROL_BIN (rode sem --skip-build)"
    exit 1
fi

SCENARIO_STARTED_AT=$SECONDS
check_scenario_budget() {
    local elapsed=$((SECONDS - SCENARIO_STARTED_AT))
    if (( elapsed >= SCENARIO_TIMEOUT_SECONDS )); then
        fail "cenário estourou o orçamento de ${SCENARIO_TIMEOUT_SECONDS}s em '$1' (decorrido: ${elapsed}s)"
        return 1
    fi
    return 0
}

# --- Fixture determinística (mesmo padrão de T003) --------------------------
WORK="$(mktemp -d -t farol-harness-XXXXXXXX)"
XDG_DIR="$WORK/xdg"
FAROL_CONFIG="$XDG_DIR/farol"
RPC_DIR="$WORK/rpc"
SCAN_ROOT="$WORK/repos"
mkdir -p "$FAROL_CONFIG/plugins/git-local" "$FAROL_CONFIG/plugins/uptime-kuma" \
         "$RPC_DIR" "$SCAN_ROOT/exemplo" "$WORK/bin"

log "fixture hermética em $WORK (XDG_CONFIG_HOME isolado; ~/.config/farol intocado)"

# Repositório git real, com identidade/datas fixas e sem herdar ~/.gitconfig.
(
    cd "$SCAN_ROOT/exemplo"
    export GIT_CONFIG_GLOBAL=/dev/null GIT_CONFIG_SYSTEM=/dev/null
    export GIT_AUTHOR_DATE="2026-01-01T00:00:00+00:00"
    export GIT_COMMITTER_DATE="2026-01-01T00:00:00+00:00"
    git init --quiet --initial-branch=main
    printf 'fixture determinística do harness\n' > README.md
    git add README.md
    git -c user.name='Farol Harness' -c user.email='harness@farol.invalid' \
        commit --quiet --no-gpg-sign --message='commit inicial da fixture'
) || { fail "não foi possível criar o repositório git da fixture"; exit 1; }

printf 'scan_root = "%s"\n' "$SCAN_ROOT" > "$FAROL_CONFIG/plugins/git-local/config.toml"
# Porta 1 de 127.0.0.1 nunca tem serviço escutando: o poller do plugin recebe
# "connection refused" na hora, sem latência e sem nenhum tráfego para fora da
# máquina. `Ready` depende só do handshake + `required_config` resolvido — os
# *dados* do widget são asserção da Camada 1 (T006), não desta.
printf 'base_url = "http://127.0.0.1:1"\n' > "$FAROL_CONFIG/plugins/uptime-kuma/config.toml"
# API key sintética, jamais usada contra instância real (`spec.md`
# § Clarifications: fixtures inteiramente sintéticas).
printf '[uptime-kuma]\napi_key = "farol-e2e-fixture-key"\n' > "$FAROL_CONFIG/secrets.toml"
chmod 600 "$FAROL_CONFIG/secrets.toml"

# --- Shim de `python3` que grava a transcrição JSON-RPC de cada plugin -------
cat > "$WORK/bin/python3" <<PYTHON_SHIM
#!$REAL_PYTHON
"""Relé transparente entre o core e o \`main.py\` real de um plugin.

Gerado por tests/integration/harness.sh. Repassa stdin/stdout byte a byte (com
flush por linha, para não introduzir buffer onde o protocolo NDJSON não
tolera) e grava cada direção em \$RPC_DIR/<plugin>.<direção>.ndjson.
"""
import os
import subprocess
import sys
import threading

REAL_PYTHON = "$REAL_PYTHON"
RPC_DIR = "$RPC_DIR"

args = sys.argv[1:]
plugin = "desconhecido"
for arg in args:
    if arg.endswith("main.py"):
        plugin = os.path.basename(os.path.dirname(arg))
        break

child = subprocess.Popen(
    [REAL_PYTHON] + args, stdin=subprocess.PIPE, stdout=subprocess.PIPE
)


def pump(source, sink, transcript):
    with open(transcript, "ab", buffering=0) as log:
        while True:
            line = source.readline()
            if not line:
                break
            log.write(line)
            sink.write(line)
            sink.flush()
    try:
        sink.close()
    except OSError:
        pass


threading.Thread(
    target=pump,
    args=(sys.stdin.buffer, child.stdin,
          os.path.join(RPC_DIR, plugin + ".core-to-plugin.ndjson")),
    daemon=True,
).start()
threading.Thread(
    target=pump,
    args=(child.stdout, sys.stdout.buffer,
          os.path.join(RPC_DIR, plugin + ".plugin-to-core.ndjson")),
    daemon=True,
).start()

sys.exit(child.wait())
PYTHON_SHIM
chmod +x "$WORK/bin/python3"

# --- Sobe o binário real, em seu próprio grupo de processos ------------------
FAROL_LOG="$WORK/farol.log"
log "subindo target/debug/farol sob xvfb-run (cwd = raiz do repo, exigido por known_plugins())"

set -m   # job control: o job em background vira líder de seu próprio grupo
env XDG_CONFIG_HOME="$XDG_DIR" PATH="$WORK/bin:$PATH" \
    xvfb-run -a "$FAROL_BIN" > "$FAROL_LOG" 2>&1 &
FAROL_PID=$!
set +m

transcript_has() {  # <arquivo> <padrão fixo>
    [[ -f "$1" ]] && grep -qF -- "$2" "$1"
}

UPTIME_KUMA_TO_PLUGIN="$RPC_DIR/uptime-kuma.core-to-plugin.ndjson"
GIT_LOCAL_TO_PLUGIN="$RPC_DIR/git-local.core-to-plugin.ndjson"
OPENFORTIVPN_VPN_TO_PLUGIN="$RPC_DIR/openfortivpn-vpn.core-to-plugin.ndjson"

# --- Verificação 1+2+3+4: sobe, e cada plugin chega a Ready ------------------
log "aguardando (até ${STATE_TIMEOUT_SECONDS}s) os plugins alcançarem Ready"
DEADLINE=$((SECONDS + STATE_TIMEOUT_SECONDS))
UPTIME_KUMA_READY=0
GIT_LOCAL_READY=0
OPENFORTIVPN_VPN_READY=0

while (( SECONDS < DEADLINE )); do
    check_scenario_budget "espera pelos estados dos plugins" || break

    if ! kill -0 "$FAROL_PID" 2>/dev/null; then
        fail "farol saiu sozinho antes da janela de observação terminar"
        break
    fi

    # `widget/get` só é emitido por handle_refresh_tick, que retorna cedo se o
    # estado não for Ready — logo, vê-lo na linha prova a transição. Desde a
    # migração do `git-local` para o protocolo "0.2" (débito técnico #4,
    # resolvido), os dois plugins alcançam Ready pelo mesmo mecanismo.
    if transcript_has "$UPTIME_KUMA_TO_PLUGIN" '"widget/get"'; then
        UPTIME_KUMA_READY=1
    fi
    if transcript_has "$GIT_LOCAL_TO_PLUGIN" '"widget/get"'; then
        GIT_LOCAL_READY=1
    fi
    if transcript_has "$OPENFORTIVPN_VPN_TO_PLUGIN" '"widget/get"'; then
        OPENFORTIVPN_VPN_READY=1
    fi

    if (( UPTIME_KUMA_READY == 1 && GIT_LOCAL_READY == 1 && OPENFORTIVPN_VPN_READY == 1 )); then
        break
    fi
    sleep 0.2
done

if (( FAILED == 0 )); then
    if (( UPTIME_KUMA_READY == 0 )); then
        fail "uptime-kuma não alcançou Ready em ${STATE_TIMEOUT_SECONDS}s (nenhum widget/get na transcrição core→plugin)"
    else
        log "OK — uptime-kuma alcançou Ready (widget/get observado na linha)"
    fi

    if (( GIT_LOCAL_READY == 0 )); then
        fail "git-local não alcançou Ready em ${STATE_TIMEOUT_SECONDS}s (nenhum widget/get na transcrição core→plugin)"
    else
        log "OK — git-local alcançou Ready (widget/get observado na linha)"
    fi

    if (( OPENFORTIVPN_VPN_READY == 0 )); then
        fail "openfortivpn-vpn não alcançou Ready em ${STATE_TIMEOUT_SECONDS}s (nenhum widget/get na transcrição core→plugin)"
    else
        log "OK — openfortivpn-vpn alcançou Ready (widget/get observado na linha)"
    fi
fi

# --- Verificação 1 (continuação): sobrevive à janela de observação -----------
if (( FAILED == 0 )); then
    log "observando por mais ${OBSERVATION_WINDOW_SECONDS}s que o processo continua de pé"
    sleep "$OBSERVATION_WINDOW_SECONDS"
    check_scenario_budget "janela de observação" || true
    if ! kill -0 "$FAROL_PID" 2>/dev/null; then
        fail "farol morreu durante a janela de observação de ${OBSERVATION_WINDOW_SECONDS}s"
    else
        log "OK — processo vivo ao fim da janela de observação"
    fi
fi

# --- Verificação 5: encerra ao receber SIGTERM ------------------------------
# O sinal vai para o **grupo**: é isso que garante que Xvfb e os processos de
# plugin morram junto, já que `kill_on_drop` não roda quando o core é abatido
# por sinal (ver cabeçalho).
log "enviando SIGTERM ao grupo de processos $FAROL_PID"
kill_process_group "$FAROL_PID" TERM

EXIT_STATUS=0
wait "$FAROL_PID" 2>/dev/null || EXIT_STATUS=$?
# 0 = saída limpa; 143 = 128 + SIGTERM (encerrado pelo sinal, também aceito
# pelo contrato: `e2e-harness-contract.md` Camada 2).
if (( EXIT_STATUS == 0 || EXIT_STATUS == 143 )); then
    log "OK — encerrou com status $EXIT_STATUS após SIGTERM"
else
    fail "encerramento sujo após SIGTERM: status $EXIT_STATUS"
fi

# --- Verificação 6: nenhum processo remanescente ----------------------------
LEFTOVER_DEADLINE=$((SECONDS + 10))
while (( SECONDS < LEFTOVER_DEADLINE )); do
    LEFTOVERS="$(pgrep -g "$FAROL_PID" 2>/dev/null || true)"
    [[ -z "$LEFTOVERS" ]] && break
    sleep 0.2
done

LEFTOVERS="$(pgrep -g "$FAROL_PID" 2>/dev/null || true)"
if [[ -n "$LEFTOVERS" ]]; then
    # shellcheck disable=SC2086
    fail "processos remanescentes no grupo $FAROL_PID: $(ps -o pid=,args= -p $(echo "$LEFTOVERS" | tr '\n' ',' | sed 's/,$//') 2>/dev/null | tr '\n' ';')"
    kill_process_group "$FAROL_PID" KILL
else
    log "OK — nenhum processo remanescente no grupo (core, Xvfb e plugins encerrados)"
fi

# Rede de segurança independente do grupo: qualquer processo que ainda
# referencie o diretório temporário desta execução (ex.: um plugin que tivesse
# escapado para outro grupo).
STRAYS="$(pgrep -f "$WORK" 2>/dev/null || true)"
if [[ -n "$STRAYS" ]]; then
    fail "processos remanescentes referenciando $WORK: $(echo "$STRAYS" | tr '\n' ' ')"
    # shellcheck disable=SC2086
    kill -KILL $STRAYS 2>/dev/null || true
fi

# --- Relatório --------------------------------------------------------------
ELAPSED=$((SECONDS - SCENARIO_STARTED_AT))
if (( FAILED != 0 )); then
    echo "---- últimas linhas de stdout/stderr do farol ----" >&2
    tail -n 40 "$FAROL_LOG" >&2 || true
    echo "-------------------------------------------------" >&2
    printf '[harness] FALHOU em %ss (teto do cenário: %ss)\n' "$ELAPSED" "$SCENARIO_TIMEOUT_SECONDS" >&2
    exit 1
fi

log "SUCESSO — 6/6 condições confirmadas em ${ELAPSED}s (teto do cenário: ${SCENARIO_TIMEOUT_SECONDS}s)"
