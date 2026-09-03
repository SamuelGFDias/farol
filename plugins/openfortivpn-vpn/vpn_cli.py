"""Wrapper síncrono sobre o binário `openfortivpn-gui` (subprocess) — traduz
`status`/`connect`/`disconnect --json` para o vocabulário do protocolo Farol.

Implementado na task T021 (US1) de specs/004-vpn-status-plugin/tasks.md. `connect`/`disconnect`
(T028, US2) ainda são stubs `NotImplementedError` nesta fase.

Interface interna entre este módulo e `main.py` (decisão local deste plugin, não faz parte do
protocolo Farol): `query_status()` nunca lança para os casos de erro previstos no contrato
(`specs/004-vpn-status-plugin/contracts/openfortivpn-cli-mapping.md` § `widget/get`) — em vez
disso devolve um dict-marcador `{"error": {"code": <-32003|-32008>, "reason": <str>, "detail":
<str>}}`, que `main.py::handle_widget_get` reconhece pelo `code` e traduz para o envelope
JSON-RPC de erro apropriado (mensagem PT-BR fixa por `code`, `data` construído a partir de
`reason`/`detail`). Em caso de sucesso, devolve o `VpnStatusItem` já pronto (mesma forma de
`data-model.md` §1.3), pronto para entrar em `items` sem transformação adicional.
"""

from __future__ import annotations

import json
import shutil
import subprocess

STATUS_TIMEOUT_SECONDS = 10

_ERROR_EXEC_UNAVAILABLE = -32003
_ERROR_VPN_STATUS_UNAVAILABLE = -32008


def _error_marker(code: int, reason: str, detail: str) -> dict:
    """Monta o dict-marcador de erro interno descrito no docstring do módulo."""
    return {"error": {"code": code, "reason": reason, "detail": detail}}


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


def connect(profile: str) -> dict:
    """Conecta a um perfil VPN — `openfortivpn-gui connect <perfil> --json`.

    Ainda não implementado nesta subtarefa (US1) — escopo de T028 (US2).
    """
    raise NotImplementedError("connect será implementado em T028 (US2)")


def disconnect() -> dict:
    """Desconecta a VPN — `openfortivpn-gui disconnect --json`.

    Ainda não implementado nesta subtarefa (US1) — escopo de T028 (US2).
    """
    raise NotImplementedError("disconnect será implementado em T028 (US2)")
