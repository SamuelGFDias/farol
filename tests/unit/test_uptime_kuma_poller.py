"""Testes de regressão para `poller.py` — só stdlib (`unittest`), sem framework externo (D7 de
`research.md`). T046 (`specs/002-uptime-kuma-plugin/tasks.md`): lógica de cache/erro do poller,
mockando a chamada HTTP (`metrics_client.fetch_metrics`) — nunca rede de verdade, nem a `Thread.run`
real (`_poll_once` é chamado diretamente; a cadência do laço `while True: ...; time.sleep(...)` de
`PollerThread.run` já é comportamento de biblioteca padrão, não deste plugin).

Rodar com: `pytest tests/unit/test_uptime_kuma_poller.py -v` (ou `python3 -m unittest
discover -s tests/unit -p "test_uptime_kuma_*.py"`) a partir da raiz do repo. Movido de
`plugins/uptime-kuma/test_poller.py` para `tests/unit/` na issue #8.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest import mock

PLUGIN_DIR = Path(__file__).resolve().parents[2] / "plugins" / "uptime-kuma"
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

from metrics_client import MetricsUnreachableError  # noqa: E402 — import depende do sys.path.insert acima
from poller import MetricsCache, PollerThread  # noqa: E402


class MetricsCacheTests(unittest.TestCase):
    def test_initial_state_has_no_success_and_a_placeholder_error(self) -> None:
        """`data-model.md` §2.3: antes de qualquer leitura, `widget/get` MUST devolver um erro
        pontual (`metrics_unreachable`, "aguardando primeira leitura"), nunca `items: []` — só uma
        instância real sem monitores (poll bem-sucedido) produz lista vazia.
        """
        cache = MetricsCache()

        last_success, last_error = cache.snapshot()

        self.assertIsNone(last_success)
        self.assertIsNotNone(last_error)
        self.assertEqual(last_error["reason"], "metrics_unreachable")

    def test_record_success_populates_last_success_without_clearing_last_error(self) -> None:
        """`record_success` só grava `last_success` — quem decide se um `last_error` antigo ainda
        importa é `main.py::handle_widget_get`, comparando os timestamps `at` dos dois (D6 de
        `research.md`), não este método.
        """
        cache = MetricsCache()

        cache.record_success([{"name": "svc", "status": "up", "response_time_ms": None}])

        last_success, last_error = cache.snapshot()
        self.assertEqual(
            last_success["monitors"], [{"name": "svc", "status": "up", "response_time_ms": None}]
        )
        self.assertIsNotNone(last_error)

    def test_record_error_overwrites_previous_error(self) -> None:
        cache = MetricsCache()

        cache.record_error("metrics_unreachable", "primeira falha")
        cache.record_error("metrics_parse_error", "segunda falha")

        _, last_error = cache.snapshot()
        self.assertEqual(last_error["reason"], "metrics_parse_error")
        self.assertEqual(last_error["detail"], "segunda falha")


class PollOnceTests(unittest.TestCase):
    """Exercita `PollerThread._poll_once` diretamente — nunca `.start()`/`Thread.run` de verdade,
    então nenhum destes testes depende de tempo real nem de uma porta de rede.
    """

    def _make_poller(self) -> PollerThread:
        return PollerThread(
            base_url="http://kuma.invalid",
            api_key="synthetic-api-key",
            cache=MetricsCache(),
        )

    def test_successful_fetch_and_parse_records_success(self) -> None:
        poller = self._make_poller()
        body = 'monitor_status{monitor_name="svc"} 1\n'

        with mock.patch("poller.fetch_metrics", return_value=body) as fetch:
            poller._poll_once()

        fetch.assert_called_once_with("http://kuma.invalid", "synthetic-api-key")
        last_success, _ = poller._cache.snapshot()
        self.assertEqual(
            last_success["monitors"],
            [{"name": "svc", "status": "up", "response_time_ms": None}],
        )

    def test_unreachable_instance_records_metrics_unreachable_error(self) -> None:
        poller = self._make_poller()

        with mock.patch(
            "poller.fetch_metrics",
            side_effect=MetricsUnreachableError("Connection refused"),
        ):
            poller._poll_once()

        last_success, last_error = poller._cache.snapshot()
        self.assertIsNone(last_success)
        self.assertEqual(last_error["reason"], "metrics_unreachable")
        self.assertIn("Connection refused", last_error["detail"])

    def test_invalid_response_body_records_metrics_parse_error(self) -> None:
        poller = self._make_poller()

        with mock.patch(
            "poller.fetch_metrics",
            return_value="<html><body>404</body></html>\n",
        ):
            poller._poll_once()

        last_success, last_error = poller._cache.snapshot()
        self.assertIsNone(last_success)
        self.assertEqual(last_error["reason"], "metrics_parse_error")

    def test_error_does_not_erase_a_previous_success(self) -> None:
        """FR-017/`data-model.md` §2.3: um erro pontual preserva o último sucesso conhecido — o
        mesmo mecanismo que `main.py::handle_widget_get` usa para decidir entre servir os dados
        antigos ou o erro (comparando `at`), nunca apagando `last_success` por conta própria.
        """
        poller = self._make_poller()
        good_body = 'monitor_status{monitor_name="svc"} 1\n'

        with mock.patch("poller.fetch_metrics", return_value=good_body):
            poller._poll_once()
        with mock.patch(
            "poller.fetch_metrics",
            side_effect=MetricsUnreachableError("timeout"),
        ):
            poller._poll_once()

        last_success, last_error = poller._cache.snapshot()
        self.assertIsNotNone(last_success)
        self.assertEqual(
            last_success["monitors"],
            [{"name": "svc", "status": "up", "response_time_ms": None}],
        )
        self.assertEqual(last_error["reason"], "metrics_unreachable")
        self.assertGreaterEqual(last_error["at"], last_success["at"])


if __name__ == "__main__":
    unittest.main()
