<!--
Sync Impact Report
- Version change: 1.0.0 → 1.1.0 (amendment)
- Bump rationale: MINOR — expansão material de uma seção existente ("## Governance"), conforme a
  própria política de versionamento já escrita nessa seção: "MINOR: adição de novo princípio ou
  expansão material de uma seção existente". Não é MAJOR (nenhum princípio existente é removido ou
  redefinido de forma incompatível) nem PATCH (não é mero esclarecimento de redação — é uma regra
  de governança nova, com obrigação MUST própria).
- Added sections:
  - "## Governance" ganha um novo bullet, "README como fonte de verdade externa, mantido
    atualizado", no mesmo estilo dos bullets já existentes (Emendas, Versionamento, Revisão de
    conformidade, Dívida técnica rastreável). Não é um novo Core Principle (I-VII) — é regra de
    processo/higiene documental, não princípio de produto sobre os verbos Ver/Agir/Lembrar.
- Modified principles: nenhum (Core Principles I-VII inalterados).
- Modified sections: nenhuma além da adição acima.
- Removed sections: nenhuma.
- Templates requiring follow-up: nenhum — o README está sendo atualizado em paralelo por outra
  tarefa desta mesma sessão, não depende desta emenda para isso.
- Deferred / TODO placeholders: nenhum.
-->

# Farol Constitution

## Propósito do Produto

Farol é um plano de controle pessoal para a máquina do dev — nativo, modular, extensível por
plugins. Farol NÃO é um dashboard: ele resolve três verbos para quem o usa.

- **Ver**: expor o estado atual do ambiente de trabalho sem que o usuário precise abrir N janelas
  ou terminais para descobrir o que está acontecendo.
- **Agir**: permitir mudar esse estado (executar uma ação, disparar um comando, alternar algo)
  sem trocar de janela.
- **Lembrar**: manter visível o que está pendente, para que nada dependa da memória do usuário.

Toda decisão de produto e toda funcionalidade nova DEVEM ser avaliadas contra esses três verbos.
Uma proposta de feature que não sirva a nenhum deles é candidata a rejeição ou a outro produto.

## Core Principles

### I. Nativo e Sem Navegador

Farol é um aplicativo nativo para Linux. Ele NÃO é executado dentro de um navegador nem embute um
motor de navegador (sem Electron/Chromium embarcado, sem shell web) como caminho principal de
entrega. Rationale: o produto compete em espaço de recursos e responsividade com o restante da
máquina do desenvolvedor; um motor de navegador embutido contradiz a proposta de "plano de
controle" leve e sempre disponível.

### II. Plugins como Processos Isolados via JSON-RPC

Todo plugin roda como um processo separado do core, falando JSON-RPC com o core via stdin/stdout —
o mesmo modelo usado por LSP e MCP. Isso é NON-NEGOTIABLE para qualquer plugin, interno ou de
terceiro. Consequências obrigatórias desse modelo:

- Um plugin PODE ser escrito em qualquer linguagem, desde que fale o protocolo JSON-RPC do core.
- Um crash de plugin PRECISA ficar isolado — não pode derrubar o core nem outros plugins.
- O isolamento por processo é a base do sandboxing; nenhum plugin recebe acesso direto ao espaço
  de memória do core.

Rationale: isolar por processo (em vez de biblioteca dinâmica carregada em processo) é o único
jeito de garantir simultaneamente compatibilidade poliglota, tolerância a falha e uma superfície
de sandbox real de plugin de terceiro não confiável.

### III. Widgets Declarativos, Core Renderiza

Plugins DESCREVEM widgets, eles não os desenham. Um plugin devolve dados declarativos (ex.:
status-grid, lista, métrica) e é o core — nunca o plugin — quem renderiza a UI. Um plugin MUST NOT
emitir markup, pixels ou instruções de desenho de baixo nível; ele expõe apenas o modelo de dados
do widget.

Rationale: manter a renderização inteiramente do lado do core garante uma UI visualmente
consistente entre plugins de autores diferentes, fecha uma superfície de ataque óbvia (plugin de
terceiro não desenha arbitrariamente na tela do usuário) e permite trocar o toolkit de UI do core
no futuro sem quebrar contrato nenhum de plugin existente.

### IV. Permissões Explícitas por Manifesto

Todo plugin declara suas permissões em um manifesto explícito, e o core NÃO concede acesso além do
que está declarado. No mínimo, o manifesto cobre:

- **Rede**: acesso de rede restrito a uma allowlist de hosts declarada; sem acesso de rede
  irrestrito por padrão.
- **Segredos**: quando um plugin precisa de credenciais, o valor vem de um armazenamento dedicado
  gerenciado exclusivamente pelo core — nunca de arquivo de configuração em texto plano gerenciado
  pelo próprio plugin, e o plugin NUNCA lê nem escreve esse armazenamento diretamente. Hoje esse
  armazenamento é um arquivo dedicado com permissão de sistema de arquivos restrita (`0600`), fora
  do `config.toml` do plugin; o mecanismo concreto PODE evoluir no futuro (por exemplo, para o
  keyring do sistema operacional) sem mudar o contrato com o plugin, que sempre recebe o valor já
  resolvido pelo core como variável de ambiente no spawn do processo filho — nunca por acesso
  direto ao armazenamento.
- **Execução de comando (exec)**: rodar processos externos é uma capacidade sinalizada à parte,
  não implícita em nenhuma outra permissão.

Rationale: um plugin de terceiro roda sem confiança cega. O manifesto de permissões é o contrato
de segurança entre o usuário e cada plugin instalado, e é o que torna o modelo de isolamento do
Princípio II acionável pelo usuário (ele decide o que cada plugin pode tocar).

### V. Espaços (Workspaces) por Contexto

Cada contexto de uso do usuário — por exemplo Trabalho, Pessoal, Homelab — é um espaço (workspace)
com seu próprio layout e seu próprio conjunto de plugins ativos. Um plugin ativo em um espaço não
é necessariamente ativo em outro, e o layout de um espaço não vaza para outro.

Rationale: os três verbos do produto (Ver, Agir, Lembrar) têm respostas diferentes dependendo do
contexto em que o usuário está; um único layout global misturaria sinais de contextos que o
usuário quer manter separados.

### VI. Paleta de Comandos Universal

A paleta de comandos (Ctrl+K) agrega toda ação exposta por todo plugin ativo no espaço corrente,
num único ponto de entrada por teclado. Uma ação de plugin que não pode ser alcançada pela paleta
de comandos está incompleta.

Rationale: é o mecanismo central do verbo "Agir" — o usuário muda o estado do sistema sem trocar
de janela nem caçar o botão certo em cada widget.

### VII. Registry Federado sem Infra Própria

A descoberta e distribuição de plugins usa o GitHub como plataforma, sem infraestrutura própria do
Farol para hospedar pacotes: um repositório-índice central, publicação de novos plugins via pull
request nesse índice, e instalação puxando releases diretamente do repositório de cada plugin.

Rationale: manter a distribuição sobre infraestrutura já existente e amplamente confiável (GitHub)
reduz o custo operacional do projeto a zero servidor próprio e mantém o processo de publicação
auditável via PR.

## Stack e Estágio do Projeto

O projeto está em estágio de idealização. O core MUST ser implementado em Rust usando iced,
biblioteca de GUI Rust de arquitetura Elm (Model-Update-View), desde o protótipo — não há fase de
protótipo monolítico em outra linguagem no roadmap; reescrever o core mais tarde é evitado ao
nascer direto em Rust + iced.

A escolha de iced se apoia em:

- **Precedente de escala real**: o COSMIC, ambiente de área de trabalho do Pop!_OS/System76, é
  construído em iced, o que reduz o risco de a biblioteca ser abandonada ou não aguentar a
  complexidade que o Farol vai ganhar (paleta de comandos, múltiplos espaços, widgets dinâmicos).
- **Encaixe direto com o Princípio III** ("plugins descrevem widgets, core renderiza"): a
  arquitetura Model-Update-View do iced é puro Rust reativo — o core mantém um Model com a árvore
  de widgets declarativos recebida de cada plugin via JSON-RPC, e a função de view traduz isso em
  elementos de UI em runtime, sem precisar de DSL compilada à parte (diferente de Slint) e sem o
  atrito de widgets dinâmicos de terceiros em cima de immediate-mode (diferente de egui).
- **Licença MIT** (permissiva), sem risco de contaminação de licença para quem for escrever
  plugin ou consumir o core.
- **Puro Rust, sem binding contra GTK/Qt do sistema**: binário estático, boa compatibilidade
  cross-distro, sem depender da versão de toolkit gráfico instalada em cada distro.

## Governance

Esta constitution tem precedência sobre qualquer prática, template ou convenção de código do
projeto Farol que a contradiga. Em caso de conflito entre esta constitution e outro documento do
repositório, esta constitution prevalece até que seja formalmente emendada.

- **Emendas**: qualquer mudança nesta constitution (adição, remoção ou redefinição de princípio,
  ou mudança de seção de governança) é uma emenda e PRECISA vir acompanhada de justificativa
  registrada no Sync Impact Report no topo deste arquivo.
- **Versionamento**: esta constitution segue versionamento semântico dedicado
  (MAJOR.MINOR.PATCH):
  - MAJOR: remoção ou redefinição incompatível de um princípio existente.
  - MINOR: adição de novo princípio ou expansão material de uma seção existente.
  - PATCH: esclarecimento de redação, correção de erro, refinamento não semântico.
- **Revisão de conformidade**: features, planos e tasks gerados pelo toolkit Spec Kit para este
  projeto DEVEM ser verificados contra os princípios acima antes de serem considerados prontos
  para implementação; qualquer desvio precisa de justificativa explícita no artefato correspondente
  (spec, plano ou task), não de exceção silenciosa.
- **Dívida técnica rastreável**: dívida técnica identificada durante o desenvolvimento e
  deliberadamente deixada sem correção imediata (ex.: workaround documentado em comentário,
  decisão consciente de adiar um ajuste, limitação conhecida de uma dependência) MUST ser
  registrada como issue no tracker do projeto (GitHub Issues) antes de a mudança correspondente
  ser considerada concluída. Comentário de código ou nota de sessão, isoladamente, NÃO substituem
  o registro rastreável. Rationale: dívida técnica que existe só em comentário ou em memória de
  sessão desaparece do radar do projeto assim que a sessão termina ou o comentário para de ser
  lido; uma issue no tracker é o único registro que sobrevive à sessão que a criou e que pode
  entrar em backlog, milestone ou priorização futura.
- **README como fonte de verdade externa, mantido atualizado**: o `README.md` é o documento
  voltado para quem chega ao projeto de fora — status atual do produto, roadmap, integrações
  previstas. MUST ser atualizado sempre que uma mudança de sessão alterar seu conteúdo de forma
  material: fase do roadmap concluída, novo plugin de referência funcional, mudança relevante na
  linha de status do projeto. Rationale: ao contrário do `AGENTS.md` (contexto interno para quem
  já trabalha no código) ou desta constitution (princípios formais e estáveis), o README é o que
  alguém lê antes de decidir se vale a pena explorar o projeto — um README desatualizado (ex.:
  alegando "ainda sem código funcional" quando já existem features completas) passa uma imagem
  falsa do estado real do projeto, tanto para colaboradores externos quanto para o próprio autor
  revisitando o projeto depois de um tempo.

**Version**: 1.1.0 | **Ratified**: 2026-08-31 | **Last Amended**: 2026-09-02
