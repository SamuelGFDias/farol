"""Testes de `main.py::handle_widget_get`/`handle_handshake_hello` do plugin `uptime-kuma`
(T013, `specs/010-widget-detail-surface/tasks.md`, issue #9).

Nenhum teste pré-existente deste plugin exercitava `main.py` diretamente (as 4 suítes
`test_uptime_kuma_*.py` testam só `config.py`/`metrics_parser.py`/`poller.py`/`secrets.py`) — este
arquivo cobre especificamente o contrato novo em v0.5: `widget/get` deve devolver `kind: "Monitor"`
junto de `items`, e `handshake/hello` deve declarar `protocol_version: "0.5"`.

`main.py` lê `_BASE_URL`/`_API_KEY` de variáveis de ambiente uma única vez, no import do módulo
(D8/D9 de `research.md` — sem hot-reload), e usa esse valor para decidir se inicia uma
`PollerThread` real de verdade. Para não depender do ambiente do processo de teste nem disparar
I/O de rede real, os testes abaixo importam `main` normalmente (o processo de teste tipicamente não
tem essas variáveis definidas, então `_POLLER` fica `None`) e then usam `mock.patch.object` para
sobrescrever `main._BASE_URL`/`main._API_KEY` só durante cada teste, semeando
`main._METRICS_CACHE` diretamente via `MetricsCache.record_success` (mesma API usada por
`test_uptime_kuma_poller.py`) em vez de rede real.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest import mock

PLUGIN_DIR = Path(__file__).resolve().parents[2] / "plugins" / "uptime-kuma"
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

import main  # noqa: E402


class HandleWidgetGetKindTests(unittest.TestCase):
    """`widget/get` deve sempre devolver `kind: "Monitor"` quando configurado e com dados (T010/T013)."""

    def test_widget_get_result_declares_kind_monitor(self) -> None:
        monitors = [{"name": "api_example_com", "status": "up", "response_time_ms": 42}]
        main._METRICS_CACHE.record_success(monitors)

        with mock.patch.object(main, "_BASE_URL", "https://uptime.example.com"), mock.patch.object(
            main, "_API_KEY", "fake-api-key"
        ):
            request = {
                "jsonrpc": "2.0",
                "id": 1,
                "method": "widget/get",
                "params": {"widget_id": main.WIDGET_ID},
            }
            response = main.handle_widget_get(request)

        self.assertNotIn("error", response)
        self.assertEqual(response["result"]["widget_id"], main.WIDGET_ID)
        self.assertEqual(response["result"]["kind"], "Monitor")
        self.assertEqual(response["result"]["items"], monitors)

    def test_widget_get_result_declares_kind_monitor_with_empty_items(self) -> None:
        main._METRICS_CACHE.record_success([])

        with mock.patch.object(main, "_BASE_URL", "https://uptime.example.com"), mock.patch.object(
            main, "_API_KEY", "fake-api-key"
        ):
            request = {
                "jsonrpc": "2.0",
                "id": 2,
                "method": "widget/get",
                "params": {"widget_id": main.WIDGET_ID},
            }
            response = main.handle_widget_get(request)

        self.assertEqual(response["result"]["kind"], "Monitor")
        self.assertEqual(response["result"]["items"], [])


class HandshakeProtocolVersionTests(unittest.TestCase):
    """`handshake/hello` deve declarar `protocol_version: "0.5"` (T010/T013)."""

    def test_handshake_declares_protocol_version_0_5(self) -> None:
        request = {"jsonrpc": "2.0", "id": 1, "method": "handshake/hello", "params": {}}

        response = main.handle_handshake_hello(request)

        self.assertEqual(response["result"]["protocol_version"], "0.5")


if __name__ == "__main__":
    unittest.main()
