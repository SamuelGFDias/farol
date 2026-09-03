"""Testes de `vpn_cli.query_status` — só stdlib (`unittest`), sem framework externo.

Rodar com: `python3 test_vpn_cli.py` (ou `python3 -m unittest test_vpn_cli`).
"""

from __future__ import annotations

import json
import subprocess
import unittest
from unittest.mock import patch

import vpn_cli


def _completed(stdout: str, returncode: int = 0, stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["openfortivpn-gui", "status", "--json"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


class QueryStatusTests(unittest.TestCase):
    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connected_with_session_populates_elapsed_seconds(self, mock_run, _mock_which) -> None:
        payload = {
            "state": "connected",
            "selected_profile": "trabalho",
            "profiles": ["trabalho", "casa"],
            "session": {
                "profile": "trabalho",
                "iface": "ppp0",
                "started_at": 1234567890.0,
                "elapsed_seconds": 42.5,
            },
        }
        mock_run.return_value = _completed(json.dumps(payload))

        result = vpn_cli.query_status()

        self.assertNotIn("error", result)
        self.assertEqual(result["state"], "connected")
        self.assertEqual(result["active_profile"], "trabalho")
        self.assertEqual(result["elapsed_seconds"], 42.5)
        self.assertTrue(result["disconnect_action"]["enabled"])
        for profile in result["available_profiles"]:
            self.assertFalse(profile["connect_action"]["enabled"])

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnected_with_two_profiles(self, mock_run, _mock_which) -> None:
        payload = {
            "state": "disconnected",
            "selected_profile": None,
            "profiles": ["trabalho", "casa"],
            "session": None,
        }
        mock_run.return_value = _completed(json.dumps(payload))

        result = vpn_cli.query_status()

        self.assertNotIn("error", result)
        self.assertIsNone(result["elapsed_seconds"])
        self.assertEqual(len(result["available_profiles"]), 2)
        for profile in result["available_profiles"]:
            self.assertTrue(profile["connect_action"]["enabled"])
        self.assertFalse(result["disconnect_action"]["enabled"])

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnected_with_no_profiles_is_not_an_error(self, mock_run, _mock_which) -> None:
        payload = {
            "state": "disconnected",
            "selected_profile": None,
            "profiles": [],
            "session": None,
        }
        mock_run.return_value = _completed(json.dumps(payload))

        result = vpn_cli.query_status()

        self.assertNotIn("error", result)
        self.assertEqual(result["available_profiles"], [])

    @patch("vpn_cli.shutil.which", return_value=None)
    def test_binary_missing_returns_exec_unavailable_marker(self, _mock_which) -> None:
        result = vpn_cli.query_status()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32003)
        self.assertEqual(result["error"]["reason"], "exec_unavailable")

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_cli_error_payload_returns_vpn_status_unavailable_marker(
        self, mock_run, _mock_which
    ) -> None:
        payload = {"error": {"code": "internal_error", "message": "algo deu errado"}}
        mock_run.return_value = _completed(json.dumps(payload))

        result = vpn_cli.query_status()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32008)
        self.assertEqual(result["error"]["reason"], "vpn_status_unavailable")

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_invalid_json_output_returns_vpn_status_unavailable_marker(
        self, mock_run, _mock_which
    ) -> None:
        mock_run.return_value = _completed("isto não é JSON")

        result = vpn_cli.query_status()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32008)
        self.assertEqual(result["error"]["reason"], "vpn_status_unavailable")


def _action_completed(
    args: list[str], stdout: str, returncode: int = 0, stderr: str = ""
) -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["openfortivpn-gui", *args], returncode=returncode, stdout=stdout, stderr=stderr
    )


class ConnectDisconnectTests(unittest.TestCase):
    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connect_success_maps_to_vpn_status_item(self, mock_run, _mock_which) -> None:
        payload = {
            "state": "connected",
            "selected_profile": "trabalho",
            "profiles": ["trabalho", "casa"],
            "session": {
                "profile": "trabalho",
                "iface": "ppp0",
                "started_at": 1234567890.0,
                "elapsed_seconds": 0.1,
            },
        }
        mock_run.return_value = _action_completed(
            ["connect", "trabalho", "--json"], json.dumps(payload)
        )

        result = vpn_cli.connect("trabalho")

        self.assertNotIn("error", result)
        self.assertEqual(result["state"], "connected")
        self.assertEqual(result["active_profile"], "trabalho")

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnect_success_maps_to_vpn_status_item(self, mock_run, _mock_which) -> None:
        payload = {
            "state": "disconnected",
            "selected_profile": None,
            "profiles": ["trabalho"],
            "session": None,
        }
        mock_run.return_value = _action_completed(["disconnect", "--json"], json.dumps(payload))

        result = vpn_cli.disconnect()

        self.assertNotIn("error", result)
        self.assertEqual(result["state"], "disconnected")
        self.assertIsNone(result["elapsed_seconds"])

    def _assert_action_error(
        self, mock_run, action_callable, args: list[str], cli_code: str, expected_message: str
    ) -> None:
        cli_message = f"mensagem bruta para {cli_code}"
        error_payload = {"error": {"code": cli_code, "message": cli_message}}
        mock_run.return_value = _action_completed(args, json.dumps(error_payload), returncode=1)

        result = action_callable()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32009)
        self.assertEqual(result["error"]["reason"], "vpn_action_failed")
        self.assertEqual(result["error"]["cli_code"], cli_code)
        self.assertEqual(result["error"]["message"], expected_message)
        self.assertEqual(result["error"]["detail"], cli_message)

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connect_profile_not_found_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            lambda: vpn_cli.connect("inexistente"),
            ["connect", "inexistente", "--json"],
            "profile_not_found",
            "Perfil não encontrado — pode ter sido removido ou renomeado.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connect_already_connected_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            lambda: vpn_cli.connect("trabalho"),
            ["connect", "trabalho", "--json"],
            "already_connected",
            "Já existe uma conexão VPN ativa.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connect_timeout_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            lambda: vpn_cli.connect("trabalho"),
            ["connect", "trabalho", "--json"],
            "connect_timeout",
            "A conexão não confirmou dentro do tempo esperado.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_connect_sudo_denied_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            lambda: vpn_cli.connect("trabalho"),
            ["connect", "trabalho", "--json"],
            "sudo_denied",
            "Permissão de sistema negada para abrir/fechar o túnel VPN.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnect_not_connected_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            vpn_cli.disconnect,
            ["disconnect", "--json"],
            "not_connected",
            "Não há conexão VPN ativa para desconectar.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnect_sudo_denied_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            vpn_cli.disconnect,
            ["disconnect", "--json"],
            "sudo_denied",
            "Permissão de sistema negada para abrir/fechar o túnel VPN.",
        )

    @patch("vpn_cli.shutil.which", return_value="/usr/bin/openfortivpn-gui")
    @patch("vpn_cli.subprocess.run")
    def test_disconnect_internal_error_translated(self, mock_run, _mock_which) -> None:
        self._assert_action_error(
            mock_run,
            vpn_cli.disconnect,
            ["disconnect", "--json"],
            "internal_error",
            "Erro interno ao consultar/operar a VPN.",
        )


if __name__ == "__main__":
    unittest.main()
