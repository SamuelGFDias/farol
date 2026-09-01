"""Cliente HTTP para o endpoint `/metrics` de uma instância Uptime Kuma.

Ver `specs/002-uptime-kuma-plugin/contracts/uptime-kuma-plugin.md` § "Autenticação HTTP contra
`/metrics`" (FR-019) e `research.md` D5/D7: HTTP Basic Auth (usuário vazio, `api_key` como senha),
timeout de 10s, apenas `urllib.request` (stdlib) — nenhuma dependência externa.

Este módulo faz só a chamada HTTP e devolve o corpo bruto (texto) em sucesso, ou levanta
`MetricsUnreachableError` em qualquer falha de rede/HTTP não-2xx — quem chama (poller.py) decide o
que fazer com isso (nunca é chamado diretamente pelo handler de `widget/get`, D6 de `research.md`).
"""

from __future__ import annotations

import base64
import urllib.error
import urllib.request

HTTP_TIMEOUT_SECONDS = 10


class MetricsUnreachableError(Exception):
    """Falha de rede/HTTP ao consultar `/metrics` — mapeia para `metrics_unreachable` (-32006)."""


def build_request(base_url: str, api_key: str) -> urllib.request.Request:
    """Monta a requisição HTTP GET contra `${base_url}/metrics` com Basic Auth.

    Usuário vazio, `api_key` como senha (Clarifications do spec, `contracts/uptime-kuma-plugin.md`
    § Autenticação HTTP).
    """
    credentials = base64.b64encode(f":{api_key}".encode()).decode()
    req = urllib.request.Request(f"{base_url.rstrip('/')}/metrics")
    req.add_header("Authorization", f"Basic {credentials}")
    return req


def fetch_metrics(base_url: str, api_key: str) -> str:
    """Executa a chamada HTTP e devolve o corpo da resposta como texto (UTF-8).

    Levanta `MetricsUnreachableError` para qualquer falha de rede, timeout ou HTTP não-2xx
    (`urllib.request.urlopen` já levanta `HTTPError` para status não-2xx, `URLError`/`OSError` para
    falhas de conexão/timeout — todas mapeadas aqui numa exceção única do domínio deste plugin).
    """
    req = build_request(base_url, api_key)
    try:
        with urllib.request.urlopen(req, timeout=HTTP_TIMEOUT_SECONDS) as response:
            return response.read().decode("utf-8")
    except (urllib.error.URLError, OSError, ValueError) as exc:
        raise MetricsUnreachableError(str(exc)) from exc
