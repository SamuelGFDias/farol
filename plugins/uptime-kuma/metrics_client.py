"""Cliente HTTP para consultar o endpoint `/metrics` da instância Uptime Kuma.

D7 de `research.md` (feature 002): sem dependência externa, usando apenas `urllib.request` da
biblioteca padrão. Implementa autenticação via HTTP Basic Auth, com a credencial (base URL e API
Key) fornecida como parâmetro — lida em `secrets.py` a partir de variáveis de ambiente já
resolvidas pelo core.

Responsabilidades:
- Montar URL absoluta a partir de base_url e path (`/metrics`)
- Codificar credencial em Base64 para o header `Authorization: Basic`
- Fazer requisição GET com timeout (10s, conforme D5 de `research.md` — timeout HTTP interno,
  deliberadamente menor que o intervalo de refresh, para nunca sobrepor duas tentativas)
- Desserializar resposta como texto (formato Prometheus, parseable por `metrics_parser.py`)

Apenas biblioteca padrão — `urllib.request`, `base64`.
"""

from __future__ import annotations
