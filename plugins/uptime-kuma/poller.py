"""Thread de polling em background + cache do plugin `uptime-kuma`.

Ver `research.md` D6 e `data-model.md` §2.3/§2.4: o handler de `widget/get` (rodando na thread
principal do loop NDJSON) nunca faz I/O de rede — ele só lê um cache (`MetricsCache`) mantido por
uma `threading.Thread(daemon=True)` que roda em laço estritamente sequencial (nunca duas chamadas
HTTP concorrentes por construção, já que é um único laço `while` numa única thread, não um
agendador paralelo).

Apenas biblioteca padrão — `threading`, `time`.
"""

from __future__ import annotations

import threading
import time

from metrics_client import MetricsUnreachableError, fetch_metrics
from metrics_parser import MetricsParseError, parse_metrics

DEFAULT_POLL_INTERVAL_MS = 30000


class MetricsCache:
    """Estado compartilhado entre a thread de polling e o handler de `widget/get`.

    `last_success`/`last_error` per `data-model.md` §2.3. Todo acesso (leitura ou escrita) MUST
    acontecer sob `self.lock`.
    """

    def __init__(self) -> None:
        self.lock = threading.Lock()
        self.last_success: dict | None = None
        self.last_error: dict | None = {
            "reason": "metrics_unreachable",
            "detail": "aguardando primeira leitura",
            "at": time.time(),
        }

    def record_success(self, monitors: list[dict]) -> None:
        with self.lock:
            self.last_success = {"monitors": monitors, "at": time.time()}

    def record_error(self, reason: str, detail: str) -> None:
        with self.lock:
            self.last_error = {"reason": reason, "detail": detail, "at": time.time()}

    def snapshot(self) -> tuple[dict | None, dict | None]:
        """Devolve `(last_success, last_error)` sob lock, para leitura consistente pelo handler."""
        with self.lock:
            return self.last_success, self.last_error


class PollerThread(threading.Thread):
    """Laço sequencial de polling de `/metrics`, a cada `interval_ms` (D6 de `research.md`).

    Só é instanciada/iniciada quando `base_url`/`api_key` estão ambos resolvidos — quem decide isso
    é `main.py`, não este módulo (mantém a checagem de `not_configured` num único lugar).
    """

    def __init__(
        self,
        base_url: str,
        api_key: str,
        cache: MetricsCache,
        interval_ms: int = DEFAULT_POLL_INTERVAL_MS,
    ) -> None:
        super().__init__(daemon=True)
        self._base_url = base_url
        self._api_key = api_key
        self._cache = cache
        self._interval_seconds = interval_ms / 1000.0

    def run(self) -> None:
        while True:
            self._poll_once()
            time.sleep(self._interval_seconds)

    def _poll_once(self) -> None:
        try:
            body = fetch_metrics(self._base_url, self._api_key)
        except MetricsUnreachableError as exc:
            self._cache.record_error("metrics_unreachable", str(exc))
            return

        try:
            monitors = parse_metrics(body)
        except MetricsParseError as exc:
            self._cache.record_error("metrics_parse_error", str(exc))
            return

        self._cache.record_success(monitors)
