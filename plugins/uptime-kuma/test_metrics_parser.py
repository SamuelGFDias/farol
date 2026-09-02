"""Testes de regressão para `metrics_parser.py` — só stdlib (`unittest`), sem framework externo
(D7 de `research.md`).

Rodar com: `python3 test_metrics_parser.py` (ou `python3 -m unittest test_metrics_parser`).
"""

from __future__ import annotations

import unittest

from metrics_parser import MetricsParseError, parse_metrics


class ParseMetricsTests(unittest.TestCase):
    def test_negative_response_time_becomes_none(self) -> None:
        """Linha real capturada na investigação do bug: monitor `docker` sem tempo de resposta
        aplicável emite `monitor_response_time ... -1` (sentinela do Uptime Kuma para "não
        aplicável"). O parser deve traduzir isso para `response_time_ms: None`, nunca `-1` — um
        `Option<u32>` do lado Rust rejeita negativo e derruba o decode do widget inteiro.
        """
        body = (
            '# HELP monitor_response_time Monitor Response Time (ms)\n'
            '# TYPE monitor_response_time gauge\n'
            'monitor_response_time{monitor_id="1",monitor_name="Container",'
            'monitor_type="docker",monitor_url="https://",monitor_hostname="null",'
            'monitor_port="null"} -1\n'
            '# HELP monitor_status Monitor Status\n'
            '# TYPE monitor_status gauge\n'
            'monitor_status{monitor_id="1",monitor_name="Container",'
            'monitor_type="docker",monitor_url="https://",monitor_hostname="null",'
            'monitor_port="null"} 1\n'
        )

        items = parse_metrics(body)

        self.assertEqual(
            items,
            [{"name": "Container", "status": "up", "response_time_ms": None}],
        )

    def test_non_negative_response_time_is_preserved(self) -> None:
        body = (
            'monitor_response_time{monitor_name="API"} 42.4\n'
            'monitor_status{monitor_name="API"} 1\n'
        )

        items = parse_metrics(body)

        self.assertEqual(
            items,
            [{"name": "API", "status": "up", "response_time_ms": 42}],
        )

    def test_missing_monitor_status_line_raises(self) -> None:
        with self.assertRaises(MetricsParseError):
            parse_metrics('monitor_response_time{monitor_name="API"} 12\n')


if __name__ == "__main__":
    unittest.main()
