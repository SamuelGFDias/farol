# Contrato: Handshake de Inicialização (`handshake/hello`)

**Pré-requisito**: `framing-and-versioning.md` (framing NDJSON, regra de versão).

Cobre FR-004, FR-005, FR-006, FR-006a, FR-006b, FR-007, FR-008.

## Sequenciamento

O core spawna o processo do plugin e, assim que `stdin`/`stdout` estão conectados, envia o
`handshake/hello` request **primeiro** (o core é sempre quem inicia — o plugin nunca fala antes de
ser perguntado; simetria com o papel de "client" do LSP `initialize`). O plugin MUST responder ao
`handshake/hello` antes de aceitar qualquer outro método.

```
core                                   plugin (processo filho)
 |--- spawn() ------------------------->|
 |--- handshake/hello (request id=1) -->|
 |                                      | (valida versão, monta manifesto)
 |<-- result (id=1) --------------------|
 | [core compara versões — ver framing-and-versioning.md]
 | [se compatível: PluginState = Ready; se não: Unavailable{VersionIncompatible}]
```

## Request: `handshake/hello`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "handshake/hello",
  "params": {
    "protocol_version": "0.1",   // string MAJOR.MINOR — versão que o core suporta (D7)
    "core_name": "farol-core"    // identificação do core, informativo
  }
}
```

## Response (sucesso): `HandshakeHelloResult`

```jsonc
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "protocol_version": "0.1",       // versão que o plugin fala (D7)
    "plugin_name": "git-local",      // FR-006: identidade do plugin
    "capabilities": {                // FR-007
      "capabilities": ["exec"]
    },
    "widgets": [                     // FR-006
      {
        "id": "repo-status",
        "kind": "status-grid",
        "title": "Repositórios Git",
        "suggested_refresh_interval_ms": 30000   // opcional, FR-011
      }
    ],
    "actions": []                    // FR-006a — ver nota de sequenciamento abaixo
  }
}
```

### Nota de sequenciamento: por que `actions` pode vir vazio no handshake

A lista de ações de fetch (uma por repositório) só é conhecível depois que o plugin varre o
diretório configurado (FR-012) — e essa varredura, para o plugin de referência Git, é a mesma
operação que produz os dados do widget (`widget/get`, ver `widget-protocol.md`). Duas opções foram
consideradas para onde `actions[]` aparece pela primeira vez:

1. **Handshake bloqueia até a primeira varredura completar**, retornando `actions[]` já populado no
   `HandshakeHelloResult`. Rejeitada como obrigatória: acopla o tempo de resposta do handshake ao
   tempo de uma varredura de filesystem (potencialmente lenta em `~/dev` grande), arriscando o
   timeout de handshake (D6) em máquinas com muitos repositórios.
2. **Handshake retorna `actions: []`** (lista vazia — plugin ainda não varreu nada), e a lista real
   de ações passa a viajar **dentro de cada resposta de `widget/get`**, uma `ActionDeclaration` por
   `GitRepository` retornado (ver `action-protocol.md` e `widget-protocol.md`). **Adotada.**

Isso é permitido pelo protocolo porque FR-006a exige que o plugin declare as ações que oferece
"simetricamente aos widgets" — não exige que a declaração completa esteja presente no exato
instante do handshake, apenas que o core nunca infira/hardcode uma ação por conta própria (FR-006b)
e que toda ação exposta na UI tenha vindo de uma declaração explícita do plugin, seja ela entregue
no handshake ou em uma atualização subsequente do widget que a declara.

## Response (erro de negociação, exemplo ilustrativo)

Se o plugin, por algum motivo, recusar o `protocol_version` do core (ex.: o plugin só fala uma
versão muito mais nova e decide falhar cedo em vez de deixar o core decidir), ele PODE responder
com um erro JSON-RPC em vez de `result` — ver `error-model.md`. Nesta feature de referência, o
plugin `git-local` sempre responde com `result` (nunca recusa por conta própria); é o **core** quem
aplica a regra de compatibilidade de `framing-and-versioning.md` sobre o `protocol_version`
recebido no `result`. O caminho de erro explícito do plugin existe no protocolo para plugins
futuros mais restritivos, mas não é exercitado pelo plugin `git-local` desta feature.

## Timeout

O handshake é uma requisição JSON-RPC como qualquer outra — sujeita ao `RPC_TIMEOUT` (default 5s,
D6). Se expirar, `PluginState` vai para `Unavailable{Unresponsive}`, nenhum widget é registrado
(mesmo efeito de UI de uma falha de versão — FR-020 não distingue os dois na UI, ver
`data-model.md` § 2.1).
