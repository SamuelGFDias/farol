//! Localização do artefato de build usado para a mediação real de `exec`
//! dentro do sandbox (feature `009-sandbox-hardening`, User Story 1, issue
//! #12).
//!
//! # Revisão de D1 (achado empírico desta sessão)
//!
//! A primeira versão deste módulo (T004 original) gerava o filtro
//! `seccomp-bpf` no processo do Farol via [`seccompiler`](https://docs.rs/seccompiler)
//! e o serializava num `memfd`, pronto para ser repassado ao `bwrap` via
//! `--seccomp FD` (`bwrap --seccomp FD ... -- <comando do plugin>`). Essa
//! abordagem se mostrou **inviável**: `bwrap --seccomp FD` instala o filtro
//! seccomp NO PRÓPRIO PROCESSO `bwrap`, antes do `execvp()` final que ele
//! mesmo faz para lançar o comando do plugin — um filtro que bloqueia
//! `execve` bloqueia justamente esse `execvp` interno, e o plugin nunca
//! chega a iniciar (confirmado empiricamente: `bwrap: execvp
//! /usr/bin/python3: Permission denied`).
//!
//! A correção (D1 revisado, `specs/009-sandbox-hardening/plan.md`): o filtro
//! passa a ser aplicado DENTRO do processo do plugin, depois que ele já foi
//! `exec`ado com sucesso pelo `bwrap` — via `LD_PRELOAD` de uma biblioteca
//! compartilhada com um construtor ELF (`crates/farol-seccomp-preload`,
//! crate `cdylib` novo desta revisão). A geração/aplicação do filtro BPF em
//! si (bloquear `execve`/`execveat`, ação `Errno(EACCES)`) vive inteiramente
//! naquele crate agora — não há mais nenhum código de geração de filtro
//! seccomp do lado do processo do Farol; a lógica de bloqueio é idêntica à
//! versão anterior, só realocada (ver doc de
//! `farol_seccomp_preload::build_exec_deny_filter` para o porquê de não ter
//! sido reaproveitada via import, já que `farol-core` não tem um alvo
//! `[lib]`). A versão anterior deste módulo (geração do filtro + `memfd`)
//! está preservada no histórico do git para referência, não neste arquivo.
//!
//! Este módulo, depois da revisão, só localiza o `.so` já compilado de
//! `farol-seccomp-preload` no `target/` do workspace, para que
//! `sandbox::build_bwrap_args` possa bind-montá-lo read-only dentro do
//! sandbox e passar `--setenv LD_PRELOAD <caminho>` ao `bwrap` quando
//! `allow_exec = false`.

use std::path::{Path, PathBuf};

use crate::sandbox::SandboxMountError;

/// Nome do arquivo `.so` produzido por `cargo build -p farol-seccomp-preload`
/// — convenção padrão do Cargo para um `cdylib` de um pacote/lib chamado
/// `farol_seccomp_preload` (hífen vira `_`, `[lib] name` de
/// `crates/farol-seccomp-preload/Cargo.toml`): `lib<nome>.so` no Linux.
const SECCOMP_PRELOAD_LIB_FILENAME: &str = "libfarol_seccomp_preload.so";

/// Escape-hatch só de teste (mesmo padrão de `FAROL_SANDBOX_TEST_EXTRA_BIND`,
/// D14 de `specs/006-sandbox-permissoes-bubblewrap/research.md`): quando
/// definida com um valor não vazio, força [`locate_seccomp_preload_library`]
/// a falhar como se o `.so` não pudesse ser localizado neste sistema — usada
/// só por `sandbox::sandbox_unit_tests` para exercitar o caminho fail-closed
/// de `sandbox::build_bwrap_args` (FR-004) sem precisar apagar de verdade um
/// artefato de build já compilado. Nenhum código de produção (nenhum
/// plugin, nenhuma entrada de `plugin_worker::known_plugins()`) depende
/// dela.
pub(crate) const FORCE_LIBRARY_MISSING_ENV_VAR: &str =
    "FAROL_SANDBOX_TEST_FORCE_SECCOMP_PRELOAD_MISSING";

/// Lock compartilhado para serializar QUALQUER teste (deste módulo ou de
/// `sandbox::sandbox_unit_tests`) que mute [`FORCE_LIBRARY_MISSING_ENV_VAR`]
/// — precisa ser um único `Mutex` visível dos dois lugares, não um por
/// módulo: dois `Mutex`s distintos protegendo a mesma variável de ambiente
/// global do processo não impediriam interleaving entre um teste de um
/// módulo e um teste do outro rodando em threads paralelas (mesma classe de
/// bug intermitente de interleaving em variável de ambiente global já
/// reproduzida e corrigida nesta feature para a variável equivalente da
/// versão anterior deste módulo — ver histórico do git).
#[cfg(test)]
pub(crate) static FORCE_LIBRARY_MISSING_ENV_VAR_TEST_LOCK: std::sync::Mutex<()> =
    std::sync::Mutex::new(());

/// Localiza o `.so` compilado de `farol-seccomp-preload` em
/// `<raiz-do-repo>/target/{debug,release}/libfarol_seccomp_preload.so` —
/// mesmo `target/` compartilhado por todo o workspace Cargo, independente
/// de qual crate-membro dispara o build (`cargo build --workspace`,
/// `cargo build -p farol-seccomp-preload`, ou a própria compilação de
/// `farol-core`, já que `farol-seccomp-preload` é membro do workspace desde
/// esta feature). O subdiretório de profile (`debug`/`release`) é escolhido
/// via `cfg!(debug_assertions)` do PRÓPRIO binário `farol-core` — mesma
/// convenção implícita já usada por `cargo test`/`cargo build` sem
/// `--release` (profile `dev`, `debug_assertions = true`).
///
/// Fail-closed (FR-004): devolve `Err(SandboxMountError::SeccompUnavailable)`
/// (nunca um caminho inexistente que o chamador pudesse usar sem checar) se
/// o arquivo não existir — ex.: `farol-seccomp-preload` nunca foi compilado
/// nesta máquina. `sandbox::build_bwrap_args` (chamado por
/// `plugin_worker::worker()` como função infalível, `Vec<String>`, fora do
/// escopo desta feature mudar essa assinatura) trata esse `Err` propagando
/// um `panic!` com a mensagem de [`SandboxMountError`] — nunca monta o
/// sandbox sem a proteção real de `exec`.
pub(crate) fn locate_seccomp_preload_library() -> Result<PathBuf, SandboxMountError> {
    if let Some(reason) = forced_missing_reason() {
        return Err(SandboxMountError::SeccompUnavailable(reason));
    }

    let repo_root = repo_root_for_target_dir();
    let profile_dir = if cfg!(debug_assertions) { "debug" } else { "release" };
    let candidate = repo_root
        .join("target")
        .join(profile_dir)
        .join(SECCOMP_PRELOAD_LIB_FILENAME);

    if candidate.is_file() {
        Ok(candidate)
    } else {
        Err(SandboxMountError::SeccompUnavailable(format!(
            "{candidate:?} não encontrado — rode `cargo build -p farol-seccomp-preload` (ou \
             `cargo build --workspace`) antes de iniciar o Farol; sem esse artefato, a mediação \
             real de exec (issue #12) não pode ser aplicada"
        )))
    }
}

fn forced_missing_reason() -> Option<String> {
    let value = std::env::var_os(FORCE_LIBRARY_MISSING_ENV_VAR)?;
    if value.is_empty() {
        return None;
    }
    Some(format!(
        "{FORCE_LIBRARY_MISSING_ENV_VAR} definida — indisponibilidade simulada só de teste \
         (FR-004), nenhum problema real neste build"
    ))
}

/// Raiz do repositório Farol, para resolver `target/` — mesma técnica de
/// `plugin_worker::farol_repo_root()` (`env!("CARGO_MANIFEST_DIR")` de
/// `farol-core`, dois níveis acima).
fn repo_root_for_target_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CARGO_MANIFEST_DIR deve ser <raiz-do-repo>/crates/farol-core")
        .to_path_buf()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn locate_seccomp_preload_library_honors_the_test_only_force_missing_escape_hatch() {
        let _guard = FORCE_LIBRARY_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::set_var(FORCE_LIBRARY_MISSING_ENV_VAR, "1");
        let result = locate_seccomp_preload_library();
        std::env::remove_var(FORCE_LIBRARY_MISSING_ENV_VAR);

        assert!(
            matches!(result, Err(SandboxMountError::SeccompUnavailable(_))),
            "esperava Err(SeccompUnavailable(_)) com o escape-hatch de teste ativo; got={result:?}"
        );
    }

    /// Sem a variável de ambiente definida, o resultado depende só de o
    /// `.so` já ter sido compilado neste `target/` — não afirma `Ok`/`Err`
    /// aqui (rodar só `cargo test -p farol-core sandbox_seccomp` sem um
    /// `cargo build -p farol-seccomp-preload` prévio devolveria `Err` de
    /// forma legítima, não um bug); só confirma que o escape-hatch de teste
    /// não vaza para esta chamada por engano.
    #[test]
    fn locate_seccomp_preload_library_without_the_escape_hatch_does_not_report_the_forced_reason() {
        let _guard = FORCE_LIBRARY_MISSING_ENV_VAR_TEST_LOCK
            .lock()
            .unwrap_or_else(|e| e.into_inner());

        std::env::remove_var(FORCE_LIBRARY_MISSING_ENV_VAR);
        if let Err(SandboxMountError::SeccompUnavailable(detail)) = locate_seccomp_preload_library() {
            assert!(
                !detail.contains("indisponibilidade simulada só de teste"),
                "sem o escape-hatch definido, um Err real não deveria carregar a mensagem do \
                 caminho simulado; detail={detail:?}"
            );
        }
    }
}
