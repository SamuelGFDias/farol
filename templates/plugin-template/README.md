# Template de plugin Farol

Ponto de partida mínimo para escrever um plugin novo do Farol. Implementa só o
handshake (`handshake/hello`), sem widgets nem ações — o suficiente para o
Farol reconhecer o plugin e chegar a `Ready`.

## Como usar

1. **Copie este diretório inteiro** para um lugar novo (fora do repositório do
   Farol), com o nome do seu plugin:

   ```sh
   cp -r templates/plugin-template /caminho/para/meu-plugin
   cd /caminho/para/meu-plugin
   ```

2. **Ajuste o nome do plugin** em dois arquivos — os dois valores MUST ser
   idênticos:
   - `farol-plugin.toml`: campo `plugin_name`.
   - `main.py`: constante `PLUGIN_NAME`.

   O nome MUST ser único entre os plugins ativos (os 4 de referência do Farol
   — `git-local`, `uptime-kuma`, `openfortivpn-vpn`, `docker-containers` — e
   qualquer outro plugin já instalado). Uma colisão faz o Farol ignorar seu
   plugin com um aviso, não travar.

3. **Implemente seu plugin de verdade**: adicione widgets/ações em
   `handle_handshake_hello` e os handlers correspondentes (`widget/get`,
   `action/invoke`). Veja qualquer um dos 4 plugins de referência em
   `plugins/` no repositório do Farol para o padrão completo — em especial
   `plugins/git-local/main.py`, o mais simples deles.

4. **Registre o plugin para o Farol descobrir**: copie (ou mova) o diretório
   do seu plugin para dentro do diretório de instalação de plugins do Farol:

   ```sh
   cp -r /caminho/para/meu-plugin ~/.local/share/farol/plugins/meu-plugin
   ```

   Esse caminho é `$XDG_DATA_HOME/farol/plugins/<plugin_name>/` — se
   `$XDG_DATA_HOME` não estiver definida, o padrão é `~/.local/share/farol/`.
   O nome do subdiretório não precisa coincidir com `plugin_name` do
   manifesto (o Farol usa o valor de dentro do arquivo, não o nome do
   diretório), mas é boa prática deixá-los iguais.

5. **Abra o Farol** (ou reinicie, se já estiver rodando) — a descoberta de
   plugins instalados acontece uma vez, no início. O Farol varre
   `~/.local/share/farol/plugins/`, lê o `farol-plugin.toml` de cada
   subdiretório e faz o spawn do processo (`command`/`args` do manifesto,
   relativos ao próprio diretório do plugin) na próxima abertura.

## Schema do `farol-plugin.toml`

Ver
`specs/007-registry-instalacao-plugins-github/contracts/plugin-manifest-and-install-contract.md`
§ "Schema de `farol-plugin.toml`" no repositório do Farol para o contrato
normativo completo (campos obrigatórios, validação, mensagens de erro).

## Protocolo

O protocolo completo (JSON-RPC 2.0 sobre NDJSON em stdin/stdout) está
documentado, de forma agnóstica de linguagem, em `protocol/SPEC.md` no
repositório do Farol.
