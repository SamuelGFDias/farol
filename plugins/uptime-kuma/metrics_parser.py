"""Parser Prometheus mínimo para o corpo de `/metrics` do Uptime Kuma.

Ver `specs/002-uptime-kuma-plugin/contracts/uptime-kuma-plugin.md` § "Parsing e mapeamento de
status" (FR-011, FR-012, FR-016) e `research.md` D7.

Reconhece **apenas** duas famílias de métrica, uma linha por amostra:
- `monitor_status{monitor_name="...", ...} <valor>` → mapeada para `status` (`1→up`, `0→down`,
  `2→pending`, `3→maintenance`).
- `monitor_response_time{monitor_name="...", ...} <valor>` → `response_time_ms` (arredondado).

Qualquer outra família de métrica é ignorada. Linhas malformadas isoladas dentro das duas famílias
reconhecidas (ex.: sem `monitor_name`) são puladas — tolerância parcial. `MetricsParseError` é
levantado apenas quando a resposta inteira é inválida: nenhum sinal de `monitor_status` reconhecível
em todo o corpo (nem amostra, nem a declaração `# HELP`/`# TYPE monitor_status` que o Prometheus
sempre emite para uma família de métrica registrada, mesmo sem nenhuma amostra), ou algum valor de
`monitor_status` fora de `{0, 1, 2, 3}`.

Uma instância Uptime Kuma real, acessível, mas sem nenhum monitor cadastrado, ainda emite a
declaração `# HELP`/`# TYPE monitor_status` (o exportador declara a família de métrica
independentemente de haver amostras) — só não emite nenhuma linha `monitor_status{...}`. Esse caso é
distinto de um corpo genuinamente não reconhecível (ex.: página HTML de erro), que não tem nem a
declaração nem a amostra: o primeiro é sucesso com `items: []` (Edge Case de `spec.md`, Cenário 6 de
`quickstart.md`, T039/T051 de `specs/002-uptime-kuma-plugin/tasks.md`), o segundo continua
`MetricsParseError`.

Apenas biblioteca padrão — `re`.
"""

from __future__ import annotations

import re

STATUS_MAP = {
    1: "up",
    0: "down",
    2: "pending",
    3: "maintenance",
}

_METRIC_LINE_RE = re.compile(
    r"^(?P<name>[A-Za-z_][A-Za-z0-9_]*)\{(?P<labels>[^}]*)\}\s+(?P<value>\S+)\s*$"
)
_LABEL_RE = re.compile(r'(?P<key>[A-Za-z_][A-Za-z0-9_]*)\s*=\s*"(?P<value>[^"]*)"')

_MONITOR_STATUS_DECLARATION_RE = re.compile(r"^#\s*(HELP|TYPE)\s+monitor_status\b")


class MetricsParseError(Exception):
    """Corpo de `/metrics` não reconhecível como resposta válida do Uptime Kuma (`-32007`)."""


def _parse_labels(raw_labels: str) -> dict:
    return {match.group("key"): match.group("value") for match in _LABEL_RE.finditer(raw_labels)}


def parse_metrics(body: str) -> list[dict]:
    """Faz o parse do corpo de `/metrics` e devolve `MonitorStatusItem[]` (como dicts).

    Levanta `MetricsParseError` quando nenhuma linha `monitor_status{...}` é encontrada, ou quando
    algum valor de `monitor_status` está fora de `{0, 1, 2, 3}` — invalida a resposta inteira
    daquela tentativa (não item a item, D7 de `research.md`).
    """
    statuses: dict[str, int] = {}
    response_times: dict[str, float] = {}
    found_monitor_status_line = False
    found_monitor_status_declaration = False

    for raw_line in body.splitlines():
        line = raw_line.strip()
        if not line:
            continue
        if line.startswith("#"):
            if _MONITOR_STATUS_DECLARATION_RE.match(line):
                found_monitor_status_declaration = True
            continue

        match = _METRIC_LINE_RE.match(line)
        if match is None:
            continue

        metric_name = match.group("name")
        if metric_name not in ("monitor_status", "monitor_response_time"):
            continue

        labels = _parse_labels(match.group("labels"))
        monitor_name = labels.get("monitor_name")
        if not monitor_name:
            # Linha malformada isolada (sem monitor_name) — pulada, não invalida o parse inteiro.
            continue

        if metric_name == "monitor_status":
            found_monitor_status_line = True
            try:
                raw_value = int(float(match.group("value")))
            except ValueError:
                # Valor não numérico — linha malformada isolada, pulada.
                continue
            if raw_value not in STATUS_MAP:
                raise MetricsParseError(
                    f"valor de monitor_status fora do domínio esperado: {raw_value!r}"
                )
            statuses[monitor_name] = raw_value
        else:  # monitor_response_time
            try:
                response_times[monitor_name] = float(match.group("value"))
            except ValueError:
                # Valor não numérico — linha malformada isolada, pulada.
                continue

    if not found_monitor_status_line and not found_monitor_status_declaration:
        raise MetricsParseError("nenhuma linha monitor_status{...} encontrada no corpo de /metrics")

    items: list[dict] = []
    for monitor_name, raw_status in statuses.items():
        response_time_ms = None
        if monitor_name in response_times:
            raw_response_time = response_times[monitor_name]
            # O Uptime Kuma emite -1 como sentinela de "não aplicável" (ex.: monitores `docker`).
            # Um tempo de resposta nunca é negativo de verdade, então qualquer valor < 0 vira
            # None (→ `null` no JSON) — ver protocol/schema/v0.2/widget.schema.json.
            if raw_response_time >= 0:
                response_time_ms = round(raw_response_time)
        items.append(
            {
                "name": monitor_name,
                "status": STATUS_MAP[raw_status],
                "response_time_ms": response_time_ms,
            }
        )

    return items
