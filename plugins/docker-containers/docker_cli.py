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

Esta subtarefa (Setup, T001) só declara o esqueleto — todas as funções lançam `NotImplementedError`.
"""

from __future__ import annotations


def find_binary() -> str | None:
    """Localiza o binário `docker` no `PATH` — implementado em T021 (US1)."""
    raise NotImplementedError("find_binary será implementado em T021 (US1)")


def list_containers() -> dict:
    """Lista os containers Docker — implementado em T021 (US1)."""
    raise NotImplementedError("list_containers será implementado em T021 (US1)")


def start(container_id: str) -> dict:
    """Inicia um container Docker — implementado em T028 (US2)."""
    raise NotImplementedError("start será implementado em T028 (US2)")


def stop(container_id: str) -> dict:
    """Para um container Docker — implementado em T028 (US2)."""
    raise NotImplementedError("stop será implementado em T028 (US2)")


def restart(container_id: str) -> dict:
    """Reinicia um container Docker — implementado em T028 (US2)."""
    raise NotImplementedError("restart será implementado em T028 (US2)")
