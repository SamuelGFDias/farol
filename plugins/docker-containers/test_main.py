"""Testes de `main.py::handle_widget_get`/`handle_handshake_hello` do plugin `docker-containers`
(T013, `specs/010-widget-detail-surface/tasks.md`, issue #9).

`test_docker_cli.py` (colocado neste mesmo diretório) testa só `docker_cli.list_containers` —
nenhum teste pré-existente exercitava `main.py`. Este arquivo cobre especificamente o contrato novo
em v0.5: `widget/get` deve devolver `kind: "Container"` junto de `items`, e `handshake/hello` deve
declarar `protocol_version: "0.5"`.

Mesmo padrão de import direto de `test_docker_cli.py` (mesmo diretório, sem `sys.path.insert`).
Rodar com: `python3 -m unittest test_main.py -v` (ou `python3 -m unittest discover -p "test_*.py"`)
a partir de `plugins/docker-containers/`.
"""

from __future__ import annotations

import unittest
from unittest import mock

import main


class HandleWidgetGetKindTests(unittest.TestCase):
    """`widget/get` deve sempre devolver `kind: "Container"` (T012/T013)."""

    @mock.patch(
        "main.docker_cli.list_containers",
        return_value={
            "items": [
                {
                    "id": "a" * 64,
                    "name": "web",
                    "image": "nginx:latest",
                    "state": "running",
                    "start_action": None,
                    "stop_action": {
                        "action_id": "docker.container.stop",
                        "target": {"type": "docker-container", "id": "a" * 64},
                    },
                    "restart_action": {
                        "action_id": "docker.container.restart",
                        "target": {"type": "docker-container", "id": "a" * 64},
                    },
                }
            ]
        },
    )
    def test_widget_get_result_declares_kind_container(self, _mock_list: mock.Mock) -> None:
        request = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "widget/get",
            "params": {"widget_id": main.WIDGET_ID},
        }

        response = main.handle_widget_get(request)

        self.assertNotIn("error", response)
        self.assertEqual(response["result"]["widget_id"], main.WIDGET_ID)
        self.assertEqual(response["result"]["kind"], "Container")
        self.assertEqual(len(response["result"]["items"]), 1)

    @mock.patch("main.docker_cli.list_containers", return_value={"items": []})
    def test_widget_get_result_declares_kind_container_with_empty_items(
        self, _mock_list: mock.Mock
    ) -> None:
        request = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "widget/get",
            "params": {"widget_id": main.WIDGET_ID},
        }

        response = main.handle_widget_get(request)

        self.assertEqual(response["result"]["kind"], "Container")
        self.assertEqual(response["result"]["items"], [])


class HandshakeProtocolVersionTests(unittest.TestCase):
    """`handshake/hello` deve declarar `protocol_version: "0.5"` (T012/T013)."""

    def test_handshake_declares_protocol_version_0_5(self) -> None:
        request = {"jsonrpc": "2.0", "id": 1, "method": "handshake/hello", "params": {}}

        response = main.handle_handshake_hello(request)

        self.assertEqual(response["result"]["protocol_version"], "0.5")


if __name__ == "__main__":
    unittest.main()
