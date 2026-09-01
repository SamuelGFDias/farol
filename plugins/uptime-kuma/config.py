"""Leitura da configuração do plugin `uptime-kuma`.

Ver `specs/002-uptime-kuma-plugin/contracts/uptime-kuma-plugin.md` § Configuração (D8 de
`research.md`). A configuração é declarada pelo plugin no handshake via campo `required_config`
(novo em protocol_version "0.2"), e o core (`farol-core`) a resolve e injeta como variável de
ambiente no spawn do processo.

Este arquivo não lê nenhum arquivo de configuração — apenas ambiente. Convenção de variável de
ambiente: `FAROL_PLUGIN_UPTIME_KUMA_<FIELD_NAME>` (ex.: `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL`,
`FAROL_PLUGIN_UPTIME_KUMA_API_KEY`). O core gerencia armazenamento persistente em
`$XDG_CONFIG_HOME/farol/plugins/uptime-kuma/config.toml` (não-secreto) e
`$XDG_CONFIG_HOME/farol/secrets.toml` (secreto, permissão 0600), mas o plugin nunca lê esses
arquivos — apenas variáveis de ambiente já resolvidas pelo core no momento do spawn.

Apenas biblioteca padrão — `os`.
"""

from __future__ import annotations

import os

ENV_VAR_BASE_URL = "FAROL_PLUGIN_UPTIME_KUMA_BASE_URL"


def load_base_url() -> str | None:
    """Lê `FAROL_PLUGIN_UPTIME_KUMA_BASE_URL` do ambiente.

    Sem default seguro (diferente de `scan_root` em `plugins/git-local/config.py`): ausência ou
    valor vazio da variável é devolvido como `None`. Tratar isso como "não configurado"
    (`not_configured`, FR-007/FR-008) é responsabilidade de quem chama, não desta função.
    """
    value = os.environ.get(ENV_VAR_BASE_URL)
    if not value:
        return None
    return value
