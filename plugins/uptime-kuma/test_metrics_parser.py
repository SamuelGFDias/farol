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

    def test_instance_with_no_monitors_returns_empty_items(self) -> None:
        """T051 (débito #5, issue #7): uma instância Uptime Kuma real, acessível, mas sem nenhum
        monitor cadastrado ainda emite a declaração `# HELP`/`# TYPE monitor_status` — só não emite
        nenhuma amostra. Isso é sucesso (`items: []`), não `MetricsParseError` — distinto de um
        corpo genuinamente não reconhecível (sem declaração nenhuma), coberto pelo teste acima.
        """
        body = (
            "# HELP monitor_cert_days_remaining Monitor Certificate Days Remaining\n"
            "# TYPE monitor_cert_days_remaining gauge\n"
            "# HELP monitor_response_time Monitor Response Time (ms)\n"
            "# TYPE monitor_response_time gauge\n"
            "# HELP monitor_status Monitor Status\n"
            "# TYPE monitor_status gauge\n"
        )

        items = parse_metrics(body)

        self.assertEqual(items, [])

    def test_non_metrics_body_still_raises(self) -> None:
        """Corpo sem nenhuma declaração `monitor_status` (ex.: página HTML de erro) continua
        `MetricsParseError` — não deve ser confundido com "zero monitores".
        """
        with self.assertRaises(MetricsParseError):
            parse_metrics("<html><body><h1>404 Not Found</h1></body></html>\n")

    def test_status_mapping_covers_all_four_known_values(self) -> None:
        """FR-012: os quatro valores conhecidos de `monitor_status` (`STATUS_MAP`) mapeiam para o
        rótulo esperado — `1→up`, `0→down`, `2→pending`, `3→maintenance`.
        """
        known_values = [(1, "up"), (0, "down"), (2, "pending"), (3, "maintenance")]
        for raw_value, expected_label in known_values:
            with self.subTest(raw_value=raw_value):
                body = f'monitor_status{{monitor_name="svc"}} {raw_value}\n'

                items = parse_metrics(body)

                self.assertEqual(
                    items,
                    [{"name": "svc", "status": expected_label, "response_time_ms": None}],
                )

    def test_status_value_outside_known_domain_raises(self) -> None:
        """Um valor de `monitor_status` fora de `{0, 1, 2, 3}` invalida a resposta inteira (D7 de
        `research.md`) — não é uma linha malformada isolada, pulada em silêncio.
        """
        with self.assertRaises(MetricsParseError):
            parse_metrics('monitor_status{monitor_name="svc"} 4\n')

    def test_malformed_status_line_without_monitor_name_is_skipped(self) -> None:
        """Linha malformada isolada (sem `monitor_name`) dentro da família reconhecida é pulada —
        tolerância parcial (`research.md` D7) — não invalida o parse inteiro quando outra linha
        válida está presente.
        """
        body = (
            'monitor_status{monitor_type="http"} 1\n'
            'monitor_status{monitor_name="svc"} 1\n'
        )

        items = parse_metrics(body)

        self.assertEqual(items, [{"name": "svc", "status": "up", "response_time_ms": None}])


if __name__ == "__main__":
    unittest.main()
