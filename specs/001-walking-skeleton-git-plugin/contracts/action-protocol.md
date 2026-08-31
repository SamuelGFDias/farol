# Contrato: Invocação de Ação (`action/invoke`)

**Pré-requisito**: `framing-and-versioning.md`, `handshake.md`, `widget-protocol.md`.

Cobre FR-015, FR-016, FR-017, FR-018.

## Fluxo (round-trip da User Story 2)

```
UI do core (usuário clica "Fetch" num repo, ação já exibida no estado declarado pelo plugin)
  │  FR-015: core só mostra a ação porque veio habilitada em ActionDeclaration.enabled == true
  ▼
core envia action/invoke (via worker/Subscription, D5 de research.md — não bloqueia update/view)
  ▼
plugin executa `git fetch` no repositório-alvo (capacidade `exec` do manifesto)
  ▼
plugin responde: sucesso (novo GitRepository) OU erro estruturado (FR-017 — processo do plugin
não morre por causa de uma falha do `git fetch`, ex.: rede indisponível)
  ▼
core funde o resultado no RepositoryViewModel do repo-alvo (FR-018) — sem precisar de um
widget/get adicional
```

## Request: `action/invoke`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 12,
  "method": "action/invoke",
  "params": {
    "action_id": "git.fetch",
    "target": { "type": "repo", "id": "/home/dev/projetos/farol" }
  }
}
```

- `target` MUST bater com o `target` da `ActionDeclaration` que a UI exibiu (o core envia de volta
  exatamente o alvo declarado, não reconstrói um alvo por conta própria — reforça FR-006b).
- O core só envia este request se a última `ActionDeclaration` conhecida para este `action_id` +
  `target` tinha `enabled: true`. Nenhuma validação adicional de "ainda está habilitada" é feita
  pelo plugin no momento da invocação nesta feature (sem requisito de corrida definido pela spec —
  ver Edge Cases; a spec assume que não há suporte formal a múltiplas execuções simultâneas da
  mesma ação no mesmo repositório).

## Response (sucesso)

```jsonc
{
  "jsonrpc": "2.0",
  "id": 12,
  "result": {
    "repo": {
      "id": "/home/dev/projetos/farol",
      "name": "farol",
      "path": "/home/dev/projetos/farol",
      "dirty": false,
      "remote_status": { "kind": "tracked", "ahead": 0, "behind": 0 }
    }
  }
}
```

- O `GitRepository` retornado é o estado **pós-fetch** (FR-018) — o core substitui diretamente o
  `repo` do `RepositoryViewModel` correspondente, sem chamada adicional.

## Response (erro estruturado — ex.: rede indisponível)

```jsonc
{
  "jsonrpc": "2.0",
  "id": 12,
  "error": {
    "code": -32001,
    "message": "git fetch falhou",
    "data": {
      "reason": "fetch_failed",
      "target": { "type": "repo", "id": "/home/dev/projetos/farol" },
      "detail": "fatal: unable to access '...': Could not resolve host"
    }
  }
}
```

- Ver `error-model.md` para o espaço de `code`/`data.reason`.
- FR-017: este erro MUST NOT derrubar o processo do plugin — é uma resposta JSON-RPC de erro
  normal, o plugin continua respondendo a `widget/get`/outras invocações depois.
- FR-018: o core exibe este erro associado ao repositório-alvo (`RepositoryViewModel.last_error`,
  ver `data-model.md` § 2.2); o `ahead`/`behind` exibido permanece o último valor conhecido
  (nenhum dado é perdido por causa de um fetch que falhou).

## Timeout

Sujeito ao mesmo `RPC_TIMEOUT` (D6) de qualquer requisição — mas **não** contribui para marcar
`PluginState = Unavailable{Unresponsive}` por si só nesta feature: um `git fetch` genuinamente
lento (rede ruim, não travamento do plugin) não deveria derrubar a percepção de "plugin
disponível" só porque uma ação pontual estourou o timeout local. Ver nota de resolução: o timeout
de uma invocação de ação específica é reportado ao usuário como erro daquela ação (mesmo formato de
erro estruturado acima, com `data.reason: "action_timeout"`), distinto do timeout do ciclo de
refresh periódico (que é o sinal usado por D6 para marcar o plugin como indisponível).
