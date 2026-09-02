"""Testes de regressão para `secrets.py` — só stdlib (`unittest`), sem framework externo (D7 de
`research.md`). T046 (`specs/002-uptime-kuma-plugin/tasks.md`): leitura de variável de ambiente
(`config.py`/`secrets.py`, T021/T022) substitui o mock de `op read` de versões anteriores deste
documento — aqui via `unittest.mock.patch.dict(os.environ, ...)`, com um valor sintético, nunca um
segredo real.

Rodar com: `python3 test_secrets.py` (ou `python3 -m unittest test_secrets`).
"""

from __future__ import annotations

import os
import unittest
from secrets import ENV_VAR_API_KEY, load_api_key
from unittest import mock


class LoadApiKeyTests(unittest.TestCase):
    def test_returns_value_when_env_var_is_set(self) -> None:
        with mock.patch.dict(os.environ, {ENV_VAR_API_KEY: "synthetic-api-key"}):
            self.assertEqual(load_api_key(), "synthetic-api-key")

    def test_returns_none_when_env_var_is_absent(self) -> None:
        with mock.patch.dict(os.environ, {}, clear=True):
            self.assertIsNone(load_api_key())

    def test_returns_none_when_env_var_is_empty_string(self) -> None:
        """Mesmo tratamento de `config.load_base_url()`: ausência e string vazia colapsam no mesmo
        `None` — "não configurado" (FR-007/FR-008), não uma API key literal vazia.
        """
        with mock.patch.dict(os.environ, {ENV_VAR_API_KEY: ""}):
            self.assertIsNone(load_api_key())


if __name__ == "__main__":
    unittest.main()
