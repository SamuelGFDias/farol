"""Testes unitários do plugin de referência `git-local` (T049).

Escolha de framework: `unittest` (biblioteca padrão), não `pytest`. `tasks.md` (T049) menciona
pytest como exemplo, mas D3 de `research.md` fixa "apenas biblioteca padrão" para toda a
implementação deste plugin — manter a suíte de testes também em `unittest`, sem `pip install
pytest`, é a escolha consistente com essa decisão (D3 é sobre o plugin como um todo, e reabrir uma
dependência externa só para os testes contradiria o espírito dela mesmo que não contradiga a letra
- "sem pip install de nada" no escopo desta tarefa).

Cria repositórios git *reais* em diretórios temporários via `subprocess` (incluindo um remote
"bare" local para exercitar `git fetch` de verdade) em vez de usar mocks de subprocess sempre que
possível — mais fiel ao que o core efetivamente observa ao falar com o processo do plugin.
"""

from __future__ import annotations

import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

PLUGIN_DIR = Path(__file__).resolve().parents[2] / "plugins" / "git-local"
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

import scan  # noqa: E402 — import depende do sys.path.insert acima


def _run(args: list[str], cwd: Path) -> subprocess.CompletedProcess[str]:
    return subprocess.run(args, cwd=str(cwd), check=True, capture_output=True, text=True)


def _init_repo_with_commit(path: Path, *, name: str) -> None:
    """Cria um repositório git standalone (sem remote) com um commit inicial."""
    path.mkdir(parents=True)
    _run(["git", "init", "--initial-branch=main"], path)
    _run(["git", "config", "user.email", "test@example.com"], path)
    _run(["git", "config", "user.name", "Test"], path)
    (path / "README.md").write_text(f"# {name}\n")
    _run(["git", "add", "README.md"], path)
    _run(["git", "commit", "-m", "initial"], path)


def _clone_with_commit(bare_remote: Path, dest: Path, *, filename: str, content: str) -> None:
    """Clona `bare_remote` em `dest`, cria um commit e faz push com upstream configurado
    (`git push -u`) — necessário para que `@{u}` resolva depois, exercitando o mesmo caminho que
    um clone humano normal deixaria configurado."""
    _run(["git", "clone", str(bare_remote), str(dest)], dest.parent)
    # Garante o nome do branch independentemente do default de git configurado no ambiente.
    _run(["git", "symbolic-ref", "HEAD", "refs/heads/main"], dest)
    _run(["git", "config", "user.email", "test@example.com"], dest)
    _run(["git", "config", "user.name", "Test"], dest)
    (dest / filename).write_text(content)
    _run(["git", "add", filename], dest)
    _run(["git", "commit", "-m", f"commit de {filename}"], dest)
    _run(["git", "push", "-u", "origin", "HEAD"], dest)


class FindRepoDirsAndScanTests(unittest.TestCase):
    """Varredura de `scan_root`: casos de borda + repositório sem remote (FR-013/FR-014)."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.scan_root = Path(self._tmp.name)

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_missing_scan_root_is_success_with_empty_list(self) -> None:
        missing = self.scan_root / "does-not-exist"
        self.assertEqual(scan.scan_repositories(missing), [])

    def test_empty_scan_root_is_success_with_empty_list(self) -> None:
        self.assertEqual(scan.scan_repositories(self.scan_root), [])

    def test_non_git_subdirectory_is_ignored(self) -> None:
        (self.scan_root / "not-a-repo").mkdir()
        self.assertEqual(scan.scan_repositories(self.scan_root), [])

    def test_nested_git_repo_is_ignored_only_direct_children_scanned(self) -> None:
        # Um repositório git dois níveis abaixo de scan_root MUST NOT aparecer - a varredura é de
        # um nível só (subdiretórios diretos), não recursiva em profundidade arbitrária.
        nested = self.scan_root / "group" / "nested-repo"
        _init_repo_with_commit(nested, name="nested-repo")
        self.assertEqual(scan.scan_repositories(self.scan_root), [])

    def test_repo_without_remote_reports_no_remote_and_disabled_fetch_action(self) -> None:
        repo_path = self.scan_root / "solo"
        _init_repo_with_commit(repo_path, name="solo")

        items = scan.scan_repositories(self.scan_root)

        self.assertEqual(len(items), 1)
        item = items[0]
        self.assertEqual(item["repo"]["name"], "solo")
        self.assertEqual(item["repo"]["remote_status"], {"kind": "no_remote"})
        self.assertFalse(item["fetch_action"]["enabled"])
        self.assertEqual(item["fetch_action"]["id"], "git.fetch")
        self.assertEqual(
            item["fetch_action"]["target"],
            {"type": "repo", "id": item["repo"]["id"]},
        )

    def test_dirty_repo_is_reported_as_dirty(self) -> None:
        repo_path = self.scan_root / "dirty"
        _init_repo_with_commit(repo_path, name="dirty")
        (repo_path / "untracked.txt").write_text("mudanca pendente\n")

        items = scan.scan_repositories(self.scan_root)

        self.assertEqual(len(items), 1)
        self.assertTrue(items[0]["repo"]["dirty"])

    def test_clean_repo_is_reported_as_not_dirty(self) -> None:
        repo_path = self.scan_root / "clean"
        _init_repo_with_commit(repo_path, name="clean")

        items = scan.scan_repositories(self.scan_root)

        self.assertEqual(len(items), 1)
        self.assertFalse(items[0]["repo"]["dirty"])

    def test_repo_with_remote_reports_tracked_status_and_enabled_fetch_action(self) -> None:
        bare_remote = self.scan_root / "remote.git"
        bare_remote.mkdir()
        _run(["git", "init", "--bare"], bare_remote)

        repo_path = self.scan_root / "cloned"
        _clone_with_commit(bare_remote, repo_path, filename="README.md", content="# cloned\n")

        items = scan.scan_repositories(self.scan_root)

        self.assertEqual(len(items), 1)
        item = items[0]
        self.assertEqual(item["repo"]["remote_status"], {"kind": "tracked", "ahead": 0, "behind": 0})
        self.assertTrue(item["fetch_action"]["enabled"])

    def test_multiple_repositories_are_all_reported(self) -> None:
        _init_repo_with_commit(self.scan_root / "repo-a", name="repo-a")
        _init_repo_with_commit(self.scan_root / "repo-b", name="repo-b")

        items = scan.scan_repositories(self.scan_root)

        names = sorted(item["repo"]["name"] for item in items)
        self.assertEqual(names, ["repo-a", "repo-b"])


class FetchActionTests(unittest.TestCase):
    """`git.fetch` real contra um remote "bare" local (T034)."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_path = Path(self._tmp.name)

        self.bare_remote = self.tmp_path / "remote.git"
        self.bare_remote.mkdir()
        _run(["git", "init", "--bare"], self.bare_remote)

        self.repo_path = self.tmp_path / "work"
        _clone_with_commit(self.bare_remote, self.repo_path, filename="README.md", content="# work\n")

        # Um segundo clone simula outro desenvolvedor empurrando um commit novo para o remote — o
        # clone original (`self.repo_path`) só passa a enxergar esse commit depois de um
        # `git fetch` real, o que deixa `behind` sair de 0 e prova que o fetch efetivamente rodou.
        self.other_clone = self.tmp_path / "other"
        _clone_with_commit(
            self.bare_remote, self.other_clone, filename="novo.txt", content="conteudo novo\n"
        )

    def tearDown(self) -> None:
        self._tmp.cleanup()

    def test_fetch_updates_behind_count_after_remote_gains_a_commit(self) -> None:
        before_fetch = scan.build_repo_item(self.repo_path)["repo"]
        self.assertEqual(before_fetch["remote_status"], {"kind": "tracked", "ahead": 0, "behind": 0})

        updated_repo = scan.fetch(self.repo_path)

        self.assertEqual(updated_repo["remote_status"], {"kind": "tracked", "ahead": 0, "behind": 1})
        self.assertEqual(updated_repo["id"], before_fetch["id"])

    def test_fetch_failure_raises_fetch_failed_error_with_stderr_detail(self) -> None:
        # Aponta o remote 'origin' para um caminho inexistente para forçar a falha do `git fetch`.
        _run(
            ["git", "remote", "set-url", "origin", str(self.tmp_path / "no-such-remote")],
            self.repo_path,
        )

        with self.assertRaises(scan.FetchFailedError) as ctx:
            scan.fetch(self.repo_path)

        self.assertTrue(ctx.exception.detail)


class ExecUnavailableTests(unittest.TestCase):
    """Binário `git` ausente do sistema (T045) — nunca derruba o processo, só a operação pontual."""

    def setUp(self) -> None:
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp_path = Path(self._tmp.name)
        self._original_which = scan.shutil.which
        scan.shutil.which = lambda _name: None  # simula `git` ausente do PATH

    def tearDown(self) -> None:
        scan.shutil.which = self._original_which
        self._tmp.cleanup()

    def test_scan_repositories_raises_exec_unavailable_when_repos_present(self) -> None:
        _init_repo_with_commit(self.tmp_path / "repo", name="repo")
        with self.assertRaises(scan.ExecUnavailableError):
            scan.scan_repositories(self.tmp_path)

    def test_scan_repositories_with_no_repos_does_not_require_git(self) -> None:
        # scan_root vazio nunca precisa invocar git - não deve levantar ExecUnavailableError.
        self.assertEqual(scan.scan_repositories(self.tmp_path), [])

    def test_fetch_raises_exec_unavailable(self) -> None:
        with self.assertRaises(scan.ExecUnavailableError):
            scan.fetch(self.tmp_path)


if __name__ == "__main__":
    unittest.main()
