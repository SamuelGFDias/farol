# Contrato: Modelo de Erro

**Pré-requisito**: `framing-and-versioning.md`.

Forma padrão JSON-RPC 2.0 para o campo `error` de qualquer response:

```jsonc
{
  "code": <integer>,
  "message": "<string legível>",
  "data": { "reason": "<string_de_domínio_farol>", /* campos extras por reason */ }
}
```

## Códigos reservados (faixa JSON-RPC padrão)

Códigos `-32700`..`-32600`..`-32603` seguem a reserva padrão JSON-RPC 2.0 (parse error, invalid
request, method not found, invalid params, internal error) — usados apenas para erros de
protocolo genéricos (ex.: `method not found` se o core chamar um método que o plugin não
implementa). Não são o caminho principal desta feature.

## Códigos de domínio Farol (faixa `-32000` a `-32099`, reservada para aplicação)

| `code` | `data.reason` | Onde ocorre | Descrição |
|---|---|---|---|
| `-32000` | `protocol_version_incompatible` | resposta de `handshake/hello` (caminho de recusa do lado do plugin — ver nota em `handshake.md`) | O plugin recusa a versão proposta pelo core. |
| `-32001` | `fetch_failed` | resposta de `action/invoke` (`git.fetch`) | `git fetch` retornou código de saída não-zero (ex.: rede indisponível, credencial inválida). `data.detail` carrega stderr/mensagem do git. |
| `-32002` | `action_timeout` | reportado pelo core à UI (não pelo plugin — é o core sintetizando este erro quando um `RPC_TIMEOUT` estoura numa invocação de `action/invoke` pontual, ver `action-protocol.md`) | A invocação de ação não respondeu a tempo; não implica plugin indisponível por si só. |
| `-32003` | `exec_unavailable` | resposta de `handshake/hello` ou `action/invoke` | O binário `git` não está disponível no sistema para o plugin executar (Edge Case da spec) — o plugin reporta isso como erro/capacidade indisponível sem derrubar seu próprio processo. |
| `-32004` | `scan_root_unreadable` | resposta de `widget/get` | O diretório raiz configurado existe mas não é legível (erro de permissão) — distinto de "não existe/vazio", que é sucesso com `items: []`. |

Esta tabela é normativa para o plugin de referência `git-local` desta feature; um plugin futuro
pode reservar outros `reason` dentro da mesma faixa de `code`, documentando-os na sua própria seção
de `protocol/SPEC.md` (fora do escopo desta feature criar essa extensibilidade formal — registrado
aqui só como intenção para não colidir faixas no futuro).

## Regra geral (FR-017, FR-019)

Nenhum erro definido nesta tabela derruba o processo do plugin nem o core.

- `-32000` (`protocol_version_incompatible`), quando emitido pelo plugin em vez de `result` no
  handshake, resulta em `PluginState = Unavailable{VersionIncompatible}` (ver `handshake.md`).
- `-32001`, `-32002`, `-32003`, `-32004` são erros pontuais de uma chamada (`widget/get` ou
  `action/invoke`) — exibidos associados ao repositório/ação correspondente na UI, sem afetar
  `PluginState`. Em particular, `-32003` (`exec_unavailable`, binário `git` ausente) não torna o
  plugin "indisponível": o processo do plugin continua vivo e respondendo, só a operação que
  dependia de `exec` falha — a UI mostra o erro no repositório/ação afetada.
