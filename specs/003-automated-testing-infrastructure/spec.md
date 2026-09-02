# Feature Specification: Infraestrutura de Testes Automatizada

**Feature Branch**: `[003-automated-testing-infrastructure]`

**Created**: 2026-09-01

**Status**: Draft

**Input**: User description: "Infraestrutura de testes automatizada para o Farol (harness de execução real, cobertura de contrato mais rigorosa, testes de UI/visual, e CI). Contexto: na feature 002, três bugs reais (dois panics de `Subscription::map` com closure capturante, um erro de decode `#[serde(untagged)]` mascarado como timeout) só apareceram rodando o binário `farol` de verdade, nunca pegos por `cargo test --workspace` (71 testes, sempre verde). Diagnóstico exigiu spawn manual repetido do binário com instrumentação temporária. `tests/integration/README.md` já referencia um `harness.sh` nunca construído (débito da feature 001). Testar a UI visualmente é hoje inviável de forma automatizada (Xvfb sem GPU renderiza janela preta; sessão real é Wayland nativo sem ferramenta de screenshot instalada). Escopo: harness de execução real automatizado, cobertura de contrato mais rigorosa a partir dos JSON Schemas normativos, testes de UI/visual (endereçando o problema de tooling em aberto), e CI automatizado via GitHub Actions rodando tudo a cada push/PR."

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Harness detecta automaticamente que o app real não sobe corretamente (Priority: P1)

Quem mantém o Farol quer que qualquer execução do binário real (`farol-core` com plugins reais ou
fixtures controladas) seja verificada automaticamente contra os estados esperados — incluindo o
estado em que um plugin chega a ficar totalmente pronto (`Ready`), não só o momento inicial de
spawn — sem depender de rodar o binário manualmente e ficar observando a saída. Hoje, um defeito
que só se manifesta em runtime real (como um `panic!` do framework de UI ao montar uma inscrição de
eventos) só é descoberto se alguém lembrar de rodar `cargo run` manualmente depois de qualquer
mudança relevante — e mesmo assim, só se a condição de runtime exata que dispara o defeito for
alcançada durante aquela execução manual.

**Why this priority**: é o pilar de maior valor comprovado nesta base — dos três bugs reais que
motivaram esta feature, dois (ambos panics de spawn/execução real) teriam sido pegos por este
harness na primeira execução automatizada, sem exigir a investigação manual que de fato ocorreu.
Sem isso, qualquer outro investimento em testes continua cego para a classe de defeito mais cara já
observada neste projeto.

**Independent Test**: rodar o harness contra o estado atual do repositório e verificar que ele
conclui com sucesso (todos os plugins configurados alcançam os estados esperados, nenhum processo
sai com erro nem trava); introduzir deliberadamente uma regressão equivalente a um dos dois bugs
históricos (closure capturante numa inscrição de eventos) e verificar que o harness falha de forma
clara, apontando o processo e a condição que falhou, sem exigir leitura de log bruto para
diagnosticar.

**Acceptance Scenarios**:

1. **Given** o binário do core compilado e ao menos um plugin de referência configurado (fixture ou
   configuração real controlada), **When** o harness é executado, **Then** ele confirma que o
   processo do core inicia, o(s) plugin(s) alcançam o estado "pronto para uso" esperado, e nenhum
   processo (core ou plugin) sai com erro, trava ou excede um tempo limite definido.
2. **Given** um plugin que deveria alcançar o estado "pronto para uso" só depois de completar um
   ciclo de negociação com o core, **When** o harness é executado, **Then** ele aguarda e confirma
   especificamente esse estado avançado, não apenas o momento inicial de inicialização do processo
   (é esse o estado avançado que escondeu o segundo bug histórico por mais tempo).
3. **Given** uma regressão que faz o processo do core encerrar de forma anormal (crash/panic) sob
   uma condição de runtime específica, **When** o harness é executado e essa condição é alcançada
   durante a execução, **Then** o harness reporta falha explicitamente, identificando qual
   verificação falhou, sem exigir que quem investiga reproduza manualmente a execução para descobrir
   o que aconteceu.
4. **Given** o comportamento correto (nenhuma regressão presente), **When** o harness roda como
   parte do fluxo normal de verificação de uma mudança, **Then** ele conclui em tempo compatível com
   uso frequente (não é um processo que desencoraja rodar a cada mudança).

---

### User Story 2 - Casos de borda dos contratos são verificados automaticamente contra o próprio schema normativo (Priority: P2)

Quem mantém o Farol quer que qualquer valor permitido pelos schemas JSON normativos do protocolo
(incluindo valores de borda como negativos, nulos, ou nos limites de um tipo) seja automaticamente
testado contra a implementação do core que consome esse protocolo — não apenas os exemplos
manualmente escritos que já existem hoje. Hoje, uma divergência silenciosa entre o que o schema
permite e o que a implementação aceita só aparece se alguém, por acaso, escrever à mão um exemplo
de teste que exercite exatamente aquele valor de borda.

**Why this priority**: teria pego, sozinho e sem precisar rodar nada de verdade, o terceiro bug
real que motivou esta feature (um valor sentinela negativo, explicitamente permitido pelo schema,
rejeitado silenciosamente pela implementação e mascarado como um erro completamente diferente) —
segundo maior valor comprovado, mas de natureza estática (mais barata de rodar e mais rápida de
diagnosticar que o harness de execução real).

**Independent Test**: apontar a verificação de contrato para um schema normativo existente e
confirmar que ela exercita automaticamente valores de borda daquele schema (não só os exemplos
manuais pré-existentes); introduzir deliberadamente uma implementação mais restritiva que o schema
permite (por exemplo, um campo que o schema aceita como podendo ser negativo, mas que a
implementação rejeita) e verificar que a verificação de contrato falha, apontando exatamente qual
valor permitido pelo schema não foi aceito pela implementação.

**Acceptance Scenarios**:

1. **Given** um schema normativo do protocolo que declara um campo com um conjunto de valores
   permitidos mais amplo que o que a implementação do core aceita hoje, **When** a verificação de
   contrato roda, **Then** ela falha, identificando o campo e o valor de borda específico que o
   schema permite mas a implementação rejeita.
2. **Given** todos os schemas normativos do protocolo em sua versão corrente, **When** a
   verificação de contrato roda, **Then** cada schema tem pelo menos seus valores de borda
   relevantes (limites de tipo, valores nulos onde permitido, valores negativos onde o schema não
   declara um mínimo) exercitados automaticamente, não apenas por exemplos manuais.
3. **Given** uma mudança futura em um schema normativo (nova versão de protocolo), **When** essa
   mudança é aplicada sem a implementação correspondente ser atualizada, **Then** a verificação de
   contrato detecta a divergência sem exigir que alguém escreva manualmente um novo caso de teste
   para aquele valor específico.

---

### User Story 3 - Verificações rodam automaticamente a cada mudança enviada ao repositório (Priority: P3)

Quem mantém o Farol (e qualquer colaborador futuro) quer que as verificações automatizadas
existentes (testes automatizados, harness de execução real, verificação de contrato, checagem de
estilo/lint) rodem sozinhas a cada push ou pull request, sem depender de alguém lembrar de rodá-las
manualmente antes de considerar uma mudança pronta. Hoje, toda verificação — mesmo a que já existe
(a suíte de testes automatizados já existente) — só roda quando alguém decide rodá-la localmente.

**Why this priority**: multiplica o valor das User Stories 1 e 2 (torna-as parte do fluxo de
trabalho em vez de um passo opcional facilmente esquecido), mas já entrega valor mesmo isolada —
garantir que a suíte de testes já existente hoje (71 testes) e a checagem de estilo rodem sempre,
sem depender de disciplina manual.

**Independent Test**: abrir uma mudança (pull request) contra o repositório e verificar que as
verificações configuradas disparam automaticamente, sem nenhuma ação manual além de abrir a
mudança; introduzir deliberadamente uma quebra (por exemplo, um teste que falha, ou uma violação de
estilo) e verificar que a mudança é sinalizada como não pronta antes que qualquer pessoa precise
rodar algo localmente para descobrir isso.

**Acceptance Scenarios**:

1. **Given** uma mudança enviada ao repositório (push ou pull request), **When** o gatilho automático
   dispara, **Then** a suíte de testes existente, o harness de execução real (User Story 1), a
   verificação de contrato (User Story 2) e a checagem de estilo/lint rodam automaticamente, sem
   intervenção manual.
2. **Given** uma mudança que quebra qualquer uma dessas verificações, **When** o gatilho automático
   roda, **Then** a mudança é sinalizada como não pronta de forma visível a quem a propôs, antes de
   qualquer revisão manual.
3. **Given** uma mudança que passa em todas as verificações, **When** o gatilho automático roda,
   **Then** o resultado fica visível como confirmação de que a mudança não quebrou nada verificável
   automaticamente, sem que ninguém precise rodar as verificações localmente para confiar nisso.

---

### User Story 4 - Regressões visuais na interface são detectadas sem depender de inspeção manual (Priority: P4)

Quem mantém o Farol quer alguma forma de verificar automaticamente que a interface renderizada
continua consistente entre mudanças — hoje isso só é possível abrindo o aplicativo manualmente e
olhando a tela, o que não escala e não é praticável de forma automatizada no ambiente disponível.

**Why this priority**: valor real, mas de natureza e certeza diferentes dos três pilares
anteriores — o problema central desta story não é "que verificação escrever" e sim "o ambiente
consegue produzir uma verificação visual confiável de alguma forma". Por isso vem depois: as outras
três stories entregam valor comprovado e bem-definido primeiro; esta exige investigar e resolver (ou
contornar deliberadamente) uma limitação de ambiente antes de qualquer verificação valer a pena.

**Independent Test**: gerar uma captura (de pixels ou de estrutura declarativa da interface, a
depender de qual abordagem se mostrar viável) de uma tela conhecida do Farol; introduzir uma
mudança deliberada e visível nessa tela; verificar que a comparação contra a captura anterior aponta
a diferença, de forma que uma pessoa revisando a mudança veja exatamente o que mudou visualmente,
sem precisar abrir o aplicativo manualmente.

**Acceptance Scenarios**:

1. **Given** o ambiente de execução disponível (máquina de desenvolvimento local e/ou ambiente sem
   display gráfico interativo), **When** se avalia a viabilidade de captura visual automatizada,
   **Then** existe uma decisão documentada e explícita sobre qual abordagem foi adotada (captura de
   pixels, captura de estrutura declarativa de interface, ou outra) e por quê — nunca a ausência
   silenciosa de qualquer verificação visual sem essa decisão registrada.
2. **Given** a abordagem escolhida está implementada, **When** uma tela conhecida do Farol é
   capturada em duas execuções sem nenhuma mudança relevante entre elas, **Then** a comparação não
   aponta diferença (a captura é estável, não gera alarme falso).
3. **Given** a abordagem escolhida está implementada, **When** uma mudança altera visivelmente uma
   tela já capturada anteriormente, **Then** a comparação aponta a diferença de forma que uma pessoa
   consiga entender o que mudou sem precisar rodar o aplicativo manualmente.
4. **Given** o ambiente sem display gráfico interativo disponível hoje (renderização em preto sob
   Xvfb sem GPU, ausência de ferramenta de captura sob a sessão Wayland real), **When** a
   verificação visual escolhida é integrada ao fluxo de verificações automáticas (User Story 3),
   **Then** ela roda de forma consistente nesse ambiente, ou está explicitamente marcada como
   verificação local/manual-assistida (não bloqueando o gatilho automático) até que o ambiente
   viabilize rodá-la automaticamente.

---

### Edge Cases

- O que acontece quando o harness de execução real (US1) precisa de uma dependência externa
  (instância de um serviço monitorado, por exemplo) que não está disponível no ambiente onde o
  harness roda? O harness precisa distinguir claramente "dependência externa indisponível" (não é
  uma falha do Farol) de "o Farol falhou" — nunca reportar as duas coisas da mesma forma.
- O que acontece quando o harness de execução real trava indefinidamente (nem sucesso, nem falha
  clara) em vez de terminar? Precisa existir um tempo limite explícito após o qual a execução é
  considerada falha, encerrando qualquer processo remanescente.
- O que acontece quando um schema normativo do protocolo (US2) tem uma versão histórica retida (como
  já ocorre hoje com `protocol/schema/v0.1/`, mantida como registro sem implementação ativa)? A
  verificação de contrato precisa deixar claro se está ou não validando essa versão histórica, sem
  criar a falsa impressão de cobertura sobre uma versão que não tem implementação correspondente.
- O que acontece se a verificação automática (US3) demorar tanto que desencoraje o uso frequente do
  fluxo normal de contribuição? Precisa haver um tempo de execução alvo razoável para o conjunto
  completo de verificações.
- O que acontece se a abordagem de teste visual escolhida (US4) só puder rodar localmente (por
  exemplo, por depender de um ambiente gráfico específico não disponível no ambiente de CI)? Isso
  precisa ser uma decisão explícita e documentada, não uma lacuna silenciosa — e não deve impedir
  que as demais verificações (US1–US3) continuem rodando automaticamente.
- O que acontece quando alguém introduz um novo tipo de mensagem ou plugin no protocolo depois que
  esta infraestrutura existir? O harness (US1) e a verificação de contrato (US2) precisam continuar
  cobrindo esse novo caso sem exigir reconstrução da infraestrutura do zero — a infraestrutura é
  extensível, não um artefato de uso único desta feature.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: O sistema MUST fornecer uma forma automatizada de iniciar o binário real do core com
  plugin(s) reais ou fixtures controladas e verificar programaticamente se os estados esperados de
  cada plugin (incluindo o estado avançado de "pronto para uso", não apenas o de inicialização) são
  alcançados dentro de um tempo limite definido.
- **FR-002**: O sistema MUST detectar e sinalizar como falha qualquer encerramento anormal (crash,
  panic) do processo do core ou de um plugin durante a execução do harness de execução real.
- **FR-003**: O sistema MUST encerrar automaticamente qualquer processo remanescente ao final da
  execução do harness de execução real, com sucesso ou falha, sem deixar processos órfãos.
- **FR-004**: O sistema MUST reportar a falha do harness de execução real de forma que identifique
  qual verificação específica falhou (qual estado esperado não foi alcançado, ou qual processo saiu
  de forma anormal), sem exigir que quem investiga reproduza manualmente a execução para entender o
  que aconteceu.
- **FR-005**: O sistema MUST gerar ou exercitar automaticamente valores de borda (mínimos, máximos,
  nulos onde permitido, negativos onde o tipo não declara um mínimo) derivados diretamente dos
  schemas JSON normativos correntes do protocolo, além dos exemplos manuais já existentes.
- **FR-006**: O sistema MUST falhar a verificação de contrato quando a implementação do core rejeita
  um valor que o schema JSON normativo correspondente permite explicitamente.
- **FR-007**: O sistema MUST rodar, a cada push e a cada abertura ou atualização de pull request no
  repositório, sem intervenção manual: a suíte de testes automatizados já existente, o harness de
  execução real (FR-001–FR-004), a verificação de contrato (FR-005–FR-006), e a checagem de
  estilo/lint já em uso em cada componente do projeto.
- **FR-008**: O sistema MUST sinalizar de forma visível, associada à mudança que a originou, quando
  qualquer uma das verificações automáticas (FR-007) falha — antes de qualquer revisão manual dessa
  mudança.
- **FR-009**: O sistema MUST registrar uma decisão explícita e documentada sobre a abordagem adotada
  para verificação visual/de interface (captura de pixels, captura de estrutura declarativa, ou
  outra), incluindo a limitação de ambiente que motivou a escolha — a ausência de qualquer
  verificação visual não é uma opção silenciosa aceitável.
- **FR-010**: Quando a abordagem de verificação visual escolhida (FR-009) produz uma captura de uma
  tela conhecida do Farol, o sistema MUST permitir comparar essa captura contra uma versão anterior
  e apontar quando há diferença.
- **FR-011**: Se a abordagem de verificação visual escolhida (FR-009) não puder rodar de forma
  confiável no mesmo ambiente automático das demais verificações (FR-007), o sistema MUST deixar
  isso explícito (por exemplo, como verificação local/manual-assistida) em vez de silenciosamente
  não rodar ou bloquear as demais verificações por causa dela.
- **FR-012**: O harness de execução real (FR-001) MUST distinguir claramente, no relato de falha,
  entre uma dependência externa indisponível no ambiente de execução e uma falha real do Farol —
  nunca reportar as duas condições de forma indistinguível.
- **FR-013**: A infraestrutura descrita nesta feature (harness, verificação de contrato, gatilho de
  CI, verificação visual) MUST ser estruturada de forma que cobrir um novo plugin, um novo tipo de
  mensagem de protocolo, ou uma nova versão de schema não exija reconstruir a infraestrutura — apenas
  estendê-la (por exemplo, adicionar uma nova fixture ou um novo cenário, não reescrever o
  mecanismo).
- **FR-014**: O tempo de execução do conjunto completo de verificações automáticas (FR-007) MUST
  permanecer em uma faixa que não desencoraje seu uso a cada mudança proposta (referência prática:
  minutos, não dezenas de minutos).

### Key Entities

- **Harness de execução real**: mecanismo que sobe o binário real do core (com plugin(s) real(is) ou
  fixture(s) controlada(s)) e afirma programaticamente que estados de runtime esperados (incluindo
  estados avançados como "plugin pronto para uso") são alcançados sem falha de processo, dentro de
  um tempo limite.
- **Fixture de execução**: configuração e/ou plugin controlado usado pelo harness para produzir um
  cenário determinístico (por exemplo, um plugin de teste que sempre alcança "pronto para uso" sem
  depender de um serviço externo real).
- **Caso de borda de contrato**: valor de um campo do protocolo, derivado do próprio schema JSON
  normativo (não escrito à mão como exemplo isolado), usado para verificar que a implementação do
  core
  aceita tudo o que o schema permite.
- **Verificação automática de mudança (CI)**: conjunto de verificações (testes existentes, harness,
  contrato, lint) disparado automaticamente a cada push/pull request, cujo resultado fica associado
  à mudança que o originou.
- **Captura visual de referência**: registro (de pixels ou de estrutura declarativa de interface,
  conforme a decisão de FR-009) de uma tela conhecida do Farol, usado como base de comparação para
  detectar regressão visual em mudanças futuras.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: Uma regressão equivalente aos dois panics de execução real que motivaram esta feature
  (fechamento capturante numa inscrição de eventos de runtime, incluindo o caso que só se manifesta
  quando um plugin alcança o estado avançado "pronto para uso") é detectada pelo harness de execução
  real antes de chegar à revisão de uma mudança, sem exigir investigação manual com instrumentação
  temporária.
- **SC-002**: Uma regressão equivalente ao terceiro bug que motivou esta feature (implementação mais
  restritiva que um valor explicitamente permitido pelo schema normativo do protocolo) é detectada
  pela verificação de contrato, de forma estática, sem exigir rodar o binário real.
- **SC-003**: 100% das mudanças enviadas ao repositório (push ou pull request) disparam
  automaticamente o conjunto completo de verificações (testes existentes, harness, contrato, lint),
  sem exigir que quem propõe a mudança rode qualquer coisa manualmente antes.
- **SC-004**: O conjunto completo de verificações automáticas conclui em até 10 minutos para uma
  mudança típica, mantendo o fluxo de contribuição prático.
- **SC-005**: Existe uma forma automatizada (não manual, não dependente de abrir o aplicativo e
  olhar a tela) de detectar pelo menos uma classe de regressão visual da interface do Farol, com a
  abordagem escolhida e sua justificativa documentadas explicitamente.
- **SC-006**: Depois desta feature, diagnosticar uma futura classe de defeito equivalente às três que
  a motivaram (falha só em execução real, divergência de contrato em valor de borda) não exige mais
  o processo manual de spawn repetido com instrumentação temporária que foi necessário durante a
  feature 002 — a mesma classe de defeito é pega automaticamente antes da revisão manual.

## Assumptions

- O harness de execução real (US1) pode depender de fixtures controladas (plugin(s) de teste
  determinístico(s)) além de, ou no lugar de, plugins de referência reais — não é obrigatório que
  todo cenário do harness dependa de um serviço externo de verdade (por exemplo, uma instância real
  de um serviço monitorado); quando um cenário depender de serviço externo real, isso é uma decisão
  explícita, documentada como tal.
- "Casos de borda dos contratos" (US2) refere-se aos schemas JSON normativos correntes do protocolo
  (a versão em uso pela implementação ativa); schemas de versões históricas retidas como registro
  (sem implementação ativa) não são obrigados a ganhar a mesma cobertura de borda, exceto onde já
  fizerem parte da verificação de compatibilidade entre versões existente.
- O ambiente de CI (US3) é hospedado pela mesma plataforma onde o repositório já está hospedado
  hoje — não é assumida a introdução de uma plataforma de CI adicional além da já usada para hospedar
  o código-fonte.
- Para a verificação visual (US4), é aceitável que a abordagem final não seja captura de pixels —
  uma verificação declarativa (por exemplo, snapshot da árvore/estado de widgets que alimenta a
  renderização, sem depender de captura de tela) conta como satisfazendo o requisito, desde que
  detecte mudanças visíveis relevantes e a escolha esteja documentada com sua justificativa.
- Esta feature cobre a infraestrutura de teste em si (harness, verificação de contrato mais
  rigorosa, verificação visual, gatilho de CI) — não é escopo desta feature corrigir nenhum defeito
  de produto adicional que essas verificações venham a revelar depois de existirem; cada defeito
  revelado é tratado como um item separado (correção direta ou débito técnico rastreado, conforme a
  governança do projeto).
- Reproduzir os três bugs históricos especificamente como "teste de regressão" automatizado
  (garantir que aquele bug exato nunca reapareça) é resultado esperado e desejável desta feature,
  mas o critério de sucesso central é a capacidade geral de pegar a *classe* de defeito (execução
  real, valor de borda de contrato), não apenas os três casos específicos já corrigidos
  manualmente na feature 002.
