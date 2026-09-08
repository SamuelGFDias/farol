#!/usr/bin/env python3
"""Plugin de referência `uptime-kuma` — o lado "servidor" do protocolo Farol v0.4.

Prova do suporte do protocolo a plugins de rede e configuração de usuário (D1/D8 de `research.md`
da feature 002): este arquivo não importa nem depende do crate Rust `farol-protocol` em nenhum
momento — é uma implementação independente lendo apenas `protocol/SPEC.md` + `protocol/schema/v0.2/
*.schema.json` + os contratos em `specs/002-uptime-kuma-plugin/contracts/`.

Transporte: JSON-RPC 2.0 sobre NDJSON em stdin/stdout (mesmo padrão da feature 001,
`protocol/SPEC.md` §2-§4). O core é sempre quem inicia cada requisição; este processo nunca escreve
nada em stdout antes de receber e responder `handshake/hello`. `stderr` é livre para logging humano
(§3) — usado aqui só para diagnóstico, nunca faz parte do protocolo em si.

Handlers declarados:
- `handshake/hello`: declara `protocol_version = "0.2"`, `required_config` com campos base_url e
  api_key, e `capabilities` com apenas `network` (nenhum `exec` — este plugin não invoca binário
  externo). Sem ações (`actions` lista vazia).
- `widget/get`: devolve lista de monitores (MonitorStatusItem[]) vindo de um poller em background,
  servido a partir de cache lock-guarded para garantir que o RPC_TIMEOUT_CONTROL nunca bloqueia em
  latência de rede.

Apenas biblioteca padrão (D7 de `research.md`) — `json`, `sys`, `threading`.
Migrado para `"0.3"` como parte da feature 004 (`specs/004-vpn-status-plugin/research.md` D2)
— mudança mecânica, nenhum campo novo usado por este plugin.
Migrado novamente para `"0.4"` como parte da feature 005 (`specs/005-docker-containers-
plugin/research.md` D2) — mudança mecânica, nenhum campo novo usado por este plugin.
"""

from __future__ import annotations

import json
import sys
from secrets import load_api_key
from urllib.parse import urlsplit

from config import load_base_url
from poller import DEFAULT_POLL_INTERVAL_MS, MetricsCache, PollerThread

JSONRPC_VERSION = "2.0"
PROTOCOL_VERSION = "0.4"
PLUGIN_NAME = "uptime-kuma"
WIDGET_ID = "uptime-kuma-monitors"

METHOD_HANDSHAKE_HELLO = "handshake/hello"
METHOD_WIDGET_GET = "widget/get"

DEFAULT_PORT_BY_SCHEME = {"https": 443, "http": 80}

# Leitura de configuração/segredo uma única vez no arranque do processo (D8/D9 de `research.md` —
# sem hot-reload dentro de um processo já rodando; uma correção passa por reiniciar o processo via
# a tela de setup do core).
_BASE_URL = load_base_url()
_API_KEY = load_api_key()
_METRICS_CACHE = MetricsCache()

if _BASE_URL and _API_KEY:
    _POLLER = PollerThread(_BASE_URL, _API_KEY, _METRICS_CACHE, DEFAULT_POLL_INTERVAL_MS)
    _POLLER.start()
else:
    _POLLER = None


def _success(request_id, result: dict) -> dict:
    """Envelope JSON-RPC de sucesso (`jsonrpc`/`id`/`result`)."""
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "result": result}


def _error(request_id, code: int, message: str, data: dict | None = None) -> dict:
    """Envelope JSON-RPC de erro (`jsonrpc`/`id`/`error`), forma de `error.schema.json`."""
    error_obj: dict = {"code": code, "message": message}
    if data is not None:
        error_obj["data"] = data
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "error": error_obj}


def _derive_network_capability(base_url: str) -> dict | None:
    """Deriva `{"kind": "network", "host": ..., "port": ...}` de `base_url`.

    `host` obrigatório; `port` default por esquema (443 https / 80 http) quando não explícito na
    URL (`handshake-delta.md` § `capabilities`). Devolve `None` se `base_url` não tiver host
    reconhecível (URL malformada) — tratado como "nada concreto para declarar honestamente" (D1).
    """
    parsed = urlsplit(base_url)
    if not parsed.hostname:
        return None
    port = parsed.port or DEFAULT_PORT_BY_SCHEME.get(parsed.scheme)
    capability: dict = {"kind": "network", "host": parsed.hostname}
    if port is not None:
        capability["port"] = port
    return capability


def handle_handshake_hello(request: dict) -> dict:
    """`handshake/hello` — declara identidade, `required_config` e `capabilities` (T024).

    `required_config` é sempre declarado com os dois itens (`base_url`, `api_key`), independente de
    já haver valor injetado — é essa declaração fixa que permite ao core montar a tela de setup
    (D8 de `research.md`). `capabilities` só inclui `network` quando `base_url` foi resolvido via
    variável de ambiente (`handshake-delta.md`). `actions` é sempre `[]` (FR-004) e `widgets`
    declara sempre o único widget deste plugin, com `suggested_refresh_interval_ms` — o mesmo valor
    usado internamente como cadência da thread de polling (D6).
    """
    request_id = request.get("id")

    capabilities: list[dict] = []
    if _BASE_URL:
        network_capability = _derive_network_capability(_BASE_URL)
        if network_capability is not None:
            capabilities.append(network_capability)

    result = {
        "protocol_version": PROTOCOL_VERSION,
        "plugin_name": PLUGIN_NAME,
        "capabilities": {"capabilities": capabilities},
        "required_config": [
            {
                "name": "base_url",
                "secret": False,
                "description": "URL base da instância Uptime Kuma",
            },
            {
                "name": "api_key",
                "secret": True,
                "description": "API Key de métricas do Uptime Kuma",
            },
        ],
        "widgets": [
            {
                "id": WIDGET_ID,
                "kind": "monitor-status-grid",
                "title": "Uptime Kuma",
                "suggested_refresh_interval_ms": DEFAULT_POLL_INTERVAL_MS,
            }
        ],
        "actions": [],
    }
    return _success(request_id, result)


def handle_widget_get(request: dict) -> dict:
    """`widget/get` — devolve `MonitorStatusItem[]` a partir do cache do poller (T028).

    Lê exclusivamente o cache sob o mesmo lock usado pela thread de polling — nunca I/O de rede
    síncrono (FR-010). Lógica de decisão per `data-model.md` §2.3:

    - `not_configured` (-32005, salvaguarda — o caminho primário é o core nem chegar a chamar
      `widget/get`, D8/D9) se `base_url`/`api_key` ausentes das variáveis de ambiente;
    - senão erro (-32006 `metrics_unreachable` / -32007 `metrics_parse_error`) se ainda não houve
      nenhuma leitura bem-sucedida, ou se a última tentativa (bem-sucedida ou não) foi um erro mais
      recente que o último sucesso;
    - senão sucesso com `items` vindos do último sucesso conhecido.
    """
    request_id = request.get("id")

    if not (_BASE_URL and _API_KEY):
        return _error(
            request_id,
            -32005,
            "plugin uptime-kuma não configurado",
            {"reason": "not_configured"},
        )

    last_success, last_error = _METRICS_CACHE.snapshot()

    if last_success is None or (last_error is not None and last_error["at"] >= last_success["at"]):
        reason = last_error["reason"] if last_error is not None else "metrics_unreachable"
        detail = last_error["detail"] if last_error is not None else "aguardando primeira leitura"
        code = -32006 if reason == "metrics_unreachable" else -32007
        return _error(
            request_id,
            code,
            "falha ao consultar /metrics da instância Uptime Kuma configurada",
            {"reason": reason, "detail": detail},
        )

    return _success(request_id, {"widget_id": WIDGET_ID, "items": last_success["monitors"]})


DISPATCH = {
    METHOD_HANDSHAKE_HELLO: handle_handshake_hello,
    METHOD_WIDGET_GET: handle_widget_get,
}


def dispatch(request: dict) -> dict | None:
    """Despacha uma requisição já decodificada para o handler do método correspondente.

    Nunca deixa uma exceção não tratada de um handler derrubar o loop principal (mesma garantia de
    `plugins/git-local/main.py`) — qualquer exceção inesperada vira um erro `-32603 internal error`
    pontual, logado em stderr para diagnóstico.

    Devolve `None` apenas quando a requisição não tem `id` — esta versão do protocolo não usa
    notificações (`protocol/SPEC.md` §2), então isso não é exercitado pelo core de referência, mas
    o plugin não pode responder de forma correlacionável sem um `id` para ecoar.
    """
    request_id = request.get("id")
    method = request.get("method")

    handler = DISPATCH.get(method)
    if handler is None:
        if request_id is None:
            return None
        return _error(request_id, -32601, f"método desconhecido: {method!r}")

    try:
        return handler(request)
    except Exception as exc:  # noqa: BLE001 — última linha de defesa do loop de I/O
        print(f"[uptime-kuma] erro inesperado ao processar {method!r}: {exc!r}", file=sys.stderr)
        if request_id is None:
            return None
        return _error(request_id, -32603, f"erro interno do plugin: {exc}")


def main() -> None:
    """Loop principal: lê NDJSON de stdin, despacha, escreve NDJSON em stdout.

    `protocol/SPEC.md` §4: cada mensagem é uma linha de JSON compacto terminada em `\\n`, UTF-8;
    uma linha vazia MUST ser ignorada silenciosamente. `sys.stdout.flush()` após cada escrita é
    crítico — sem ele o core fica esperando indefinidamente um pipe bufferizado.
    """
    for raw_line in sys.stdin:
        line = raw_line.rstrip("\n")
        if not line.strip():
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as exc:
            # Não exercitado nesta versão do protocolo (protocol/SPEC.md §2.1) — sem um `id`
            # parseável não há como responder de forma correlacionável; loga e segue.
            print(f"[uptime-kuma] linha não é JSON válido, ignorada: {exc!r}", file=sys.stderr)
            continue

        if not isinstance(request, dict):
            print(
                f"[uptime-kuma] mensagem não é um objeto JSON-RPC, ignorada: {request!r}",
                file=sys.stderr,
            )
            continue

        response = dispatch(request)
        if response is None:
            continue

        sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
