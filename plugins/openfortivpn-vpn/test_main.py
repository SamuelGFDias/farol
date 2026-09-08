"""Testes de `main.py::handle_widget_get`/`handle_handshake_hello` do plugin `openfortivpn-vpn`
(T013, `specs/010-widget-detail-surface/tasks.md`, issue #9).

`test_vpn_cli.py` (colocado neste mesmo diretório) testa só `vpn_cli.query_status`/`connect`/
`disconnect` — nenhum teste pré-existente exercitava `main.py`. Este arquivo cobre especificamente
o contrato novo em v0.5: `widget/get` deve devolver `kind: "Vpn"` junto de `items` (sempre lista de
tamanho 0 ou 1, `research.md` D3 da feature 004), e `handshake/hello` deve declarar
`protocol_version: "0.5"`.

Mesmo padrão de import direto de `test_vpn_cli.py` (mesmo diretório, sem `sys.path.insert`).
Rodar com: `python3 -m unittest test_main.py -v` (ou `python3 -m unittest discover -p "test_*.py"`)
a partir de `plugins/openfortivpn-vpn/`.
"""

from __future__ import annotations

import unittest
from unittest import mock

import main


class HandleWidgetGetKindTests(unittest.TestCase):
    """`widget/get` deve sempre devolver `kind: "Vpn"` com sucesso (T011/T013)."""

    @mock.patch(
        "main.vpn_cli.query_status",
        return_value={
            "state": "connected",
            "active_profile": "work",
            "elapsed_seconds": 120,
            "available_profiles": [],
            "disconnect_action": {
                "action_id": "vpn.disconnect",
                "target": {"type": "vpn-connection", "id": "vpn-connection"},
            },
        },
    )
    def test_widget_get_result_declares_kind_vpn(self, _mock_query_status: mock.Mock) -> None:
        request = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "widget/get",
            "params": {"widget_id": main.WIDGET_ID},
        }

        response = main.handle_widget_get(request)

        self.assertNotIn("error", response)
        self.assertEqual(response["result"]["widget_id"], main.WIDGET_ID)
        self.assertEqual(response["result"]["kind"], "Vpn")
        self.assertEqual(len(response["result"]["items"]), 1)


class HandshakeProtocolVersionTests(unittest.TestCase):
    """`handshake/hello` deve declarar `protocol_version: "0.5"` (T011/T013)."""

    def test_handshake_declares_protocol_version_0_5(self) -> None:
        request = {"jsonrpc": "2.0", "id": 1, "method": "handshake/hello", "params": {}}

        response = main.handle_handshake_hello(request)

        self.assertEqual(response["result"]["protocol_version"], "0.5")


if __name__ == "__main__":
    unittest.main()
