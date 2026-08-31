# Contrato: Widget Declarativo (`widget/get`)

**Pré-requisito**: `framing-and-versioning.md`, `handshake.md`.

Cobre FR-009, FR-010, FR-011, FR-013, FR-014.

## Modelo: polling core-iniciado (decisão D8 de `research.md`)

O core chama `widget/get` a cada ciclo de refresh — intervalo = `suggested_refresh_interval_ms`
declarado pelo plugin no handshake (`handshake.md`), ou 30000ms se ausente (FR-011). O plugin
nunca envia dados de widget sem ser perguntado nesta feature (sem push/notificação).

## Request: `widget/get`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "method": "widget/get",
  "params": {
    "widget_id": "repo-status"   // id declarado em handshake.result.widgets[].id
  }
}
```

## Response (sucesso)

```jsonc
{
  "jsonrpc": "2.0",
  "id": 7,
  "result": {
    "widget_id": "repo-status",
    "items": [
      {
        "repo": {
          "id": "/home/dev/projetos/farol",
          "name": "farol",
          "path": "/home/dev/projetos/farol",
          "dirty": false,
          "remote_status": { "kind": "tracked", "ahead": 0, "behind": 2 }
        },
        "fetch_action": {
          "id": "git.fetch",
          "label": "Fetch",
          "target": { "type": "repo", "id": "/home/dev/projetos/farol" },
          "enabled": true
        }
      },
      {
        "repo": {
          "id": "/home/dev/projetos/scratch",
          "name": "scratch",
          "path": "/home/dev/projetos/scratch",
          "dirty": true,
          "remote_status": { "kind": "no_remote" }
        },
        "fetch_action": {
          "id": "git.fetch",
          "label": "Fetch",
          "target": { "type": "repo", "id": "/home/dev/projetos/scratch" },
          "enabled": false
        }
      }
    ]
  }
}
```

- `items` MAY ser uma lista vazia — diretório configurado sem repositórios git é estado válido, não
  erro (Edge Case da spec, Assumptions).
- Cada item pareia um `GitRepository` (ver `data-model.md` § 1.5) com sua `ActionDeclaration` de
  fetch correspondente (ver nota de sequenciamento em `handshake.md`) — é assim que `actions[]`,
  vazio no handshake, chega ao core na prática.
- **Invariante MUST**: `repo.remote_status.kind == "no_remote"` ⟺ `fetch_action.enabled == false`
  (FR-014). O core, defensivamente, também nunca habilita fetch para um repo `no_remote` mesmo que
  receba `enabled: true` por engano (ver `data-model.md` § 4) — mas o plugin de referência é a
  fonte que garante essa invariante corretamente.

## Renderização (FR-009/FR-010)

`items` é dado puro — nenhuma instrução de desenho, cor, layout ou markup. O `kind` do widget
(`"status-grid"`, declarado no handshake) é o único sinal que o core usa para escolher *como*
desenhar esses itens; o vocabulário de `kind` é definido pelo core (lista fechada nesta feature:
apenas `"status-grid"` é suportado — um plugin que declare um `kind` desconhecido tem seu widget
ignorado/não renderizado, sem derrubar o plugin nem o core).

## Erros e timeout

- Se o plugin responder com erro JSON-RPC (ex.: diretório configurado inacessível por permissão de
  filesystem — não confundir com "diretório não existe", que é lista vazia): ver
  `error-model.md`; o core mantém os últimos `items` conhecidos e sinaliza o erro pontualmente,
  sem mudar `PluginState` para `Unavailable` (um erro de uma chamada de `widget/get` não é, por si
  só, "plugin indisponível" — só timeout/morte do processo, D6, mudam `PluginState`).
- Timeout de `widget/get` (sem resposta dentro de `RPC_TIMEOUT`): conta para D6 — se ocorrer no
  ciclo de refresh, contribui para a detecção de plugin travado.
