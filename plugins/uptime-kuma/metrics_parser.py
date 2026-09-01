"""Parser Prometheus mínimo para métricas da instância Uptime Kuma.

D7 de `research.md` (feature 002): sem dependência externa de parse de Prometheus, implementado
com regex/string da biblioteca padrão. O `/metrics` do Uptime Kuma expõe duas métricas relevantes:
- `monitor_status{...}` — status do monitor (1=up, 0=down, outros=intermediate)
- `monitor_response_time{...}` — tempo de resposta em milissegundos

Este arquivo extrai e mapeia essas métricas para os campos de `MonitorStatusItem` do protocolo
Farol v0.2 (name/status/response_time_ms), sem dependência de schema ou tipo externo — apenas
dicionários e strings Python.

Responsabilidades:
- Iterar linhas do texto Prometheus
- Pular comentários (`#`) e linhas vazias
- Fazer parsing de `metric_name{labels} value` com regex
- Agrupar valores por monitor (usando labels como chave)
- Mapear status numérico (1/0/outro) para label descritivo (up/down/intermediate, conforme
  mapeamento FR-012 do spec)

Apenas biblioteca padrão — `re`, `dict`, `str`.
"""

from __future__ import annotations
