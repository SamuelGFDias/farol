"""Testes de regressão para `config.py` — só stdlib (`unittest`), sem framework externo (D7 de
`research.md`). T046 (`specs/002-uptime-kuma-plugin/tasks.md`): leitura de variável de ambiente
(`config.py`/`secrets.py`, T021/T022) substitui o mock de `op read` de versões anteriores deste
documento — aqui via `unittest.mock.patch.dict(os.environ, ...)`, não um segredo real.

Rodar com: `pytest tests/unit/test_uptime_kuma_config.py -v` (ou `python3 -m unittest
discover -s tests/unit -p "test_uptime_kuma_*.py"`) a partir da raiz do repo. Movido de
`plugins/uptime-kuma/test_config.py` para `tests/unit/` na issue #8.
"""

from __future__ import annotations

import os
import sys
import unittest
from pathlib import Path
from unittest import mock

PLUGIN_DIR = Path(__file__).resolve().parents[2] / "plugins" / "uptime-kuma"
if str(PLUGIN_DIR) not in sys.path:
    sys.path.insert(0, str(PLUGIN_DIR))

from config import ENV_VAR_BASE_URL, load_base_url  # noqa: E402 — import depende do sys.path.insert acima


class LoadBaseUrlTests(unittest.TestCase):
    def test_returns_value_when_env_var_is_set(self) -> None:
        with mock.patch.dict(os.environ, {ENV_VAR_BASE_URL: "https://kuma.example.com"}):
            self.assertEqual(load_base_url(), "https://kuma.example.com")

    def test_returns_none_when_env_var_is_absent(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertIsNone(load_base_url())

    def test_returns_none_when_env_var_is_empty_string(self) -> None:
        """Ausência e string vazia são tratadas do mesmo jeito — "não configurado" (FR-007/FR-008),
        não um `base_url` literal vazio.
        """
        with mock.patch.dict(os.environ, {ENV_VAR_BASE_URL: ""}):
            self.assertIsNone(load_base_url())


if __name__ == "__main__":
    unittest.main()
