# Contrato: Framing e Versionamento do Protocolo

**Feature**: `001-walking-skeleton-git-plugin` — este documento é a base normativa de todos os
outros contratos nesta pasta. Corresponde ao que, no repositório real (fora do escopo desta
feature de planejamento), vai virar `protocol/SPEC.md` — ver `plan.md` § Project Structure.

## Transporte

- Canal: stdin/stdout do processo filho do plugin (FR-003). stdin do plugin recebe requests do
  core; stdout do plugin emite responses ao core. stderr do plugin é livre para logging humano —
  o core MUST NOT tentar interpretar stderr como protocolo.
- Mensagens: JSON-RPC 2.0 (`jsonrpc`, `id`, `method`, `params` | `result` | `error`).

## Framing: NDJSON (decisão D2 de `research.md`)

- Cada mensagem JSON-RPC é escrita como **uma linha** de JSON compacto (sem espaços/indentação
  supérfluos, sem quebras de linha internas), seguida por um único `\n` (LF).
- Codificação: UTF-8.
- **MUST**: implementações (core e qualquer plugin) MUST serializar cada mensagem em modo
  compacto/single-line. Um serializador "bonito" (pretty-print) é uma violação do protocolo, pois
  introduz `\n` cru dentro do corpo de uma mesma mensagem, quebrando o framing por linha.
- **MUST**: o leitor (tanto core lendo stdout do plugin, quanto plugin lendo stdin do core) lê uma
  linha completa (até `\n`) antes de tentar `json.loads`/`serde_json::from_str`. Uma linha vazia
  MUST ser ignorada silenciosamente (permite heartbeats/robustez de buffering, mas não é usada
  ativamente por esta feature).
- Justificativa completa da escolha (vs. `Content-Length` do LSP): ver `research.md` § D2.

### Exemplo de duas mensagens consecutivas no stream (ilustrativo, não literal desta feature)

```
{"jsonrpc":"2.0","id":1,"method":"handshake/hello","params":{"protocol_version":"0.1","core_name":"farol-core"}}
{"jsonrpc":"2.0","id":1,"result":{"protocol_version":"0.1","plugin_name":"git-local","capabilities":{"capabilities":["exec"]},"widgets":[{"id":"repo-status","kind":"status-grid","title":"Repositórios Git"}],"actions":[]}}
```

## Versionamento (decisão D7 de `research.md`)

- Campo `protocol_version`: string `"MAJOR.MINOR"` (ex.: `"0.1"`).
- **MAJOR**: incrementa em mudança incompatível de wire (campo obrigatório removido, semântica de
  campo mudada, método removido).
- **MINOR**: incrementa em adição compatível (campo opcional novo, método novo que um consumidor
  antigo pode ignorar).
- **Algoritmo de compatibilidade** (avaliado pelo core, ao receber `HandshakeHelloResult`):

  ```
  se plugin.MAJOR == 0 (série pré-1.0):
      compatível ⟺ plugin.protocol_version == core.protocol_version   # igualdade exata
  senão:
      compatível ⟺ plugin.MAJOR == core.MAJOR E core.MINOR >= plugin.MINOR
  ```

- Em caso de incompatibilidade, o core MUST recusar a inicialização deste plugin (FR-005),
  transicionar seu estado para `Unavailable{VersionIncompatible}` (ver `data-model.md` § 2.1), e
  exibir mensagem legível citando ambas as versões, ex.:
  `"plugin 'git-local' declara protocolo 0.2; este core suporta 0.1 — versões incompatíveis"`.
- Esta feature (`001-walking-skeleton-git-plugin`) fixa `protocol_version = "0.1"` em ambos os
  lados (core e plugin de referência).

## Correlação de requisição/resposta

- Todo request do core carrega `id` (inteiro ou string, único por conexão, escolhido pelo core).
- Toda resposta do plugin ecoa o mesmo `id`.
- O core MUST descartar (logar, não crashar) qualquer mensagem de stdout do plugin que não seja
  JSON-RPC válido ou que referencie um `id` desconhecido — isso não é tratado como
  "plugin indisponível" por si só nesta feature (só timeout ou morte do processo o são — ver
  `research.md` § D6).
