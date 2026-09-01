"""Leitura de segredos do plugin `uptime-kuma` via variáveis de ambiente.

Complementa `config.py` para a credencial de autenticação (API Key) contra o endpoint `/metrics`
da instância Uptime Kuma. Na revisão de D8 de `research.md` (auditoria pós-plan da feature 002),
o mecanismo de segredo foi redesenhado: deixou de ser resolvido via CLI `op` (1Password) no
próprio plugin, e passou a ser gerido inteiramente pelo **core**, que armazena em
`$XDG_CONFIG_HOME/farol/secrets.toml` (permissão 0600), nunca expõe em texto plano na UI, e
injeta como variável de ambiente no spawn do processo.

Convenção de variável de ambiente: `FAROL_PLUGIN_UPTIME_KUMA_API_KEY`. Este arquivo não invoca
nenhum subprocess — apenas lê ambiente.

Nota: `config.py` e `secrets.py` podem colapsar numa única task futura (T0xx), mas por enquanto
ficam separados conforme a árvore de `plan.md` § Project Structure.

Apenas biblioteca padrão — `os`.
"""

from __future__ import annotations

import os

ENV_VAR_API_KEY = "FAROL_PLUGIN_UPTIME_KUMA_API_KEY"


def load_api_key() -> str | None:
    """Lê `FAROL_PLUGIN_UPTIME_KUMA_API_KEY` do ambiente.

    Mesmo mecanismo de `config.load_base_url()`: sem default seguro, ausência ou valor vazio da
    variável é devolvido como `None` — quem chama trata isso como "não configurado", não é
    responsabilidade desta função.
    """
    value = os.environ.get(ENV_VAR_API_KEY)
    if not value:
        return None
    return value
