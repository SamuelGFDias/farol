"""Wrapper síncrono sobre o binário `docker` (subprocess) — traduz `ps`/`start`/`stop`/`restart`
para o vocabulário do protocolo Farol.

Implementado nas tasks T021 (US1) e T028 (US2) de specs/005-docker-containers-plugin/tasks.md.

Interface interna entre este módulo e `main.py` (decisão local deste plugin, não faz parte do
protocolo Farol, mesmo padrão de `plugins/openfortivpn-vpn/vpn_cli.py`): `list_containers()` nunca
lança para os casos de erro previstos pelo contrato
(`specs/005-docker-containers-plugin/contracts/docker-cli-mapping.md` § `widget/get`) — em vez
disso devolve um dict-marcador `{"error": {"code": <-32003|-32010>, "condition": <str>}}`, que
`main.py::handle_widget_get` reconhece pelo `code` e traduz para o envelope JSON-RPC de erro
apropriado (`condition` vira `data.detail.condition` no caso `-32010`). Em caso de sucesso, devolve
`{"items": [<ContainerStatusItem>, ...]}` (possivelmente vazia, FR-011), pronto para entrar em
`items` sem transformação adicional.
"""

from __future__ import annotations

import json
import shutil
import subprocess
import sys

LIST_TIMEOUT_SECONDS = 3
RECHECK_TIMEOUT_SECONDS = 3

# Timeout de subprocess por ação (contracts/docker-cli-mapping.md § action/invoke) — sempre abaixo
# do `timeout_hint_ms` declarado em `_build_action`, para que quem reporte um estouro seja este
# plugin (erro de domínio `-32011`/`timeout`), não o core sintetizando `-32002`/`action_timeout`.
ACTION_TIMEOUT_SECONDS = {"start": 15, "stop": 30, "restart": 40}

_ERROR_EXEC_UNAVAILABLE = -32003
_ERROR_DOCKER_UNAVAILABLE = -32010
_ERROR_CONTAINER_ACTION_FAILED = -32011

# Tradução PT-BR por `docker_condition` (`contracts/docker-cli-mapping.md` § action/invoke,
# `protocol/schema/v0.4/error.schema.json` catálogo `-32011`/`container_action_failed`).
_ACTION_ERROR_MESSAGES = {
    "no_such_container": (
        "O container não existe mais — ele pode ter sido removido enquanto a lista estava aberta."
    ),
    "container_gone": (
        "O container foi removido durante a operação — a ação pode ter sido executada, mas o "
        "estado final não pôde ser confirmado."
    ),
    "permission_denied": "Sem permissão para operar containers no daemon do Docker.",
    "daemon_unreachable": (
        "O daemon do Docker não está respondendo — a operação não foi executada."
    ),
    "timeout": (
        "A operação não terminou dentro do tempo esperado. O container pode ainda estar em "
        "transição."
    ),
    "cli_error": "O Docker recusou a operação.",
}

# Vocabulário publicado pelo Docker (data-model.md §1.2) — qualquer outro valor de `State` vira
# "unknown" (FR-012), sem invalidar as demais linhas.
_KNOWN_STATES = {"created", "restarting", "running", "removing", "paused", "exited", "dead"}

# Matriz normativa completa de FR-008 (repetida em data-model.md §1.3, invariante 5) — chave é o
# `state` já normalizado (inclusive "unknown").
_ENABLED_BY_STATE = {
    "created": {"start": True, "stop": False, "restart": True},
    "running": {"start": False, "stop": True, "restart": True},
    "restarting": {"start": False, "stop": True, "restart": True},
    "paused": {"start": False, "stop": True, "restart": True},
    "exited": {"start": True, "stop": False, "restart": True},
    "removing": {"start": False, "stop": False, "restart": False},
    "dead": {"start": False, "stop": False, "restart": False},
    "unknown": {"start": False, "stop": False, "restart": False},
}


def _error_marker(code: int, condition: str) -> dict:
    """Monta o dict-marcador de erro interno descrito no docstring do módulo."""
    return {"error": {"code": code, "condition": condition}}


def find_binary() -> str | None:
    """Localiza o binário `docker` no `PATH` — implementado em T021."""
    return shutil.which("docker")


def _classify_stderr(stderr: str) -> str:
    """Classifica o stderr de `docker ps` com falha (`contracts/docker-cli-mapping.md`).

    **A ordem dos testes é normativa** (research.md D5.1): `permission_denied` MUST ser testado
    antes de `daemon_unreachable` — um stderr real de "sem permissão" também casa, literalmente,
    com a ideia de "falha de conexão", e inverter a ordem classificaria toda falta de permissão
    como "daemon parado".
    """
    lowered = stderr.lower()
    if "permission denied" in lowered:
        return "permission_denied"
    if (
        "failed to connect" in lowered
        or "cannot connect" in lowered
        or "is the docker daemon running" in lowered
    ):
        return "daemon_unreachable"
    return "cli_error"


def _classify_action_stderr(stderr: str) -> str:
    """Classifica o stderr de `docker start|stop|restart`/releitura com falha
    (`contracts/docker-cli-mapping.md` § action/invoke).

    **A ordem dos testes é normativa** (mesma disciplina de `_classify_stderr`, com uma causa a
    mais na frente): `no_such_container` MUST ser testado antes de `permission_denied`, que MUST
    ser testado antes de `daemon_unreachable` — `no_such_container` é a falha esperada e frequente
    do edge case "container removido entre a exibição da lista e o clique", e merece a mensagem
    mais específica antes de qualquer outra classificação genérica.
    """
    lowered = stderr.lower()
    if "no such container" in lowered:
        return "no_such_container"
    if "permission denied" in lowered:
        return "permission_denied"
    if (
        "failed to connect" in lowered
        or "cannot connect" in lowered
        or "is the docker daemon running" in lowered
    ):
        return "daemon_unreachable"
    return "cli_error"


def _action_error_marker(condition: str) -> dict:
    """Monta o dict-marcador de erro `-32011`/`container_action_failed` para `start`/`stop`/
    `restart`. `message` é sempre a tradução PT-BR fixa por `condition`
    (`contracts/docker-cli-mapping.md` § action/invoke)."""
    return {
        "error": {
            "code": _ERROR_CONTAINER_ACTION_FAILED,
            "condition": condition,
            "message": _ACTION_ERROR_MESSAGES.get(condition, _ACTION_ERROR_MESSAGES["cli_error"]),
        }
    }


def _build_action(
    action_id: str, label: str, timeout_hint_ms: int, container_id: str, enabled: bool
) -> dict:
    """Monta uma `ActionDeclaration` (data-model.md §1.3/invariantes 3-4).

    `label` é obrigatório no schema do protocolo (`handshake.schema.json`
    `$defs.ActionDeclaration.required` / `farol_protocol::messages::ActionDeclaration::label`,
    `String` não-opcional) — mesmo padrão de `plugins/openfortivpn-vpn/vpn_cli.py`.
    """
    return {
        "id": action_id,
        "label": label,
        "target": {"type": "docker-container", "id": container_id},
        "timeout_hint_ms": timeout_hint_ms,
        "enabled": enabled,
    }


def _build_container_status_item(row: dict) -> dict:
    """Mapeia uma linha (já parseada) de `docker ps --format '{{json .}}'` para
    `ContainerStatusItem` (`contracts/docker-cli-mapping.md` § Mapeamento de campos)."""
    container_id = row.get("ID", "")
    names = row.get("Names", "") or ""
    name = names.split(",")[0] if names else names
    image = row.get("Image", "") or ""
    raw_state = row.get("State", "") or ""
    state = raw_state if raw_state in _KNOWN_STATES else "unknown"
    status_text = row.get("Status") or None

    enabled = _ENABLED_BY_STATE[state]

    return {
        "id": container_id,
        "name": name,
        "image": image,
        "state": state,
        "status_text": status_text,
        "start_action": _build_action(
            "docker.container.start", "Iniciar", 20000, container_id, enabled["start"]
        ),
        "stop_action": _build_action(
            "docker.container.stop", "Parar", 35000, container_id, enabled["stop"]
        ),
        "restart_action": _build_action(
            "docker.container.restart", "Reiniciar", 45000, container_id, enabled["restart"]
        ),
    }


def list_containers() -> dict:
    """Lista os containers Docker — implementado em T021.

    Ver docstring do módulo para a forma dos dois tipos de retorno possíveis (sucesso vs.
    dict-marcador de erro). Lista vazia é sucesso normal, `{"items": []}`, nunca erro (FR-011).
    """
    binary = find_binary()
    if binary is None:
        return _error_marker(_ERROR_EXEC_UNAVAILABLE, "exec_unavailable")

    try:
        result = subprocess.run(
            ["docker", "ps", "--all", "--no-trunc", "--format", "{{json .}}"],
            capture_output=True,
            text=True,
            timeout=LIST_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        return _error_marker(_ERROR_DOCKER_UNAVAILABLE, "timeout")
    except OSError:
        return _error_marker(_ERROR_DOCKER_UNAVAILABLE, "cli_error")

    if result.returncode != 0:
        condition = _classify_stderr(result.stderr or "")
        return _error_marker(_ERROR_DOCKER_UNAVAILABLE, condition)

    items = []
    for line in (result.stdout or "").splitlines():
        line = line.strip()
        if not line:
            continue
        try:
            row = json.loads(line)
        except json.JSONDecodeError as exc:
            print(
                f"[docker-containers] linha de 'docker ps' não é JSON válido, ignorada: {exc!r}",
                file=sys.stderr,
            )
            continue
        if not isinstance(row, dict):
            print(
                f"[docker-containers] linha de 'docker ps' não é um objeto JSON, ignorada: {row!r}",
                file=sys.stderr,
            )
            continue
        items.append(_build_container_status_item(row))

    items.sort(key=lambda item: (item["name"], item["id"]))

    return {"items": items}


def _reread_container(container_id: str) -> dict:
    """Releitura pontual de um único container após uma ação bem-sucedida (`contracts/
    docker-cli-mapping.md` § action/invoke, passo 3) — nunca a lista inteira.

    Devolve `{"container": <ContainerStatusItem>}` em sucesso (com `enabled` recalculado a partir
    do novo `state`, via `_build_container_status_item`/`_ENABLED_BY_STATE`); dict-marcador de erro
    `-32011` caso contrário — `condition: "container_gone"` quando a releitura vem vazia (container
    removido no intervalo), ou classificado por `_classify_action_stderr` em caso de falha da
    própria releitura.
    """
    try:
        result = subprocess.run(
            [
                "docker",
                "ps",
                "--all",
                "--no-trunc",
                "--filter",
                f"id={container_id}",
                "--format",
                "{{json .}}",
            ],
            capture_output=True,
            text=True,
            timeout=RECHECK_TIMEOUT_SECONDS,
        )
    except subprocess.TimeoutExpired:
        return _action_error_marker("timeout")
    except OSError:
        return _action_error_marker("cli_error")

    if result.returncode != 0:
        return _action_error_marker(_classify_action_stderr(result.stderr or ""))

    lines = [line.strip() for line in (result.stdout or "").splitlines() if line.strip()]
    if not lines:
        return _action_error_marker("container_gone")

    try:
        row = json.loads(lines[0])
    except json.JSONDecodeError:
        return _action_error_marker("cli_error")

    if not isinstance(row, dict):
        return _action_error_marker("cli_error")

    return {"container": _build_container_status_item(row)}


def _run_action(command: str, container_id: str) -> dict:
    """Executa `docker start|stop|restart <id>` e, em sucesso, faz a releitura pontual do
    container (`contracts/docker-cli-mapping.md` § action/invoke). Nunca passa `--time` — a
    política de desligamento do container é do usuário, o Farol só reporta o Docker sem alterá-la
    (FR-015, `research.md` D6)."""
    binary = find_binary()
    if binary is None:
        return _error_marker(_ERROR_EXEC_UNAVAILABLE, "exec_unavailable")

    try:
        result = subprocess.run(
            ["docker", command, container_id],
            capture_output=True,
            text=True,
            timeout=ACTION_TIMEOUT_SECONDS[command],
        )
    except subprocess.TimeoutExpired:
        return _action_error_marker("timeout")
    except OSError:
        return _action_error_marker("cli_error")

    if result.returncode != 0:
        return _action_error_marker(_classify_action_stderr(result.stderr or ""))

    return _reread_container(container_id)


def start(container_id: str) -> dict:
    """Inicia um container Docker — implementado em T028."""
    return _run_action("start", container_id)


def stop(container_id: str) -> dict:
    """Para um container Docker — implementado em T028."""
    return _run_action("stop", container_id)


def restart(container_id: str) -> dict:
    """Reinicia um container Docker — implementado em T028."""
    return _run_action("restart", container_id)
