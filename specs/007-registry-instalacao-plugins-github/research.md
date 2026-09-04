# Research: Registry — Descoberta e Instalação de Plugins de Terceiros via GitHub

**Feature**: `007-registry-instalacao-plugins-github` | **Data**: 2026-09-04

## D1: Formato do manifesto — `farol-plugin.toml`, TOML plano espelhando `SandboxProfile`

**Decisão**: novo arquivo `farol-plugin.toml`, na raiz do código-fonte de um plugin, formato:

```toml
plugin_name = "exemplo-plugin"
command = "python3"
args = ["main.py"]

[capabilities]
network = false
exec = true
```

**Rationale**: em vez de espelhar a forma aninhada do `CapabilityManifest` do protocolo
(`{"capabilities": [{"kind": "exec"}]}`, uma lista de variantes de enum), o manifesto local usa
booleanos planos (`network`/`exec`) que mapeiam 1:1 para os campos já existentes de
`sandbox::SandboxProfile` (`allow_network`/`allow_exec`, feature 006) — conversão direta, sem
tradução de formato lista-de-enum-para-booleano. Campos desconhecidos no manifesto MUST ser
ignorados na desserialização (não usar `deny_unknown_fields`), consistente com a tolerância a campo
desconhecido já praticada pelo protocolo JSON-RPC (Edge Cases de `spec.md`).

## D2: Diretório de dados — `$XDG_DATA_HOME/farol/plugins/<nome>/`, distinto de `$XDG_CONFIG_HOME`

**Decisão**: novo módulo (ou função) resolvendo o diretório base de dados de plugins instalados,
mesma convenção XDG já usada por `config_store::farol_config_base_dir()` (feature 002), mas para
`XDG_DATA_HOME` (fallback `~/.local/share`) em vez de `XDG_CONFIG_HOME` (fallback `~/.config`):

```
farol_data_base_dir() = $XDG_DATA_HOME/farol  (fallback ~/.local/share/farol)
installed_plugin_dir(nome) = farol_data_base_dir()/plugins/<nome>/
```

**Rationale**: `~/.config/farol` já é usado por `config_store`/`secrets_store` para *configuração*
do usuário (valores de `required_config`, segredos) — guardar ali também o *código-fonte instalado*
de um plugin de terceiro misturaria duas categorias de dado com ciclo de vida e sensibilidade
diferentes (configuração é pequena e editável à mão; código instalado é maior e gerado por
download). A convenção XDG já distingue exatamente essas duas categorias
(`XDG_CONFIG_HOME` vs. `XDG_DATA_HOME`) — usar o diretório certo para cada uma é a escolha
padrão do ecossistema Linux, não uma decisão nova deste projeto.

## D3: `known_plugins()` continua intocado; descoberta é uma função nova, somada no único ponto de montagem

**Decisão**: `plugin_worker::known_plugins()` (os 4 plugins de referência, hardcoded) **não muda**.
Uma função nova, `plugin_worker::discover_installed_plugins() -> Vec<PluginSpawnConfig>`, escaneia
`installed_plugin_dir` (D2), lê cada `<nome>/farol-plugin.toml` (D1), valida (D7) e devolve os
`PluginSpawnConfig` correspondentes. O único ponto de montagem do `Farol` real
(`main.rs::Farol::default`, hoje `Self::with_plugins(plugin_worker::known_plugins())`) passa a somar
as duas listas, filtrando colisão de nome (D6) antes de montar os slots.

**Rationale**: preserva 100% do comportamento e dos testes já existentes dos 4 plugins de referência
(nenhum deles depende de manifesto ou de filesystem fora do repo) — a única mudança de
comportamento observável é a lista ficar maior quando existem plugins instalados. Consistente com
SC-002 (regressão zero).

## D4: `PluginSpawnConfig` ganha `code_root: PathBuf` — generaliza o bind de filesystem do sandbox

**Decisão**: `PluginSpawnConfig` (já com `sandbox_profile` desde a feature 006) ganha um campo novo,
`code_root: PathBuf` — o diretório que o sandbox bind-monta como código do plugin e usa como
`--chdir` (`sandbox::build_bwrap_args`, hoje recebe `repo_root` calculado dentro de `worker()` via
`env!("CARGO_MANIFEST_DIR")`, feature 006 D8). Os 4 plugins de referência (`known_plugins()`) passam
a preencher esse campo com a raiz do repositório Farol (mesmo valor de hoje, calculado uma vez);
plugins descobertos (`discover_installed_plugins()`) preenchem com o próprio diretório onde foram
instalados (`installed_plugin_dir(nome)`, D2). `worker()` passa a usar `config.code_root`
diretamente em vez de calcular `repo_root` internamente — mudança mínima, mesmo comportamento para
os 4 plugins de referência.

**Rationale**: bindar a raiz inteira do repositório Farol como "código do plugin" (D4 de
`specs/006-sandbox-permissoes-bubblewrap/research.md`) fazia sentido quando só existiam os 4
plugins de referência, que de fato vivem lá — não faz sentido para um plugin de terceiro instalado
em `~/.local/share/farol/plugins/<nome>/`, que não tem nada a ver com o repositório Farol. Cada
plugin passa a enxergar, no sandbox, só o próprio código — mais estrito que antes para os plugins
instalados (não conseguem ler o código-fonte de outros plugins nem do core), sem enfraquecer nada
para os 4 de referência (continuam vendo exatamente o que viam).

## D5: Instalação — `farol install <owner>/<repo>` via subcommand de CLI, `curl`+`tar`, sem dependência Rust nova

**Decisão**: `main()` verifica `std::env::args()` antes de montar o `iced::application` — se o
primeiro argumento for `install` e o segundo for `<owner>/<repo>`, executa o fluxo de instalação
síncrono (função nova, ex. `install::run(owner, repo)`) e sai com o código de saída apropriado, sem
abrir janela nenhuma. Fluxo, validado empiricamente nesta sessão contra a API real do GitHub:

1. `curl -sL https://api.github.com/repos/<owner>/<repo>/releases/latest` → parseia o JSON (via
   `serde_json`, já dependência do workspace) por `tag_name` e `tarball_url`. 404 ⟹ "nenhuma release
   encontrada" (Edge Case, FR-010).
2. `curl -sL <tarball_url> -o <tmp>/release.tar.gz` — baixa para um diretório temporário.
3. `tar -xzf <tmp>/release.tar.gz -C <tmp>/staging --strip-components=1` — o tarball de código-fonte
   que o GitHub gera automaticamente sempre tem um único diretório-raiz (`<repo>-<sha>/`);
   `--strip-components=1` remove essa camada, deixando o conteúdo real na raiz de `staging/`.
4. Valida `staging/farol-plugin.toml` (existe, parseia, `plugin_name` não colide com um dos 4 de
   referência — mesma validação de D7/FR-005, checada também aqui para falhar cedo).
5. Rename atômico de `staging/` para `installed_plugin_dir(plugin_name)` (removendo o que já
   existisse ali antes — FR-008, substituição limpa) — só depois de tudo validado, nunca antes
   (FR-007, sem estado parcial visível).

**Validado empiricamente nesta sessão** (repositório real `jqlang/jq`, só para provar o mecanismo —
não é um plugin Farol de verdade): `curl` contra `/releases/latest` devolve `tag_name`/`tarball_url`
corretamente; `curl -sL <tarball_url> -o arquivo` baixa o tarball; `tar -xzf ... --strip-components=1`
extrai para um diretório plano sem a camada `<repo>-<sha>/` no meio.

**Rationale (sem dependência Rust nova)**: `curl`/`tar` já são assumidos disponíveis por convenção
implícita em qualquer distro Linux alvo do projeto (Princípio I) — adicionar um cliente HTTP Rust
(`reqwest`, que traz `tokio`/TLS/etc. como sub-dependências pesadas) só para este único caso de uso
não se paga; mesma disciplina de minimalismo de dependência já seguida pelas features 001-006
(nenhuma delas adicionou dependência nova ao `farol-core`/`farol-protocol`).

## D6: Colisão de nome — plugin de referência sempre vence; entre dois instalados, o primeiro na ordem de varredura

**Decisão**: ao montar a lista final (`main.rs::Farol::default`), um `plugin_name` descoberto que já
exista entre os 4 de referência é descartado com um aviso (`eprintln!`, mesma disciplina de
diagnóstico já usada no projeto — sem infraestrutura de log nova). Entre dois plugins descobertos
com o mesmo nome (caso patológico, não deveria ocorrer em uso normal), mantém o primeiro encontrado
pela ordem de `std::fs::read_dir` (determinística dentro de uma mesma execução, não garantida entre
execuções diferentes do SO — aceitável, é um caso de erro do usuário, não um caminho a otimizar).

## D7: Validação de manifesto compartilhada entre descoberta (US1) e instalação (US2)

**Decisão**: uma função só, `parse_manifest(path: &Path) -> Result<PluginManifest, ManifestError>`,
usada tanto por `discover_installed_plugins()` quanto pelo fluxo de instalação (D5, passo 4) — evita
duas implementações divergentes do mesmo contrato. `ManifestError` cobre: arquivo ausente, TOML
inválido, campo obrigatório ausente (`plugin_name`/`command`/`args`), `plugin_name` vazio.

## D8: Estratégia de teste — servidor de fixture local para a API do GitHub, mesmo padrão de `MetricsFixtureServer`

**Decisão**: o fluxo de instalação (D5) aceita um `base_url` configurável para a API do GitHub via
variável de ambiente (`FAROL_GITHUB_API_BASE`, default `https://api.github.com` quando ausente) —
mesmo padrão de escape-hatch só-de-teste já usado em `FAROL_SANDBOX_TEST_EXTRA_BIND` (feature 006,
D14). Testes automatizados sobem um servidor HTTP local (`TcpListener`, mesmo padrão de
`MetricsFixtureServer` da feature 002) que serve uma resposta JSON de `/repos/<owner>/<repo>/
releases/latest` sintética e um tarball de teste construído em runtime (com `tar`/`gzip` reais,
gerado pelo próprio teste) — cobre os cenários de sucesso e cada uma das falhas de Edge Cases (sem
release, manifesto ausente/inválido, nome colidindo) sem depender de rede externa nem de nenhum
repositório GitHub real continuar existindo. Um cenário manual único (`quickstart.md`) valida contra
o GitHub real, mesma disciplina de "automação hermética + validação manual pontual" já usada pelas
features 002/004/005 para os casos que envolvem rede/serviço externo de verdade.

## D9: Rate limit não-autenticado da API do GitHub — aceito como limitação documentada, sem mitigação nesta fase

**Decisão**: nenhuma autenticação (token) nesta fase — 60 requisições/hora por IP (confirmado
empiricamente contra a API real nesta sessão), suficiente para uso pessoal esporádico
(`farol install` não é uma operação de alta frequência). Autenticação para aumentar o limite ou
acessar repositório privado fica fora de escopo (`spec.md`, já registrado).
