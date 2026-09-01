"""Thread de polling em background com cache lock-guarded.

D6 de `research.md` (feature 002): resolve FR-010 (RPC_TIMEOUT_CONTROL nunca bloqueia em latência
de rede). A fonte de dados do plugin é uma chamada HTTP potencialmente lenta contra `/metrics`;
este arquivo implementa um background thread que:
- Faz poll periódico do endpoint (intervalo configurável, conforme `suggested_refresh_interval_ms`
  declarado no handshake)
- Guarda resultado em cache protegido por lock (mutual exclusion)
- Trata erros de rede/timeout gracefully (cache obsoleto melhor que erro imediato)

`widget/get` (em `main.py`) nunca bloqueia esperando uma chamada de rede — sempre lê do cache,
respondendo dentro do orçamento `RPC_TIMEOUT_CONTROL` (5s default, conforme D5 de `research.md`).

Responsabilidades:
- Espawnar thread daemon ao startup
- Loop infinito de polling com intervalo
- Chamar `metrics_client.py` para fetch e `metrics_parser.py` para parse
- Proteger cache por lock (threading.Lock) contra race conditions entre background thread e
  requisições `widget/get` do core

Apenas biblioteca padrão — `threading`, `time`.
"""

from __future__ import annotations
