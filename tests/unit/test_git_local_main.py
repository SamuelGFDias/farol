"""Testes de `main.py::handle_widget_get`/`handle_handshake_hello` do plugin `git-local`
(T013, `specs/010-widget-detail-surface/tasks.md`, issue #9).

Nenhum teste pré-existente deste plugin exercitava `main.py` diretamente (`test_git_local_scan.py`
testa só `scan.scan_repositories` contra repositórios git reais) — este arquivo cobre especificamente
o contrato novo em v0.5: `widget/get` deve devolver `kind: "Git"` junto de `items`, e
`handshake/hello` deve declarar `protocol_version: "0.5"`.

Escolha de framework: `unittest` (mesmo raciocínio de D3/`research.md` documentado em
`test_git_local_scan.py` — biblioteca padrão, nenhuma dependência externa nova). `scan.scan_repositories`
e `config.load_scan_root` são mockados aqui (ao contrário de `test_git_local_scan.py`, que usa
repositórios git reais) porque o que este arquivo verifica é a forma do envelope JSON-RPC que
`handle_widget_get` monta em torno do valor de retorno dessas funções, não o comportamento da
varredura em si — já coberto pela outra suíte.
"""

from __future__ import annotations

import sys
import unittest
from pathlib import Path
from unittest import mock

PLUGIN_DIR = Path(__file__).resolve().parents[2] / "plugins" / "git-local"
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

import main  # noqa: E402


class HandleWidgetGetKindTests(unittest.TestCase):
    """`widget/get` deve sempre devolver `kind: "Git"` (T009/T013)."""

    @mock.patch("main.scan.scan_repositories", return_value=[])
    @mock.patch("main.config.load_scan_root", return_value=Path("/tmp/does-not-matter"))
    def test_widget_get_result_declares_kind_git_with_empty_items(
        self, _mock_scan_root: mock.Mock, _mock_scan: mock.Mock
    ) -> None:
        request = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "widget/get",
            "params": {"widget_id": main.WIDGET_ID},
        }

        response = main.handle_widget_get(request)

        self.assertNotIn("error", response)
        self.assertEqual(response["result"]["widget_id"], main.WIDGET_ID)
        self.assertEqual(response["result"]["kind"], "Git")
        self.assertEqual(response["result"]["items"], [])

    @mock.patch(
        "main.scan.scan_repositories",
        return_value=[
            {
                "id": "/tmp/repo",
                "name": "repo",
                "branch": "main",
                "remote_status": "in_sync",
                "has_uncommitted_changes": False,
                "fetch_action": {"action_id": main.ACTION_ID, "target": {"type": "repo", "id": "/tmp/repo"}},
            }
        ],
    )
    @mock.patch("main.config.load_scan_root", return_value=Path("/tmp/does-not-matter"))
    def test_widget_get_result_declares_kind_git_with_items(
        self, _mock_scan_root: mock.Mock, _mock_scan: mock.Mock
    ) -> None:
        request = {
            "jsonrpc": "2.0",
            "id": 2,
            "method": "widget/get",
            "params": {"widget_id": main.WIDGET_ID},
        }

        response = main.handle_widget_get(request)

        self.assertEqual(response["result"]["kind"], "Git")
        self.assertEqual(len(response["result"]["items"]), 1)


class HandshakeProtocolVersionTests(unittest.TestCase):
    """`handshake/hello` deve declarar `protocol_version: "0.5"` (T009/T013)."""

    def test_handshake_declares_protocol_version_0_5(self) -> None:
        request = {"jsonrpc": "2.0", "id": 1, "method": "handshake/hello", "params": {}}

        response = main.handle_handshake_hello(request)

        self.assertEqual(response["result"]["protocol_version"], "0.5")


if __name__ == "__main__":
    unittest.main()
