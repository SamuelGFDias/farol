"""Testes de `docker_cli.list_containers` — só stdlib (`unittest`), sem framework externo.

Rodar com: `python3 test_docker_cli.py` (ou `python3 -m unittest test_docker_cli`).
"""

from __future__ import annotations

import json
import subprocess
import unittest
from unittest.mock import patch

import docker_cli


def _completed(stdout: str, returncode: int = 0, stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["docker", "ps", "--all", "--no-trunc", "--format", "{{json .}}"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def _row(
    container_id: str,
    name: str,
    state: str,
    image: str = "nginx:latest",
    status: str = "Up 2 hours",
) -> dict:
    return {
        "ID": container_id,
        "Names": name,
        "Image": image,
        "State": state,
        "Status": status,
    }


def _ndjson(rows: list[dict]) -> str:
    return "\n".join(json.dumps(row) for row in rows) + "\n"


class ListContainersBinaryMissingTests(unittest.TestCase):
    @patch("docker_cli.shutil.which", return_value=None)
    def test_binary_missing_returns_exec_unavailable_marker(self, _mock_which) -> None:
        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32003)
        self.assertEqual(result["error"]["condition"], "exec_unavailable")


class ListContainersSuccessTests(unittest.TestCase):
    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_multi_state_list_computes_enabled_matrix_per_fr008(
        self, mock_run, _mock_which
    ) -> None:
        """(1) Cada estado da matriz normativa de FR-008 produz o `enabled` correto por linha."""
        rows = [
            _row("a" * 64, "criado", "created"),
            _row("b" * 64, "rodando", "running"),
            _row("c" * 64, "reiniciando", "restarting"),
            _row("d" * 64, "pausado", "paused"),
            _row("e" * 64, "parado", "exited"),
            _row("f" * 64, "removendo", "removing"),
            _row("0" * 64, "morto", "dead"),
        ]
        mock_run.return_value = _completed(_ndjson(rows))

        result = docker_cli.list_containers()

        self.assertNotIn("error", result)
        by_state = {item["state"]: item for item in result["items"]}

        expected = {
            "created": (True, False, True),
            "running": (False, True, True),
            "restarting": (False, True, True),
            "paused": (False, True, True),
            "exited": (True, False, True),
            "removing": (False, False, False),
            "dead": (False, False, False),
        }
        for state, (start_enabled, stop_enabled, restart_enabled) in expected.items():
            item = by_state[state]
            self.assertEqual(
                item["start_action"]["enabled"], start_enabled, f"start_action de {state}"
            )
            self.assertEqual(
                item["stop_action"]["enabled"], stop_enabled, f"stop_action de {state}"
            )
            self.assertEqual(
                item["restart_action"]["enabled"], restart_enabled, f"restart_action de {state}"
            )
            self.assertEqual(item["start_action"]["id"], "docker.container.start")
            self.assertEqual(item["stop_action"]["id"], "docker.container.stop")
            self.assertEqual(item["restart_action"]["id"], "docker.container.restart")
            self.assertEqual(item["start_action"]["timeout_hint_ms"], 20000)
            self.assertEqual(item["stop_action"]["timeout_hint_ms"], 35000)
            self.assertEqual(item["restart_action"]["timeout_hint_ms"], 45000)
            for action in ("start_action", "stop_action", "restart_action"):
                self.assertEqual(
                    item[action]["target"], {"type": "docker-container", "id": item["id"]}
                )

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_empty_list_is_success_not_error(self, mock_run, _mock_which) -> None:
        """(2) `docker ps` sem containers é sucesso normal com `items: []` (FR-011)."""
        mock_run.return_value = _completed("")

        result = docker_cli.list_containers()

        self.assertNotIn("error", result)
        self.assertEqual(result["items"], [])

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_unknown_state_maps_to_unknown_without_dropping_other_lines(
        self, mock_run, _mock_which
    ) -> None:
        """(3) Estado fora do vocabulário vira "unknown" sem derrubar as demais linhas."""
        rows = [
            _row("a" * 64, "normal", "running"),
            _row("b" * 64, "esquisito", "algum-estado-novo-do-docker"),
        ]
        mock_run.return_value = _completed(_ndjson(rows))

        result = docker_cli.list_containers()

        self.assertNotIn("error", result)
        self.assertEqual(len(result["items"]), 2)
        by_name = {item["name"]: item for item in result["items"]}
        self.assertEqual(by_name["esquisito"]["state"], "unknown")
        self.assertFalse(by_name["esquisito"]["start_action"]["enabled"])
        self.assertFalse(by_name["esquisito"]["stop_action"]["enabled"])
        self.assertFalse(by_name["esquisito"]["restart_action"]["enabled"])
        self.assertEqual(by_name["normal"]["state"], "running")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_result_is_sorted_by_name_then_id_regardless_of_cli_order(
        self, mock_run, _mock_which
    ) -> None:
        """(4) Ordenação estável por (name, id), mesmo com entrada embaralhada."""
        rows = [
            _row("c" * 64, "zebra", "running"),
            _row("a" * 64, "abacate", "running"),
            _row("b" * 64, "abacate", "running"),
            _row("d" * 64, "melancia", "running"),
        ]
        mock_run.return_value = _completed(_ndjson(rows))

        result = docker_cli.list_containers()

        self.assertNotIn("error", result)
        got = [(item["name"], item["id"]) for item in result["items"]]
        self.assertEqual(
            got,
            [
                ("abacate", "a" * 64),
                ("abacate", "b" * 64),
                ("melancia", "d" * 64),
                ("zebra", "c" * 64),
            ],
        )

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_one_unparseable_ndjson_line_does_not_drop_the_others(
        self, mock_run, _mock_which
    ) -> None:
        """(5) Uma linha NDJSON não parseável no meio de linhas válidas não derruba as demais."""
        valid_row_1 = json.dumps(_row("a" * 64, "primeiro", "running"))
        valid_row_2 = json.dumps(_row("b" * 64, "segundo", "exited"))
        stdout = f"{valid_row_1}\nisto não é JSON\n{valid_row_2}\n"
        mock_run.return_value = _completed(stdout)

        result = docker_cli.list_containers()

        self.assertNotIn("error", result)
        names = sorted(item["name"] for item in result["items"])
        self.assertEqual(names, ["primeiro", "segundo"])


class ListContainersFailureClassificationTests(unittest.TestCase):
    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_permission_denied_stderr_classified_as_permission_denied(
        self, mock_run, _mock_which
    ) -> None:
        """(6a) stderr de permissão negada vira `condition: "permission_denied"`."""
        mock_run.return_value = _completed(
            "",
            returncode=1,
            stderr="permission denied while trying to connect to the docker API at unix:///var/run/docker.sock",
        )

        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32010)
        self.assertEqual(result["error"]["condition"], "permission_denied")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_daemon_unreachable_stderr_classified_as_daemon_unreachable(
        self, mock_run, _mock_which
    ) -> None:
        """(6b) stderr de daemon fora do ar vira `condition: "daemon_unreachable"`."""
        mock_run.return_value = _completed(
            "",
            returncode=1,
            stderr=(
                "failed to connect to the docker API at unix:///var/run/docker.sock; check if "
                "the path is correct and if the daemon is running"
            ),
        )

        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32010)
        self.assertEqual(result["error"]["condition"], "daemon_unreachable")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_unexpected_stderr_classified_as_cli_error(self, mock_run, _mock_which) -> None:
        """(6c) Qualquer outra saída de erro vira `condition: "cli_error"`."""
        mock_run.return_value = _completed(
            "", returncode=1, stderr="algo totalmente inesperado aconteceu"
        )

        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32010)
        self.assertEqual(result["error"]["condition"], "cli_error")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_timeout_classified_as_timeout(self, mock_run, _mock_which) -> None:
        """(6d) Timeout do subprocess vira `condition: "timeout"`."""
        mock_run.side_effect = subprocess.TimeoutExpired(
            cmd=["docker", "ps", "--all", "--no-trunc", "--format", "{{json .}}"],
            timeout=docker_cli.LIST_TIMEOUT_SECONDS,
        )

        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32010)
        self.assertEqual(result["error"]["condition"], "timeout")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_stderr_containing_both_permission_denied_and_connection_failure_is_permission_denied(
        self, mock_run, _mock_which
    ) -> None:
        """(7) O teste mais importante desta task (research.md D5.1): um stderr que contém tanto
        "permission denied" quanto um texto de falha de conexão MUST classificar como
        `permission_denied`, não `daemon_unreachable` — a ordem de teste é normativa."""
        mock_run.return_value = _completed(
            "",
            returncode=1,
            stderr=(
                "permission denied while trying to connect to the docker API at "
                "unix:///var/run/docker.sock: failed to connect to the docker daemon, "
                "is the docker daemon running?"
            ),
        )

        result = docker_cli.list_containers()

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32010)
        self.assertEqual(result["error"]["condition"], "permission_denied")


def _recheck_completed(
    stdout: str, returncode: int = 0, stderr: str = ""
) -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["docker", "ps", "--all", "--no-trunc", "--filter", "id=x", "--format", "{{json .}}"],
        returncode=returncode,
        stdout=stdout,
        stderr=stderr,
    )


def _action_completed(returncode: int = 0, stderr: str = "") -> subprocess.CompletedProcess:
    return subprocess.CompletedProcess(
        args=["docker"], returncode=returncode, stdout="", stderr=stderr
    )


class ActionSuccessTests(unittest.TestCase):
    """(1) `start`/`stop`/`restart` bem-sucedidos: releitura pontual devolve o
    `ContainerStatusItem` inteiro com `enabled` recalculado para o novo estado (D11)."""

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_start_success_rereads_and_recomputes_enabled_for_new_state(
        self, mock_run, _mock_which
    ) -> None:
        container_id = "a" * 64
        row = _row(container_id, "app", "running")
        mock_run.side_effect = [
            _action_completed(returncode=0),
            _recheck_completed(_ndjson([row])),
        ]

        result = docker_cli.start(container_id)

        self.assertNotIn("error", result)
        container = result["container"]
        self.assertEqual(container["state"], "running")
        self.assertFalse(container["start_action"]["enabled"])
        self.assertTrue(container["stop_action"]["enabled"])
        self.assertTrue(container["restart_action"]["enabled"])

        first_call_args = mock_run.call_args_list[0].args[0]
        self.assertEqual(first_call_args, ["docker", "start", container_id])
        self.assertEqual(mock_run.call_args_list[0].kwargs["timeout"], 15)
        self.assertNotIn("--time", first_call_args)

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_stop_success_rereads_and_recomputes_enabled_for_new_state(
        self, mock_run, _mock_which
    ) -> None:
        container_id = "b" * 64
        row = _row(container_id, "app", "exited")
        mock_run.side_effect = [
            _action_completed(returncode=0),
            _recheck_completed(_ndjson([row])),
        ]

        result = docker_cli.stop(container_id)

        self.assertNotIn("error", result)
        container = result["container"]
        self.assertEqual(container["state"], "exited")
        self.assertTrue(container["start_action"]["enabled"])
        self.assertFalse(container["stop_action"]["enabled"])
        self.assertTrue(container["restart_action"]["enabled"])

        first_call_args = mock_run.call_args_list[0].args[0]
        self.assertEqual(first_call_args, ["docker", "stop", container_id])
        self.assertEqual(mock_run.call_args_list[0].kwargs["timeout"], 30)
        self.assertNotIn("--time", first_call_args)

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_restart_success_rereads_and_recomputes_enabled_for_new_state(
        self, mock_run, _mock_which
    ) -> None:
        container_id = "c" * 64
        row = _row(container_id, "app", "running")
        mock_run.side_effect = [
            _action_completed(returncode=0),
            _recheck_completed(_ndjson([row])),
        ]

        result = docker_cli.restart(container_id)

        self.assertNotIn("error", result)
        container = result["container"]
        self.assertEqual(container["state"], "running")
        self.assertTrue(container["stop_action"]["enabled"])

        first_call_args = mock_run.call_args_list[0].args[0]
        self.assertEqual(first_call_args, ["docker", "restart", container_id])
        self.assertEqual(mock_run.call_args_list[0].kwargs["timeout"], 40)
        self.assertNotIn("--time", first_call_args)


class ActionRecheckEmptyTests(unittest.TestCase):
    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_recheck_returning_no_lines_is_container_gone(self, mock_run, _mock_which) -> None:
        """(2) A operação teve sucesso, mas a releitura veio vazia — o container foi removido no
        intervalo (`docker_condition: "container_gone"`)."""
        container_id = "d" * 64
        mock_run.side_effect = [
            _action_completed(returncode=0),
            _recheck_completed(""),
        ]

        result = docker_cli.stop(container_id)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "container_gone")
        self.assertEqual(
            result["error"]["message"], docker_cli._ACTION_ERROR_MESSAGES["container_gone"]
        )


class ActionFailureClassificationTests(unittest.TestCase):
    """(3) As quatro condições de falha, na ordem normativa, mais `timeout` — cada uma com a
    mensagem PT-BR exata da tabela de `contracts/docker-cli-mapping.md`."""

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_no_such_container_stderr_classified_first(self, mock_run, _mock_which) -> None:
        mock_run.return_value = _action_completed(
            returncode=1,
            stderr="Error response from daemon: No such container: abc123",
        )

        result = docker_cli.start("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "no_such_container")
        self.assertEqual(
            result["error"]["message"],
            "O container não existe mais — ele pode ter sido removido enquanto a lista estava "
            "aberta.",
        )

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_permission_denied_stderr_classified_second(self, mock_run, _mock_which) -> None:
        mock_run.return_value = _action_completed(
            returncode=1,
            stderr="permission denied while trying to connect to the docker API",
        )

        result = docker_cli.stop("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "permission_denied")
        self.assertEqual(
            result["error"]["message"],
            "Sem permissão para operar containers no daemon do Docker.",
        )

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_daemon_unreachable_stderr_classified_third(self, mock_run, _mock_which) -> None:
        mock_run.return_value = _action_completed(
            returncode=1,
            stderr=(
                "failed to connect to the docker API at unix:///var/run/docker.sock; check if "
                "the path is correct and if the daemon is running"
            ),
        )

        result = docker_cli.restart("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "daemon_unreachable")
        self.assertEqual(
            result["error"]["message"],
            "O daemon do Docker não está respondendo — a operação não foi executada.",
        )

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_unexpected_stderr_classified_as_cli_error(self, mock_run, _mock_which) -> None:
        mock_run.return_value = _action_completed(
            returncode=1, stderr="algo totalmente inesperado aconteceu"
        )

        result = docker_cli.start("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "cli_error")
        self.assertEqual(result["error"]["message"], "O Docker recusou a operação.")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_no_such_container_stderr_wins_over_permission_denied_when_both_present(
        self, mock_run, _mock_which
    ) -> None:
        """A ordem normativa também vale quando o stderr contém `no such container` E `permission
        denied` simultaneamente — `no_such_container` deve prevalecer (é testado primeiro)."""
        mock_run.return_value = _action_completed(
            returncode=1,
            stderr=(
                "No such container: abc123: permission denied while trying to connect to the "
                "docker API"
            ),
        )

        result = docker_cli.start("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["condition"], "no_such_container")

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_timeout_classified_as_timeout(self, mock_run, _mock_which) -> None:
        mock_run.side_effect = subprocess.TimeoutExpired(
            cmd=["docker", "stop", "a" * 64], timeout=docker_cli.ACTION_TIMEOUT_SECONDS["stop"]
        )

        result = docker_cli.stop("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32011)
        self.assertEqual(result["error"]["condition"], "timeout")
        self.assertEqual(
            result["error"]["message"],
            "A operação não terminou dentro do tempo esperado. O container pode ainda estar em "
            "transição.",
        )

    @patch("docker_cli.shutil.which", return_value=None)
    def test_binary_missing_returns_exec_unavailable_marker(self, _mock_which) -> None:
        result = docker_cli.start("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["code"], -32003)
        self.assertEqual(result["error"]["condition"], "exec_unavailable")


class ActionRecheckFailureTests(unittest.TestCase):
    """A ação em si tem sucesso, mas a releitura pontual subsequente falha (subprocess) — o
    resultado ainda deve virar `-32011` classificado, não deixar a exceção escapar."""

    @patch("docker_cli.shutil.which", return_value="/usr/bin/docker")
    @patch("docker_cli.subprocess.run")
    def test_recheck_timeout_after_successful_action_is_reported_as_timeout(
        self, mock_run, _mock_which
    ) -> None:
        mock_run.side_effect = [
            _action_completed(returncode=0),
            subprocess.TimeoutExpired(
                cmd=["docker", "ps"], timeout=docker_cli.RECHECK_TIMEOUT_SECONDS
            ),
        ]

        result = docker_cli.start("a" * 64)

        self.assertIn("error", result)
        self.assertEqual(result["error"]["condition"], "timeout")


if __name__ == "__main__":
    unittest.main()
