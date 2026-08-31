"""Leitura da configuração do plugin `git-local`.

Ver `specs/001-walking-skeleton-git-plugin/contracts/git-local-plugin.md` § Configuração (FR-012).
Este arquivo de configuração é próprio do plugin — não faz parte do protocolo JSON-RPC e nunca é
lido pelo core (`protocol/SPEC.md` não normatiza nada sobre ele).

Formato TOML mínimo, um único campo usado nesta feature: `scan_root`. Usa `tomllib` (biblioteca
padrão desde Python 3.11, apenas leitura) — decisão D3 de `research.md`: sem dependência externa
em nenhum ponto da implementação deste plugin.
"""

from __future__ import annotations

import os
import tomllib
from pathlib import Path

# Único valor de caminho hardcoded permitido em todo o plugin — fallback quando o arquivo de
# configuração não existe ou não define `scan_root` (Clarifications Q3 de spec.md). O caminho
# efetivo MUST NOT vir de nenhum outro lugar do código.
DEFAULT_SCAN_ROOT = "~/dev"


def config_path() -> Path:
    """Caminho do arquivo de configuração deste plugin.

    `$XDG_CONFIG_HOME/farol/plugins/git-local/config.toml`, com fallback para
    `~/.config/farol/plugins/git-local/config.toml` quando `XDG_CONFIG_HOME` não está definida
    (convenção XDG padrão em Linux).
    """
    xdg_config_home = os.environ.get("XDG_CONFIG_HOME")
    base = Path(xdg_config_home) if xdg_config_home else (Path.home() / ".config")
    return base / "farol" / "plugins" / "git-local" / "config.toml"


def load_scan_root(path: Path | None = None) -> Path:
    """Devolve o `scan_root` configurado, já com `~` expandido pelo próprio plugin.

    Se `path` (ou o arquivo de configuração padrão) não existir, não puder ser lido, ou não
    definir o campo `scan_root`, o default é `~/dev` (FR-012, Clarifications Q3) — nunca um erro.
    """
    cfg_path = path if path is not None else config_path()

    raw_value = DEFAULT_SCAN_ROOT
    try:
        with cfg_path.open("rb") as handle:
            data = tomllib.load(handle)
        raw_value = data.get("scan_root", DEFAULT_SCAN_ROOT)
    except (FileNotFoundError, tomllib.TOMLDecodeError, OSError):
        # Arquivo ausente, ilegível ou malformado: comportamento MUST ser o mesmo default de
        # "campo ausente" — o plugin nunca falha por causa de um arquivo de config problemático.
        raw_value = DEFAULT_SCAN_ROOT

    if not isinstance(raw_value, str) or not raw_value.strip():
        raw_value = DEFAULT_SCAN_ROOT

    return Path(raw_value).expanduser()
