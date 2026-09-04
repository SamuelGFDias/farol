#!/usr/bin/env python3
"""Template mínimo de plugin Farol — ponto de partida para um plugin novo.

Implementa só o necessário para o Farol chegar a `PluginState::Ready`:
responde `handshake/hello` com um `HandshakeHelloResult` válido, sem declarar
nenhum widget (`widgets: []`) nem nenhuma ação (`actions: []`). Não implementa
`widget/get` nem `action/invoke` — como este plugin não anuncia widgets nem
ações no handshake, o core nunca chama esses métodos (`protocol/SPEC.md`
§5.2/§5.3). Ao adicionar seu primeiro widget, veja `plugins/git-local/main.py`
(ou qualquer um dos outros 3 plugins de referência do repositório) para o
padrão completo de `handle_widget_get`.

Apenas biblioteca padrão, seguindo o mesmo padrão dos plugins de referência do
Farol (sem dependência externa: `json`, `sys`).

Transporte: JSON-RPC 2.0 sobre NDJSON em stdin/stdout (`protocol/SPEC.md`
§2-§4). O core é sempre quem inicia cada requisição; este processo nunca
escreve nada em stdout antes de receber e responder `handshake/hello`.
`stderr` é livre para logging humano, nunca faz parte do protocolo em si.
"""

from __future__ import annotations

import json
import sys

JSONRPC_VERSION = "2.0"

# Versão do protocolo suportada por este template. Deve bater exatamente com
# `CORE_PROTOCOL_VERSION` do core (`crates/farol-core/src/plugin_worker.rs`) —
# o core recusa, por igualdade exata, qualquer versão diferente
# (`protocol/SPEC.md` §6.4).
PROTOCOL_VERSION = "0.4"

# Ajuste para o mesmo valor de `plugin_name` no `farol-plugin.toml` ao lado.
PLUGIN_NAME = "meu-plugin"

METHOD_HANDSHAKE_HELLO = "handshake/hello"


def _success(request_id, result: dict) -> dict:
    """Envelope JSON-RPC de sucesso (`jsonrpc`/`id`/`result`)."""
    return {"jsonrpc": JSONRPC_VERSION, "id": request_id, "result": result}


def _error(request_id, code: int, message: str) -> dict:
    """Envelope JSON-RPC de erro (`jsonrpc`/`id`/`error`)."""
    return {
        "jsonrpc": JSONRPC_VERSION,
        "id": request_id,
        "error": {"code": code, "message": message},
    }


def handle_handshake_hello(request: dict) -> dict:
    """`handshake/hello` — `protocol/SPEC.md` §6.

    Sem widgets nem ações: basta isto para o Farol considerar a conexão
    `Ready`. Adicione entradas em `widgets`/`actions` (e os handlers
    correspondentes) conforme seu plugin crescer.
    """
    request_id = request.get("id")
    result = {
        "protocol_version": PROTOCOL_VERSION,
        "plugin_name": PLUGIN_NAME,
        "capabilities": {"capabilities": []},
        "required_config": [],
        "widgets": [],
        "actions": [],
    }
    return _success(request_id, result)


DISPATCH = {
    METHOD_HANDSHAKE_HELLO: handle_handshake_hello,
}


def dispatch(request: dict) -> dict | None:
    """Despacha uma requisição já decodificada para o handler do método
    correspondente. Nunca deixa uma exceção não tratada de um handler
    derrubar o loop principal.

    Devolve `None` apenas quando a requisição não tem `id` (notificação) —
    esta versão do protocolo não usa notificações, então isso não é
    exercitado na prática.
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
        print(f"[{PLUGIN_NAME}] erro inesperado ao processar {method!r}: {exc!r}", file=sys.stderr)
        if request_id is None:
            return None
        return _error(request_id, -32603, f"erro interno do plugin: {exc}")


def main() -> None:
    """Loop principal: lê NDJSON de stdin, despacha, escreve NDJSON em stdout.

    `protocol/SPEC.md` §4: cada mensagem é uma linha de JSON compacto
    terminada em `\\n`, UTF-8; uma linha vazia MUST ser ignorada
    silenciosamente. `sys.stdout.flush()` após cada escrita é crítico — sem
    ele o core fica esperando indefinidamente um pipe bufferizado.
    """
    for raw_line in sys.stdin:
        line = raw_line.rstrip("\n")
        if not line.strip():
            continue

        try:
            request = json.loads(line)
        except json.JSONDecodeError as exc:
            print(f"[{PLUGIN_NAME}] linha não é JSON válido, ignorada: {exc!r}", file=sys.stderr)
            continue

        if not isinstance(request, dict):
            print(f"[{PLUGIN_NAME}] mensagem não é um objeto JSON-RPC, ignorada: {request!r}", file=sys.stderr)
            continue

        response = dispatch(request)
        if response is None:
            continue

        sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
