#!/usr/bin/env python3
"""Plugin `openfortivpn-vpn` — o lado "servidor" do protocolo Farol v0.4.

Prova que o protocolo (`protocol/SPEC.md`) é agnóstico de linguagem: este arquivo não importa nem
depende do crate Rust `farol-protocol` em nenhum momento — é uma implementação independente lendo
apenas `protocol/SPEC.md` + `protocol/schema/v0.3/*.schema.json` + os contratos em
`specs/004-vpn-status-plugin/contracts/`. Consome a CLI `openfortivpn-gui status|connect|disconnect
--json` (contrato em `../../openfortivpn-gui/specs/001-add-cli-interface/contracts/`, projeto irmão
fora deste repo) via subprocess — sem importar nada daquele projeto, só invoca o binário.

Transporte: JSON-RPC 2.0 sobre NDJSON em stdin/stdout (`protocol/SPEC.md` §2-§4). O core é sempre
quem inicia cada requisição; este processo nunca escreve nada em stdout antes de receber e responder
`handshake/hello`. `stderr` é livre para logging humano (§3) — usado aqui só para diagnóstico,
nunca faz parte do protocolo em si.

Apenas biblioteca padrão — `json`, `sys`.
Migrado para `"0.4"` como parte da feature 005 (`specs/005-docker-containers-plugin/research.md` D2) — mudança mecânica, nenhum campo novo usado por este plugin.
"""

from __future__ import annotations

import json
import sys

import vpn_cli

JSONRPC_VERSION = "2.0"
PROTOCOL_VERSION = "0.4"
PLUGIN_NAME = "openfortivpn-vpn"

WIDGET_ID = "vpn-connection"
WIDGET_KIND = "vpn-status"
WIDGET_TITLE = "VPN"

METHOD_HANDSHAKE_HELLO = "handshake/hello"
METHOD_WIDGET_GET = "widget/get"
METHOD_ACTION_INVOKE = "action/invoke"


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
    """`handshake/hello` — declara identidade, `required_config` e `capabilities` (T020).

    `capabilities` declara `exec` e `network` — este plugin invoca o binário `openfortivpn-gui` via
    subprocess, e essa CLI depende de rede para conectar de fato à VPN (mesmo raciocínio já usado
    por `plugins/git-local`, cujo `git fetch` depende de rede real; `research.md` D7 da feature
    006).
    `required_config` é sempre `[]` — este plugin não pede nenhuma
    credencial/configuração ao Farol (`specs/004-vpn-status-plugin/research.md` D6). `actions` é
    sempre `[]` neste handshake: as `ActionDeclaration`s reais (`vpn.connect` por perfil,
    `vpn.disconnect`) só são conhecíveis depois de consultar `openfortivpn-gui status`, o que só
    acontece em `widget/get` (T022) — mesmo raciocínio documentado em `handle_handshake_hello` de
    `plugins/git-local/main.py` para `fetch_action`. `widgets` declara sempre o único widget deste
    plugin, sem `suggested_refresh_interval_ms` — deixa o core aplicar o default (30000ms).
    """
    request_id = request.get("id")

    result = {
        "protocol_version": PROTOCOL_VERSION,
        "plugin_name": PLUGIN_NAME,
        "capabilities": {"capabilities": [{"kind": "exec"}, {"kind": "network"}]},
        "required_config": [],
        "widgets": [
            {
                "id": WIDGET_ID,
                "kind": WIDGET_KIND,
                "title": WIDGET_TITLE,
            }
        ],
        "actions": [],
    }
    return _success(request_id, result)


def handle_widget_get(request: dict) -> dict:
    """`widget/get` — devolve `VpnStatusItem` a partir de `openfortivpn-gui status --json` (T022).

    `vpn_cli.query_status()` nunca lança para os casos de erro previstos pelo contrato
    (`contracts/openfortivpn-cli-mapping.md` § `widget/get`) — devolve um dict-marcador
    `{"error": {"code", "reason", "detail"}}` (ver docstring de `vpn_cli.py`), que este handler
    traduz para o envelope JSON-RPC de erro apropriado. `items` é sempre length 0 ou 1
    (`research.md` D3, widget singleton) — aqui sempre 1 em caso de sucesso.
    """
    request_id = request.get("id")

    status = vpn_cli.query_status()

    error = status.get("error")
    if error is not None:
        code = error["code"]
        if code == -32003:
            return _error(request_id, -32003, "openfortivpn-gui não encontrado no PATH")
        return _error(
            request_id,
            -32008,
            "falha ao consultar status da VPN",
            data={"reason": "vpn_status_unavailable", "detail": error["detail"]},
        )

    return _success(
        request_id,
        {"widget_id": request["params"]["widget_id"], "items": [status]},
    )


def handle_action_invoke(request: dict) -> dict:
    """`action/invoke` — `vpn.connect`/`vpn.disconnect` via `vpn_cli.connect`/`disconnect` (T029).

    Dispatch por `action_id`/`target.type` (`contracts/openfortivpn-cli-mapping.md` §
    `action/invoke`). `vpn_cli.connect`/`disconnect` nunca lançam para os casos de erro previstos
    — devolvem o mesmo estilo de dict-marcador de `query_status()` (ver docstring de `vpn_cli.py`),
    que este handler traduz para o envelope JSON-RPC de erro (`-32009`/`vpn_action_failed`, forma
    de `ActionInvokeResponseError`) ou de sucesso (`ActionInvokeResult::Vpn`, campo `vpn_status`).
    Qualquer outra combinação de `action_id`/`target` é erro de uso (`-32602`) — não deveria
    acontecer em uso normal, já que o core só invoca `target`s que o próprio plugin declarou.
    """
    request_id = request.get("id")
    params = request["params"]
    action_id = params["action_id"]
    target = params["target"]

    if action_id == "vpn.connect" and target.get("type") == "vpn-profile":
        status = vpn_cli.connect(target["id"])
    elif action_id == "vpn.disconnect" and target.get("type") == "vpn-connection":
        status = vpn_cli.disconnect()
    else:
        return _error(
            request_id,
            -32602,
            f"action_id/target não reconhecido: {action_id!r}/{target!r}",
        )

    error = status.get("error")
    if error is not None:
        return _error(
            request_id,
            -32009,
            error["message"],
            data={
                "reason": "vpn_action_failed",
                "detail": {"cli_code": error["cli_code"], "cli_message": error["detail"]},
            },
        )

    return _success(request_id, {"vpn_status": status})


DISPATCH = {
    METHOD_HANDSHAKE_HELLO: handle_handshake_hello,
    METHOD_WIDGET_GET: handle_widget_get,
    METHOD_ACTION_INVOKE: handle_action_invoke,
}


def dispatch(request: dict) -> dict | None:
    """Despacha uma requisição já decodificada para o handler do método correspondente.

    Nunca deixa uma exceção não tratada de um handler derrubar o loop principal (reforça
    FR-017/FR-019 além do escopo específico de `action/invoke`) — qualquer exceção inesperada vira
    um erro `-32603 internal error` pontual, logado em stderr para diagnóstico.

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
        print(
            f"[openfortivpn-vpn] erro inesperado ao processar {method!r}: {exc!r}",
            file=sys.stderr,
        )
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
            print(f"[openfortivpn-vpn] linha não é JSON válido, ignorada: {exc!r}", file=sys.stderr)
            continue

        if not isinstance(request, dict):
            print(
                f"[openfortivpn-vpn] mensagem não é um objeto JSON-RPC, ignorada: {request!r}",
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
