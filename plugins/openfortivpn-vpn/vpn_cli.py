"""Wrapper síncrono sobre o binário `openfortivpn-gui` (subprocess) — traduz
`status`/`connect`/`disconnect --json` para o vocabulário do protocolo Farol.

Implementado nas tasks T021 (US1) e T028 (US2) de specs/004-vpn-status-plugin/tasks.md.

Interface interna entre este módulo e `main.py` (decisão local deste plugin, não faz parte do
protocolo Farol): `query_status()` nunca lança para os casos de erro previstos no contrato
(`specs/004-vpn-status-plugin/contracts/openfortivpn-cli-mapping.md` § `widget/get`) — em vez
disso devolve um dict-marcador `{"error": {"code": <-32003|-32008>, "reason": <str>, "detail":
<str>}}`, que `main.py::handle_widget_get` reconhece pelo `code` e traduz para o envelope
JSON-RPC de erro apropriado (mensagem PT-BR fixa por `code`, `data` construído a partir de
`reason`/`detail`). Em caso de sucesso, devolve o `VpnStatusItem` já pronto (mesma forma de
`data-model.md` §1.3), pronto para entrar em `items` sem transformação adicional.

`connect(profile)`/`disconnect()` seguem o mesmo padrão de nunca lançar (§ `action/invoke` do
mesmo contrato): em caso de falha de domínio devolvem um dict-marcador
`{"error": {"code": -32009, "reason": "vpn_action_failed", "cli_code": <str>, "message": <PT-BR>,
"detail": <str>}}` — `message` já é a tradução PT-BR da tabela do contrato (FR-007, ver
`_ACTION_ERROR_MESSAGES`), `detail` carrega o `error.message` bruto da CLI (ou uma descrição da
falha de infraestrutura, quando não há `ErrorPayload` — ex. timeout/JSON inválido, tratados como
`cli_code: "internal_error"`). Em caso de sucesso, devolve o `VpnStatusItem` já pronto, igual a
`query_status()`.
"""

from __future__ import annotations

import json
import shutil
import subprocess

STATUS_TIMEOUT_SECONDS = 10
ACTION_TIMEOUT_SECONDS = 30

_ERROR_EXEC_UNAVAILABLE = -32003
_ERROR_VPN_STATUS_UNAVAILABLE = -32008
_ERROR_VPN_ACTION_FAILED = -32009

# Tabela de tradução `error.code` (CLI) → mensagem PT-BR (FR-007), exata de
# `contracts/openfortivpn-cli-mapping.md` § "Tabela de tradução".
_ACTION_ERROR_MESSAGES = {
    "profile_not_found": "Perfil não encontrado — pode ter sido removido ou renomeado.",
    "already_connected": "Já existe uma conexão VPN ativa.",
    "not_connected": "Não há conexão VPN ativa para desconectar.",
    "connect_timeout": "A conexão não confirmou dentro do tempo esperado.",
    "sudo_denied": "Permissão de sistema negada para abrir/fechar o túnel VPN.",
    "internal_error": "Erro interno ao consultar/operar a VPN.",
}


def _error_marker(code: int, reason: str, detail: str) -> dict:
    """Monta o dict-marcador de erro interno descrito no docstring do módulo."""
    return {"error": {"code": code, "reason": reason, "detail": detail}}


def _action_error_marker(cli_code: str, cli_message: str) -> dict:
    """Monta o dict-marcador de erro `-32009`/`vpn_action_failed` para `connect`/`disconnect`.

    `message` é sempre a tradução PT-BR da tabela do contrato (FR-007) — nunca o `error.message`
    bruto da CLI como única informação; o bruto vai em `detail` para diagnóstico.
    """
    message = _ACTION_ERROR_MESSAGES.get(cli_code, _ACTION_ERROR_MESSAGES["internal_error"])
    return {
        "error": {
            "code": _ERROR_VPN_ACTION_FAILED,
            "reason": "vpn_action_failed",
            "cli_code": cli_code,
            "message": message,
            "detail": cli_message,
        }
    }


def find_binary() -> str | None:
    """Localiza o binário `openfortivpn-gui` no `PATH` — implementado em T021."""
    return shutil.which("openfortivpn-gui")


def _build_vpn_status_item(payload: dict) -> dict:
    """Mapeia um `StatusPayload` da CLI para `VpnStatusItem` (`data-model.md` §1.3/§3)."""
    state = payload["state"]
    session = payload.get("session")

    available_profiles = [
        {
            "name": name,
            "connect_action": {
                "id": "vpn.connect",
                "label": f"Conectar a {name}",
                "target": {"type": "vpn-profile", "id": name},
                "enabled": state == "disconnected",
            },
        }
        for name in payload.get("profiles", [])
    ]

    disconnect_action = {
        "id": "vpn.disconnect",
        "label": "Desconectar",
        "target": {"type": "vpn-connection", "id": "active"},
        "enabled": state == "connected",
    }

    return {
        "state": state,
        "active_profile": payload.get("selected_profile"),
        "elapsed_seconds": session["elapsed_seconds"] if session is not None else None,
        "available_profiles": available_profiles,
        "disconnect_action": disconnect_action,
    }


def query_status() -> dict:
    """Consulta o status da conexão VPN — implementado em T021.

    Ver docstring do módulo para a forma dos dois tipos de retorno possíveis (sucesso vs.
    dict-marcador de erro).
    """
    binary = find_binary()
    if binary is None:
        return _error_marker(
            _ERROR_EXEC_UNAVAILABLE,
            "exec_unavailable",
            "openfortivpn-gui não encontrado no PATH",
        )

    try:
        result = subprocess.run(
            ["openfortivpn-gui", "status", "--json"],
            capture_output=True,
            text=True,
            timeout=STATUS_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            f"openfortivpn-gui status --json excedeu o timeout de "
            f"{STATUS_TIMEOUT_SECONDS}s: {exc!r}",
        )
    except OSError as exc:
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            f"falha ao executar openfortivpn-gui: {exc!r}",
        )

    raw_output = (result.stdout or "") + (result.stderr or "")

    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            f"saída de openfortivpn-gui status --json não é JSON válido: {raw_output!r}",
        )

    if not isinstance(payload, dict):
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            f"saída de openfortivpn-gui status --json não é um objeto JSON: {raw_output!r}",
        )

    if "error" in payload:
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            f"openfortivpn-gui status --json devolveu erro: {raw_output!r}",
        )

    if "state" not in payload:
        return _error_marker(
            _ERROR_VPN_STATUS_UNAVAILABLE,
            "vpn_status_unavailable",
            "saída de openfortivpn-gui status --json não corresponde a StatusPayload "
            f"nem ErrorPayload: {raw_output!r}",
        )

    return _build_vpn_status_item(payload)


def _run_action(args: list[str]) -> dict:
    """Executa `connect`/`disconnect` e traduz o resultado — implementado em T028.

    Mesma estrutura de erro de `query_status()`: `find_binary()` ausente devolve o mesmo marcador
    `-32003`; qualquer outra falha (JSON inválido, timeout, `OSError`) é tratada como
    `cli_code: "internal_error"` (não há como distinguir a causa real sem um `ErrorPayload` da
    própria CLI, então cai na tradução PT-BR genérica da tabela do contrato).
    """
    binary = find_binary()
    if binary is None:
        return _error_marker(
            _ERROR_EXEC_UNAVAILABLE,
            "exec_unavailable",
            "openfortivpn-gui não encontrado no PATH",
        )

    try:
        result = subprocess.run(
            ["openfortivpn-gui", *args],
            capture_output=True,
            text=True,
            timeout=ACTION_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired as exc:
        return _action_error_marker(
            "internal_error",
            f"openfortivpn-gui {' '.join(args)} excedeu o timeout de "
            f"{ACTION_TIMEOUT_SECONDS}s: {exc!r}",
        )
    except OSError as exc:
        return _action_error_marker(
            "internal_error",
            f"falha ao executar openfortivpn-gui: {exc!r}",
        )

    raw_output = (result.stdout or "") + (result.stderr or "")

    try:
        payload = json.loads(result.stdout)
    except json.JSONDecodeError:
        return _action_error_marker(
            "internal_error",
            f"saída de openfortivpn-gui {' '.join(args)} não é JSON válido: {raw_output!r}",
        )

    if not isinstance(payload, dict):
        return _action_error_marker(
            "internal_error",
            f"saída de openfortivpn-gui {' '.join(args)} não é um objeto JSON: {raw_output!r}",
        )

    error = payload.get("error")
    if error is not None:
        return _action_error_marker(error.get("code", "internal_error"), error.get("message", ""))

    if "state" not in payload:
        return _action_error_marker(
            "internal_error",
            "saída de openfortivpn-gui não corresponde a StatusPayload nem ErrorPayload: "
            f"{raw_output!r}",
        )

    return _build_vpn_status_item(payload)


def connect(profile: str) -> dict:
    """Conecta a um perfil VPN — `openfortivpn-gui connect <perfil> --json` (T028)."""
    return _run_action(["connect", profile, "--json"])


def disconnect() -> dict:
    """Desconecta a VPN — `openfortivpn-gui disconnect --json` (T028)."""
    return _run_action(["disconnect", "--json"])
