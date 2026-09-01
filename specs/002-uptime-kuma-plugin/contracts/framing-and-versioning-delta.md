# Contrato (delta): Framing e Versionamento

**Pré-requisito**: `specs/001-walking-skeleton-git-plugin/contracts/framing-and-versioning.md`
(normativo, já promovido a `protocol/SPEC.md` §4/§6.4 real do repositório). Este documento cobre
**apenas o que muda ou é adicionado** por esta feature — não repete o que continua válido.

## Transporte e framing — inalterados

Nenhuma mudança. Continua stdin/stdout do processo filho, NDJSON (uma linha de JSON compacto por
mensagem, `\n`, UTF-8), sem `Content-Length`. `research.md` D2 desta feature reafirma
explicitamente D2 da feature 001 sem redesenho — nenhum requisito desta feature (leitura periódica de
rede, configuração/segredo geridos pelo core, novo widget) toca o canal NDJSON entre core e plugin; a
chamada HTTP acontece inteiramente dentro do processo do plugin, nunca no canal de protocolo.

## Versionamento — bump para `"0.2"`

- **Nova versão declarada por ambos os lados desta feature**: `protocol_version = "0.2"`.
- **Classificação**: MINOR (não MAJOR) — decisão justificada em `research.md` D1. Resumo: a mudança
  de wire (`CapabilityManifest.capabilities`, de `string[]` para `Capability[]` estruturado por
  `kind`) é, pela letra da regra geral de `protocol/SPEC.md` §6.4, uma mudança de forma que poderia
  justificar MAJOR — mas como o regime vigente é `MAJOR == 0` (compatibilidade por **igualdade exata
  de string**, já estabelecida por D7 da feature 001), o efeito em runtime de rotular esta mudança
  como MINOR ou MAJOR é **idêntico**: um plugin em `"0.1"` é rejeitado por um core em `"0.2"` da
  mesma forma que seria rejeitado por um core em `"1.0"`. Bumpar para `MAJOR = 1` sinalizaria
  prematuramente que o protocolo estabilizou — o que não é verdade nesta fase do projeto (segunda
  feature de plugin). Decisão: **MINOR, `"0.1"` → `"0.2"`**.
- **Algoritmo de compatibilidade — inalterado** (`protocol/SPEC.md` §6.4, reafirmado):

  ```text
  se plugin.MAJOR == 0 (série pré-1.0):
      compatível ⟺ plugin.protocol_version == core.protocol_version   # igualdade exata
  senão:
      compatível ⟺ plugin.MAJOR == core.MAJOR E core.MINOR >= plugin.MINOR
  ```

  Como `MAJOR` permanece `0` após este bump, a regra aplicável continua sendo a de igualdade exata —
  nenhuma mudança de algoritmo, só um novo valor de string sendo comparado.

## Consequência direta e deliberada: `git-local` (feature 001) fica incompatível

Este é o ponto que este delta precisa deixar mais explícito que qualquer outro, por instrução direta
da task que gerou este plano:

- O plugin `git-local`, **inalterado**, continua declarando `protocol_version = "0.1"` e
  `capabilities: {"capabilities": ["exec"]}` (formato antigo).
- Um `farol-core` atualizado para esta feature (falando `"0.2"`) recusa `git-local` **de forma
  limpa**, pelo mecanismo já construído na feature 001: `HandshakeHelloResult.protocol_version`
  (`"0.1"`) difere de `core.protocol_version` (`"0.2"`) → incompatibilidade detectada **antes** de
  qualquer tentativa de interpretar `capabilities` no formato novo → `PluginState` transiciona para
  `Unavailable{VersionIncompatible}`, com mensagem legível citando as duas versões
  (`protocol/SPEC.md` §6.4) → **nenhum crash do core, nenhum erro de parse confuso** — exatamente o
  comportamento que a checagem de versão por igualdade exata existe para garantir.
- **Efeito prático**: `git-local` para de funcionar (nenhum widget de repositórios git é exibido)
  assim que o usuário atualiza para um core desta feature, até `git-local` ser migrado para
  `protocol_version = "0.2"` + o novo formato de `capabilities`. Esta é uma perda de funcionalidade
  real e imediata para quem já usa o plugin da feature 001 — não uma ressalva teórica.
- **Migração de `git-local` NÃO é feita por esta feature** (fora do escopo desta sessão de
  planejamento, que cobre exclusivamente `002-uptime-kuma-plugin`) — é **débito técnico registrado**
  que, pela constitution v0.3.0 (Governance, "Dívida técnica rastreável"), **MUST** virar uma issue
  no GitHub antes de esta feature ser considerada encerrada. Ver `plan.md` § Constitution Check e
  § Complexity Tracking para o registro formal dessa obrigação; a issue em si não é criada por este
  plano.

## Correlação de requisição/resposta — inalterada

Nenhuma mudança: `id` único por conexão, respostas ecoam o mesmo `id`, mensagens não-reconhecíveis
são descartadas silenciosamente (logadas, nunca tratadas como "plugin indisponível" por si só).
