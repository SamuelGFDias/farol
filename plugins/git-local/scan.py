"""Varredura de repositórios git sob um `scan_root` e execução da ação `git.fetch`.

Implementa a lógica normativa de
`specs/001-walking-skeleton-git-plugin/contracts/git-local-plugin.md` (§ Varredura, § Ação de
fetch) e a forma de dado de `protocol/schema/v0.1/widget.schema.json`
(`GitRepository`/`RemoteStatus`/`WidgetItem`) e `handshake.schema.json` (`ActionDeclaration`,
campos em snake_case). Produz dicionários já no formato JSON exato esperado por essas mensagens —
`main.py` só embrulha o resultado na envelope JSON-RPC.

Apenas biblioteca padrão (`subprocess`, `pathlib`, `shutil`) — decisão D3 de `research.md`.
"""

from __future__ import annotations

import shutil
import subprocess
from pathlib import Path

# Mesmo `action_id`/`label` para todo repositório — o que diferencia cada instância de ação é
# `target.id` (o caminho do repositório), não o `id` da ação em si (`contracts/git-local-plugin.md`
# § Ação de fetch).
ACTION_ID = "git.fetch"
ACTION_LABEL = "Fetch"


class ExecUnavailableError(Exception):
    """Binário `git` não disponível no sistema — corresponde a `-32003 exec_unavailable`."""


class ScanRootUnreadableError(Exception):
    """`scan_root` existe mas não pôde ser listado por permissão — `-32004 scan_root_unreadable`.

    Distinto de "não existe" ou "vazio", que MUST ser sucesso com `items: []` (nunca erro).
    """


class FetchFailedError(Exception):
    """`git fetch` retornou código de saída não-zero — corresponde a `-32001 fetch_failed`."""

    def __init__(self, detail: str) -> None:
        super().__init__(detail)
        self.detail = detail


def git_available() -> bool:
    """`True` se o binário `git` estiver disponível no `PATH` deste processo."""
    return shutil.which("git") is not None


def _run_git(args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    """Executa `git <args>` em `cwd`, capturando stdout/stderr como texto (nunca lança por
    código de saída não-zero — quem chama decide o que fazer com `returncode`)."""
    return subprocess.run(
        ["git", *args],
        cwd=str(cwd),
        capture_output=True,
        text=True,
        check=False,
    )


def _is_dirty(repo_path: Path) -> bool:
    """Equivalente a `git status --porcelain` não-vazio (`contracts/git-local-plugin.md`)."""
    result = _run_git(["status", "--porcelain"], repo_path)
    return bool(result.stdout.strip())


def _remote_names(repo_path: Path) -> list[str]:
    """Lista de remotes configurados (`git remote`) — vazia ⟺ repositório sem remoto."""
    result = _run_git(["remote"], repo_path)
    return [line for line in result.stdout.splitlines() if line.strip()]


def _ahead_behind(repo_path: Path) -> tuple[int, int]:
    """`(ahead, behind)` do branch atual em relação ao upstream configurado.

    Equivalente a `git rev-list --left-right --count <upstream>...HEAD`
    (`contracts/git-local-plugin.md`): a contagem à esquerda é "commits só alcançáveis a partir do
    upstream" (= `behind`), a contagem à direita é "commits só alcançáveis a partir de HEAD"
    (= `ahead`).

    Caso de borda não coberto explicitamente pelo contrato: um remote pode estar configurado
    (`git remote` não-vazio) sem que o branch atual tenha upstream de tracking definido (`@{u}`
    falha). Isso não é o estado "no_remote" que o contrato define — o fallback aqui é `(0, 0)` em
    vez de propagar um erro, mantendo o widget utilizável mesmo nesse caso de borda.
    """
    result = _run_git(["rev-list", "--left-right", "--count", "@{u}...HEAD"], repo_path)
    if result.returncode != 0:
        return (0, 0)

    parts = result.stdout.split()
    if len(parts) != 2:
        return (0, 0)

    behind_str, ahead_str = parts
    try:
        return (int(ahead_str), int(behind_str))
    except ValueError:
        return (0, 0)


def _remote_status(repo_path: Path) -> dict:
    """Monta o `RemoteStatus` (`{"kind":"no_remote"}` ou `{"kind":"tracked","ahead":N,"behind":M}`)."""
    if not _remote_names(repo_path):
        return {"kind": "no_remote"}
    ahead, behind = _ahead_behind(repo_path)
    return {"kind": "tracked", "ahead": ahead, "behind": behind}


def build_repo_item(repo_path: Path) -> dict:
    """Monta um `WidgetItem` (`repo` + `fetch_action`) para um repositório já identificado.

    Invariante MUST (`contracts/widget-protocol.md`): `remote_status.kind == "no_remote"` ⟺
    `fetch_action.enabled == False`.
    """
    repo_id = str(repo_path.resolve())
    remote_status = _remote_status(repo_path)
    enabled = remote_status["kind"] == "tracked"

    repo = {
        "id": repo_id,
        "name": repo_path.name,
        "path": repo_id,
        "dirty": _is_dirty(repo_path),
        "remote_status": remote_status,
    }
    fetch_action = {
        "id": ACTION_ID,
        "label": ACTION_LABEL,
        "target": {"type": "repo", "id": repo_id},
        "enabled": enabled,
    }
    return {"repo": repo, "fetch_action": fetch_action}


def find_repo_dirs(scan_root: Path) -> list[Path]:
    """Subdiretórios DIRETOS de `scan_root` contendo `.git` (um nível, não recursivo).

    `scan_root` inexistente ou vazio MUST ser sucesso silencioso (lista vazia) — nunca erro (Edge
    Case da spec). Falha de permissão ao listar o próprio `scan_root` propaga
    `ScanRootUnreadableError`.
    """
    if not scan_root.is_dir():
        return []

    try:
        entries = sorted(scan_root.iterdir())
    except PermissionError as exc:
        raise ScanRootUnreadableError(str(scan_root)) from exc

    repo_dirs = []
    for entry in entries:
        try:
            if entry.is_dir() and (entry / ".git").exists():
                repo_dirs.append(entry)
        except PermissionError:
            # Uma entrada individual inacessível não derruba a varredura inteira — só o próprio
            # scan_root inacessível é -32004 (contrato só fala do diretório raiz).
            continue
    return repo_dirs


def scan_repositories(scan_root: Path) -> list[dict]:
    """Varre `scan_root` e devolve a lista de `WidgetItem` prontos para `widget/get` (FR-013/FR-014).

    Levanta `ExecUnavailableError` quando há repositórios a inspecionar mas o binário `git` não
    está disponível (T045) — quem chama (main.py) converte isso em erro `-32003` pontual, sem
    derrubar o processo do plugin.
    """
    repo_dirs = find_repo_dirs(scan_root)
    if not repo_dirs:
        return []

    if not git_available():
        raise ExecUnavailableError("git")

    return [build_repo_item(path) for path in repo_dirs]


def fetch(repo_path: Path) -> dict:
    """Executa `git fetch` em `repo_path`; devolve o `GitRepository` pós-fetch em sucesso.

    Levanta `ExecUnavailableError` se `git` não estiver disponível, ou
    `FetchFailedError(detail)` se o subprocess sair com código != 0 (FR-017) — nenhuma das duas
    propaga além do chamador (`main.py`), que as converte em resposta de erro estruturada sem
    encerrar o processo do plugin.
    """
    if not git_available():
        raise ExecUnavailableError("git")

    result = _run_git(["fetch"], repo_path)
    if result.returncode != 0:
        detail = (result.stderr or result.stdout or "git fetch falhou sem detalhe").strip()
        raise FetchFailedError(detail)

    return build_repo_item(repo_path)["repo"]
