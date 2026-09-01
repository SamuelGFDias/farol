#!/usr/bin/env python3
"""Plugin de referência `uptime-kuma` — o lado "servidor" do protocolo Farol v0.2.

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
"""

from __future__ import annotations

import json
import sys

JSONRPC_VERSION = "2.0"
PROTOCOL_VERSION = "0.2"
PLUGIN_NAME = "uptime-kuma"

METHOD_HANDSHAKE_HELLO = "handshake/hello"
METHOD_WIDGET_GET = "widget/get"


def _success(request_id, result: dict) -> dict:
    """Envelope JSON-RPC de sucesso (`jsonrpc`/`id`/`result`)."""
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "result": result}


def _error(request_id, code: int, message: str, data: dict | None = None) -> dict:
    """Envelope JSON-RPC de erro (`jsonrpc`/`id`/`error`), forma de `error.schema.json`."""
    error_obj: dict = {"code": code, "message": message}
    if data is not None:
        error_obj["data"] = data
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "error": error_obj}


def handle_handshake_hello(request: dict) -> dict:
    """`handshake/hello` — placeholder de transporte (T020).

    TODO (User Story 1): declarar `protocol_version = "0.2"`, `required_config` (base_url,
    api_key) e `capabilities` com `network`, per docstring do módulo e `contracts/
    uptime-kuma-plugin.md`. Por ora apenas devolve um erro JSON-RPC sinalizando que a lógica de
    negócio ainda não foi implementada — o loop de transporte em si (foco desta task) já responde
    corretamente à requisição.
    """
    request_id = request.get("id")
    return _error(request_id, -32603, "handshake/hello ainda não implementado (TODO US1)")


def handle_widget_get(request: dict) -> dict:
    """`widget/get` — placeholder de transporte (T020).

    TODO (User Story 1): devolver `MonitorStatusItem[]` a partir do cache preenchido pelo poller
    em background, per docstring do módulo e `contracts/widget-protocol.md`.
    """
    request_id = request.get("id")
    return _error(request_id, -32603, "widget/get ainda não implementado (TODO US1)")


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
