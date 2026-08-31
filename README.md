# Farol

> Plano de controle pessoal para a máquina do dev — nativo, modular, extensível por plugins.

**Status: em idealização.** Ainda não há código funcional. Este repositório existe para consolidar a visão do projeto antes da implementação.

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

- **App nativo Linux**, sem navegador.
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

1. **Protótipo monolítico** — os plugins acima direto no código, sem protocolo externo ainda. Objetivo: uso diário real.
2. **Extrair o protocolo** — os mesmos plugins viram processos externos falando JSON-RPC.
3. **Sandbox e permissões** — manifesto de capacidades, isolamento via bubblewrap, vault de segredos.
4. **Registry** — repo-índice, CI de validação, instalação in-app, template de plugin.
5. **Polimento social** — espaços exportáveis, temas, galeria de plugins.

## Decisões em aberto

- Stack do core: Python (iterar rápido) vs. Rust com GTK4/libadwaita (distribuir cedo). Provável caminho: protótipo em Python, core reescrito em Rust quando a API de plugin estabilizar.
- Ações privilegiadas (ex: VPN exige root): `sudo` sob demanda, polkit, ou daemon auxiliar.
- Farol roda sempre em background (tray icon, notificações) ou só quando aberto.
- Escopo do v1 público: só dev/homelab, ou aberto a outros domínios desde já.

## Contribuindo

Ainda não há processo formal de contribuição — o projeto está na fase de definir o contrato de plugin antes de abrir para a comunidade. Acompanhe as issues deste repositório para o andamento.

## Licença

A definir.
