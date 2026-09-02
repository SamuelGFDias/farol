# Farol

> Plano de controle pessoal para a máquina do dev — nativo, modular, extensível por plugins.

**Status: núcleo funcional.** Três features completas (core Rust + iced, protocolo de plugin JSON-RPC provado por dois plugins de referência reais — `git-local` e `uptime-kuma` — e uma suíte de testes automatizada, incluindo harness de smoke do binário real). Ainda sem sandbox, sem registry e sem empacotamento (fases 3–5 do roadmap abaixo). As decisões de arquitetura estão formalizadas numa constitution versionada em [`.specify/memory/constitution.md`](.specify/memory/constitution.md), e o projeto adota spec-driven development (spec-kit).

---

## O que é

Farol não é um dashboard. Um dashboard só mostra. Farol faz três coisas, e todo plugin existe pra servir uma delas:

| Verbo | Resolve | Exemplo |
|---|---|---|
| **Ver** | qual o estado das minhas coisas agora? | serviços no Uptime Kuma, repos sujos, VPN conectada |
| **Agir** | mudar esse estado sem trocar de janela | conectar VPN, `git pull`, reiniciar container |
| **Lembrar** | o que está pendente pra mim? | issues atribuídas, PRs esperando review |

A dor: hoje o estado da sua vida técnica está espalhado em várias abas, terminais e apps. Farol é a janela única que responde "tá tudo bem? o que eu preciso fazer?" em segundos.

## Princípios de arquitetura

- **App nativo Linux**, sem navegador. Core em **Rust + iced** (GUI de arquitetura Elm/Model-Update-View): puro Rust, sem binding contra GTK/Qt do sistema, o que dá binário estático e boa compatibilidade cross-distro, sem depender da versão de toolkit gráfico instalada em cada distro.
- **Plugins são processos separados**, falando JSON-RPC com o core via stdin/stdout — mesmo modelo do LSP/MCP. Qualquer linguagem, isolamento de crash, sandbox real.
- **Plugins descrevem widgets, não desenham.** Devolvem dados declarativos (`status-grid`, `lista`, `métrica`...); o core renderiza. Mantém a UI consistente e segura, e permite trocar de toolkit no futuro sem quebrar plugin nenhum.
- **Permissões explícitas por manifesto.** Rede por allowlist de host, segredos no keyring do sistema, execução de comando (`exec`) como capacidade sinalizada. Plugin de terceiro roda sem confiança cega.
- **Espaços** (workspaces): cada contexto — Trabalho, Pessoal, Homelab — tem seu próprio layout e plugins ativos.
- **Paleta de comandos (`Ctrl+K`)** agrega toda ação de todo plugin ativo.
- **Registry no GitHub**, sem infra própria: um repo-índice, publicação via PR, instalação puxando releases direto do repo do plugin.

## Integrações previstas

- [`openfortivpn-gui`](https://github.com/SamuelGFDias/OpenFortVPN-gui) — conectar/desconectar VPN, status, log.
- **Uptime Kuma** — status de monitores via endpoint `/metrics` (Prometheus).
- **Git local** — repos em `~/dev` com mudanças pendentes, ahead/behind.
- **GitHub** — issues atribuídas, PRs aguardando review.
- **Docker** — containers up/down, logs.

## Roadmap

1. **Walking skeleton** ✅ (concluído) — fatia vertical com core em Rust + iced, protocolo de plugin JSON-RPC e um plugin de referência (Git local), provando o contrato de plugin ponta a ponta com um consumidor real.
2. **Ampliar cobertura de plugins** (em andamento) — os demais plugins da seção "Integrações previstas" (VPN, Uptime Kuma, GitHub, Docker) sobre o protocolo já provado no walking skeleton. Uptime Kuma já é um plugin funcional; VPN, GitHub e Docker seguem pendentes.
3. **Sandbox e permissões** — manifesto de capacidades, isolamento via bubblewrap, vault de segredos.
4. **Registry** — repo-índice, CI de validação, instalação in-app, template de plugin.
5. **Polimento social** — espaços exportáveis, temas, galeria de plugins.

## Decisões em aberto

- Ações privilegiadas (ex: VPN exige root): `sudo` sob demanda, polkit, ou daemon auxiliar.
- Farol roda sempre em background (tray icon, notificações) ou só quando aberto.
- Escopo do v1 público: só dev/homelab, ou aberto a outros domínios desde já.
- Empacotamento/distribuição: Flatpak vs. AppImage vs. binário estático.

## Contribuindo

Ainda não há processo formal de contribuição — o projeto está na fase de definir o contrato de plugin antes de abrir para a comunidade. Para entender as regras do projeto, veja a [constitution](.specify/memory/constitution.md). Acompanhe as issues deste repositório para o andamento.

## Licença

A definir.
