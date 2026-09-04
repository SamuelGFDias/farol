#!/usr/bin/env python3
"""Plugin de referência `git-local` — o lado "servidor" do protocolo Farol v0.4.

Prova que o protocolo (`protocol/SPEC.md`) é agnóstico de linguagem (D3 de `research.md`): este
arquivo não importa nem depende do crate Rust `farol-protocol` em nenhum momento — é uma
implementação independente lendo apenas `protocol/SPEC.md` + `protocol/schema/v0.2/*.schema.json`
+ os contratos em `specs/001-walking-skeleton-git-plugin/contracts/`. `crates/farol-protocol/src/
messages.rs` foi consultado só como referência cruzada de vocabulário de campo (nomes JSON exatos,
já que ele serializa/desserializa contra os mesmos schemas), nunca como fonte normativa.

Migrado de `protocol_version = "0.1"` para `"0.2"` (débito técnico #4 / T050,
`specs/002-uptime-kuma-plugin/tasks.md`) — o único campo do handshake que muda é `capabilities`
(passa de `string[]` para `Capability[]` estruturado, `research.md` D1 da feature 002) mais o campo
novo `required_config` (D8 da mesma feature), sempre `[]` aqui: `git-local` não tem nenhuma
credencial/configuração a declarar. `widgets`/`actions` permanecem exatamente como antes.
Migrado novamente para `"0.3"` como parte da feature 004 (`specs/004-vpn-status-plugin/research.md` D2)
— mudança mecânica, nenhum campo novo usado por este plugin.
Migrado novamente para `"0.4"` como parte da feature 005 (`specs/005-docker-containers-plugin/research.md` D2) — mudança mecânica, nenhum campo novo usado por este plugin.

Transporte: JSON-RPC 2.0 sobre NDJSON em stdin/stdout (`protocol/SPEC.md` §2-§4). O core é sempre
quem inicia cada requisição; este processo nunca escreve nada em stdout antes de receber e
responder `handshake/hello`. `stderr` é livre para logging humano (§3) — usado aqui só para
diagnóstico, nunca faz parte do protocolo em si.

Apenas biblioteca padrão (D3 de `research.md`) — `json`, `sys`, `pathlib`.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

import config
import scan

JSONRPC_VERSION = "2.0"
PROTOCOL_VERSION = "0.4"
PLUGIN_NAME = "git-local"

WIDGET_ID = "repo-status"
WIDGET_KIND = "status-grid"
WIDGET_TITLE = "Repositórios Git"

METHOD_HANDSHAKE_HELLO = "handshake/hello"
METHOD_WIDGET_GET = "widget/get"
METHOD_ACTION_INVOKE = "action/invoke"

ACTION_ID = scan.ACTION_ID


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
    """`handshake/hello` — `protocol/SPEC.md` §6, `contracts/handshake.md`.

    `actions` vem sempre vazio aqui (§6.3.1 do SPEC / nota de sequenciamento em
    `contracts/handshake.md`): a lista real de ações de fetch só é conhecível depois de varrer
    `scan_root`, e essa varredura é a mesma operação de `widget/get` — bloquear o handshake nela
    arriscaria estourar `RPC_TIMEOUT_CONTROL` em `scan_root`s grandes. Cada `fetch_action` chega ao
    core dentro do `WidgetItem` correspondente em `widget/get` (ver `handle_widget_get`).
    `suggested_refresh_interval_ms` é deliberadamente omitido — este plugin de referência exercita
    o ramo "default de 30s" do core (FR-011), não o ramo de sugestão.
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
    """`widget/get` — `protocol/SPEC.md` §5.2, `contracts/widget-protocol.md`.

    Sem cache entre chamadas: cada `widget/get` refaz a varredura de `scan_root` (aceitável dado o
    ciclo de refresh de 30s, `contracts/git-local-plugin.md` § Varredura).
    """
    request_id = request.get("id")
    params = request.get("params") or {}
    widget_id = params.get("widget_id")

    if widget_id != WIDGET_ID:
        # widget_id desconhecido: erro JSON-RPC padrão (protocol/SPEC.md §5.2), não um erro de
        # domínio Farol — este caminho não é exercitado pelo core de referência.
        return _error(request_id, -32602, f"widget_id desconhecido: {widget_id!r}")

    scan_root = config.load_scan_root()
    try:
        items = scan.scan_repositories(scan_root)
    except scan.ExecUnavailableError:
        return _error(
            request_id,
            -32003,
            "binário git não está disponível no sistema",
            {"reason": "exec_unavailable"},
        )
    except scan.ScanRootUnreadableError as exc:
        return _error(
            request_id,
            -32004,
            f"scan_root inacessível por permissão: {exc}",
            {"reason": "scan_root_unreadable"},
        )

    return _success(request_id, {"widget_id": WIDGET_ID, "items": items})


def handle_action_invoke(request: dict) -> dict:
    """`action/invoke` para `git.fetch` — `protocol/SPEC.md` §5.3, `contracts/action-protocol.md`.

    Uma falha aqui (fetch_failed ou exec_unavailable) MUST NOT encerrar o processo do plugin
    (FR-017) — sempre uma resposta de erro JSON-RPC normal; o loop principal continua respondendo
    depois.
    """
    request_id = request.get("id")
    params = request.get("params") or {}
    action_id = params.get("action_id")
    target = params.get("target") or {}

    if action_id != ACTION_ID:
        return _error(request_id, -32602, f"action_id desconhecido: {action_id!r}")

    repo_path = Path(target.get("id", ""))
    try:
        repo = scan.fetch(repo_path)
    except scan.ExecUnavailableError:
        return _error(
            request_id,
            -32003,
            "binário git não está disponível no sistema",
            {"reason": "exec_unavailable", "target": target},
        )
    except scan.FetchFailedError as exc:
        return _error(
            request_id,
            -32001,
            "git fetch falhou",
            {"reason": "fetch_failed", "target": target, "detail": exc.detail},
        )

    return _success(request_id, {"repo": repo})


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
        print(f"[git-local] erro inesperado ao processar {method!r}: {exc!r}", file=sys.stderr)
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
            print(f"[git-local] linha não é JSON válido, ignorada: {exc!r}", file=sys.stderr)
            continue

        if not isinstance(request, dict):
            print(f"[git-local] mensagem não é um objeto JSON-RPC, ignorada: {request!r}", file=sys.stderr)
            continue

        response = dispatch(request)
        if response is None:
            continue

        sys.stdout.write(json.dumps(response, separators=(",", ":")) + "\n")
        sys.stdout.flush()


if __name__ == "__main__":
    main()
