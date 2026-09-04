#!/usr/bin/env python3
"""Plugin `docker-containers` — o lado "servidor" do protocolo Farol v0.4.

Prova que o protocolo (`protocol/SPEC.md`) é agnóstico de linguagem: este arquivo não importa nem
depende do crate Rust `farol-protocol` em nenhum momento — é uma implementação independente lendo
apenas `protocol/SPEC.md` + `protocol/schema/v0.4/*.schema.json` + os contratos em
`specs/005-docker-containers-plugin/contracts/`. Consome o CLI `docker ps`/`start`/`stop`/`restart`
(binário já instalado na máquina) via subprocess — sem duplicar lógica de gerenciamento de
container.

Transporte: JSON-RPC 2.0 sobre NDJSON em stdin/stdout (`protocol/SPEC.md` §2-§4). O core é sempre
quem inicia cada requisição; este processo nunca escreve nada em stdout antes de receber e responder
`handshake/hello`. `stderr` é livre para logging humano (§3) — usado aqui só para diagnóstico,
nunca faz parte do protocolo em si.

Apenas biblioteca padrão — `json`, `sys`.
"""

from __future__ import annotations

import json
import sys

import docker_cli

JSONRPC_VERSION = "2.0"
PROTOCOL_VERSION = "0.4"
PLUGIN_NAME = "docker-containers"

WIDGET_ID = "docker-containers"
WIDGET_KIND = "container-status-grid"
WIDGET_TITLE = "Containers Docker"

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

    `capabilities` declara apenas `exec` — este plugin só invoca o binário `docker` via subprocess,
    nunca fala com a rede diretamente (mesmo raciocínio já usado por `plugins/git-local` para o
    binário `git` e por `plugins/openfortivpn-vpn` para `openfortivpn-gui`). `required_config` é
    sempre `[]` — este plugin não pede nenhuma credencial/configuração ao Farol (sem tela de setup,
    `specs/005-docker-containers-plugin/research.md` D9). `actions` é sempre `[]` neste handshake:
    as três `ActionDeclaration` por container (start/stop/restart) só são conhecíveis depois de
    consultar `docker ps`, o que só acontece em `widget/get` (T022) — mesmo raciocínio documentado
    em `handle_handshake_hello` de `plugins/openfortivpn-vpn/main.py`. `widgets` declara sempre o
    único widget deste plugin, sem `suggested_refresh_interval_ms` — deixa o core aplicar o default.
    """
    request_id = request.get("id")

    result = {
        "protocol_version": PROTOCOL_VERSION,
        "plugin_name": PLUGIN_NAME,
        "capabilities": {"capabilities": [{"kind": "exec"}]},
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
    """`widget/get` — devolve status dos containers Docker (T022).

    `docker_cli.list_containers()` nunca lança para os casos de erro previstos pelo contrato
    (`contracts/docker-cli-mapping.md` § `widget/get`) — devolve um dict-marcador
    `{"error": {"code", "condition"}}` (ver docstring de `docker_cli.py`), que este handler
    traduz para o envelope JSON-RPC de erro apropriado. Lista vazia é sucesso normal, `items: []`
    (FR-011), nunca erro.
    """
    request_id = request.get("id")

    result = docker_cli.list_containers()

    error = result.get("error")
    if error is not None:
        code = error["code"]
        if code == -32003:
            return _error(request_id, -32003, "docker não encontrado no PATH")
        return _error(
            request_id,
            -32010,
            "falha ao consultar containers Docker",
            data={"reason": "docker_unavailable", "detail": {"condition": error["condition"]}},
        )

    return _success(
        request_id,
        {"widget_id": request["params"]["widget_id"], "items": result["items"]},
    )


def handle_action_invoke(request: dict) -> dict:
    """`action/invoke` — executa ações sobre containers Docker.

    Ainda não implementado nesta subtarefa (Setup/Foundational) — o dispatch real é escopo de
    T029 (US2), `specs/005-docker-containers-plugin/tasks.md`.
    """
    raise NotImplementedError("handle_action_invoke será implementado em T029 (US2)")


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
            f"[docker-containers] erro inesperado ao processar {method!r}: {exc!r}",
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
            print(
                f"[docker-containers] linha não é JSON válido, ignorada: {exc!r}",
                file=sys.stderr,
            )
            continue

        if not isinstance(request, dict):
            print(
                f"[docker-containers] mensagem não é um objeto JSON-RPC, ignorada: {request!r}",
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
