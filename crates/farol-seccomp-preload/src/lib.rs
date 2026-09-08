//! Biblioteca `cdylib` carregada via `LD_PRELOAD` para aplicar, DENTRO do
//! próprio processo de um plugin já `exec`ado com sucesso pelo `bwrap`, o
//! filtro `seccomp-bpf` que bloqueia `execve`/`execveat` — mediação real de
//! `exec` da feature `009-sandbox-hardening`, User Story 1, issue #12.
//!
//! # Por que este crate existe (revisão de D1 — achado empírico desta sessão)
//!
//! A primeira versão de T004/T005 gerava o filtro no processo do Farol e
//! tentava repassá-lo ao `bwrap` via `--seccomp FD` (`bwrap --seccomp FD ...
//! -- <comando do plugin>`). Essa abordagem se mostrou **inviável**:
//! `bwrap --seccomp FD` instala o filtro seccomp NO PRÓPRIO PROCESSO
//! `bwrap`, antes do `execvp()` final que ele mesmo faz para lançar o
//! comando do plugin — um filtro que bloqueia `execve` bloqueia justamente
//! esse `execvp` interno, e o plugin nunca chega a iniciar (confirmado
//! empiricamente: `bwrap: execvp /usr/bin/python3: Permission denied`).
//!
//! A correção (D1 revisado, `specs/009-sandbox-hardening/plan.md`): o filtro
//! passa a ser aplicado DENTRO do processo do plugin, depois que ele já foi
//! `exec`ado com sucesso pelo `bwrap` — via `LD_PRELOAD` de UM `.so` (este
//! crate) com um construtor ELF que roda depois que o *dynamic linker*
//! termina de carregar as bibliotecas do novo processo, mas antes do
//! `main()` dele. Nesse ponto o processo já está de pé e não precisa de
//! nenhum `execve`/`execveat` adicional para continuar funcionando — o
//! filtro instalado aqui só nega tentativas SUBSEQUENTES do próprio plugin,
//! fechando o vetor residual da issue #12 (escrever um binário num
//! diretório gravável do sandbox, ex. `/tmp`, e executá-lo por caminho
//! absoluto).
//!
//! `farol_core::sandbox::build_bwrap_args` (quando `allow_exec = false`)
//! localiza o `.so` compilado deste crate
//! (`farol_core::sandbox_seccomp::locate_seccomp_preload_library`), bind-
//! monta-o read-only dentro do sandbox, e passa `--setenv LD_PRELOAD
//! <caminho>` ao `bwrap` — nenhum `memfd`/FD é mais repassado entre
//! processos: `seccompiler::apply_filter` (chamado por
//! [`try_install_filter`] abaixo) aplica o filtro diretamente no processo
//! ATUAL (`prctl(PR_SET_NO_NEW_PRIVS, ...)` + `syscall(SYS_seccomp,
//! SECCOMP_SET_MODE_FILTER, ...)`), sem precisar de nenhum FD vindo de fora.
//!
//! # Por que duplicar a lógica de geração do filtro, não importar `farol-core`
//!
//! `farol-core` não tem um alvo `[lib]` (só `[[bin]] name = "farol"`,
//! `crates/farol-core/Cargo.toml`) — importar suas funções internas exigiria
//! dar a ele um `[lib]` novo, uma mudança estrutural maior que não foi
//! pedida por esta task e que afetaria a compilação do binário principal do
//! Farol. A lógica em si (bloquear `execve`/`execveat`, ação `Errno(EACCES)`,
//! ~15 linhas) é pequena o bastante para duplicar sem risco real de
//! divergência silenciosa — mantida com o texto e a intenção idênticos ao
//! que existia em `farol_core::sandbox_seccomp` antes desta revisão (ver
//! histórico do git). Caso a lógica cresça ou precise ser reaproveitada em
//! um terceiro lugar, promover `farol-core` a ter um `[lib]` (ou extrair um
//! crate `farol-seccomp-filter` comum a ambos) é o caminho natural — fora do
//! escopo desta task.
//!
//! # Syscalls bloqueadas
//!
//! `execve`/`execveat` — e, por consequência, `fexecve`: a syscall
//! `fexecve` não existe como número de syscall próprio no kernel Linux; a
//! implementação de `fexecve(3)` na glibc é só uma função de biblioteca que
//! delega para `execveat`/`execve` — ambos os caminhos já ficam bloqueados
//! bloqueando essas duas. FR-001 cita `fexecve` explicitamente pelo nome de
//! qualquer forma, por isso a menção aqui — não há uma terceira entrada de
//! filtro correspondente porque não há uma terceira syscall real para
//! bloquear.

use std::collections::BTreeMap;

use seccompiler::{BpfProgram, SeccompAction, SeccompFilter};

// `ctor` compilado com `default-features = false, features = ["proc_macro"]`
// (`Cargo.toml` desta lib) — só a macro de atributo em si, sem puxar
// `dtor`/`link-section` (features `dtor`/`priority` do crate `ctor`, não
// usadas aqui).

/// Código de saída do processo quando o construtor não consegue aplicar o
/// filtro seccomp — fail-closed (FR-004 de `specs/009-sandbox-hardening/
/// spec.md`): o processo do plugin nunca deve continuar rodando sem a
/// mediação real de exec que `allow_exec=false` promete. `97` escolhido só
/// por não colidir com os códigos de saída convencionais (`1`, `2`, `126`,
/// `127`) que um plugin/interpretador poderia usar para os próprios erros —
/// facilita distinguir "o preload de seccomp falhou" de "o plugin falhou
/// depois de iniciar normalmente" ao investigar um `WorkerEvent` de saída
/// inesperada do lado de `plugin_worker`.
const SECCOMP_CTOR_FAILURE_EXIT_CODE: i32 = 97;

/// Construtor ELF: roda automaticamente assim que o dynamic linker termina
/// de carregar este `.so` (via `LD_PRELOAD`) no processo alvo — antes do
/// `main()` desse processo. Ver doc do módulo para o porquê deste desenho
/// (D1 revisado).
///
/// Fail-closed (FR-004): se o filtro não puder ser aplicado por qualquer
/// motivo, o processo é encerrado imediatamente (`libc::_exit`, sem rodar
/// destrutores/handlers que poderiam mascarar o problema) em vez de deixar o
/// plugin continuar de pé sem a mediação real de exec.
///
/// # Safety
///
/// `unsafe fn` exigida pelo `ctor` 0.10+ para qualquer construtor (código
/// rodando antes do `main()`, ponto em que a maior parte do runtime do Rust
/// — alocador, `std::io` com locking, `panic` com unwind — ainda não está
/// necessariamente pronta para uso normal). Esta função só usa `libc::write`/
/// `libc::_exit` crus (sem stdio bufferizado) e chama `try_install_filter`
/// (código Rust seguro), respeitando essa restrição.
#[ctor::ctor]
unsafe fn install_exec_deny_seccomp_filter() {
    if let Err(message) = try_install_filter() {
        let full_message = format!(
            "farol-seccomp-preload: falha ao aplicar filtro seccomp de bloqueio de exec \
             ({message}) — encerrando o processo (fail-closed, FR-004 de \
             specs/009-sandbox-hardening/spec.md)\n"
        );
        // SAFETY: `libc::write` com um buffer/tamanho válidos (o `String`
        // acima é dono da memória e continua vivo até o fim desta chamada);
        // stderr (fd 2) sempre existe. Usa a syscall crua em vez de
        // `eprintln!`/stdio para não depender de nenhuma inicialização de
        // runtime do Rust que ainda não tenha rodado neste ponto tão cedo
        // do carregamento do processo.
        unsafe {
            libc::write(
                libc::STDERR_FILENO,
                full_message.as_ptr() as *const libc::c_void,
                full_message.len(),
            );
        }
        // SAFETY: `_exit` é sempre segura de chamar — termina o processo
        // imediatamente, sem rodar destrutores C++/Rust nem handlers de
        // `atexit`, o que é exatamente o desejado aqui (evita qualquer
        // janela em que o processo continuasse rodando sem a proteção).
        unsafe { libc::_exit(SECCOMP_CTOR_FAILURE_EXIT_CODE) };
    }
}

fn try_install_filter() -> Result<(), String> {
    let program = build_exec_deny_filter()?;
    seccompiler::apply_filter(&program).map_err(|e| format!("seccompiler::apply_filter falhou: {e}"))
}

/// Compila o filtro BPF em memória — bloqueia `execve`/`execveat` com
/// `EACCES`, permite (`SeccompAction::Allow`) qualquer outra syscall. Este
/// filtro NÃO é uma sandbox geral de syscalls: só fecha o vetor residual da
/// issue #12.
fn build_exec_deny_filter() -> Result<BpfProgram, String> {
    let arch = std::env::consts::ARCH.try_into().map_err(|_| {
        format!(
            "arquitetura {:?} não suportada pelo seccompiler (só x86_64/aarch64 little-endian)",
            std::env::consts::ARCH
        )
    })?;

    // Regra com `Vec::new()` (nenhuma condição de argumento) == "casa sempre
    // que esta syscall for chamada, não importam os argumentos" — é
    // exatamente isso que queremos: bloquear `execve`/`execveat`
    // incondicionalmente.
    let blocked_syscalls: BTreeMap<i64, Vec<seccompiler::SeccompRule>> =
        [libc::SYS_execve, libc::SYS_execveat]
            .into_iter()
            .map(|syscall_nr| (syscall_nr, Vec::new()))
            .collect();

    let filter = SeccompFilter::new(
        blocked_syscalls,
        SeccompAction::Allow,
        SeccompAction::Errno(libc::EACCES as u32),
        arch,
    )
    .map_err(|e| format!("construção do filtro seccomp falhou: {e}"))?;

    filter
        .try_into()
        .map_err(|e: seccompiler::BackendError| format!("compilação do filtro seccomp para BPF falhou: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Confirma só que o filtro compila sem erro nesta arquitetura/host —
    /// o comportamento observável do filtro (exec de fato bloqueado dentro
    /// de um processo real) é validado com `bwrap` real por
    /// `farol_core::sandbox::sandbox_integration_tests` (T007), fora deste
    /// crate.
    #[test]
    fn build_exec_deny_filter_succeeds_on_a_supported_architecture() {
        build_exec_deny_filter().expect("build_exec_deny_filter deve funcionar neste host");
    }

    /// Não testa `install_exec_deny_seccomp_filter`/`try_install_filter`
    /// ponta a ponta neste `#[cfg(test)]` do PRÓPRIO crate: o binário de
    /// teste gerado por `cargo test -p farol-seccomp-preload` já carrega
    /// este `.so`/`rlib` diretamente no processo de teste (não via
    /// `LD_PRELOAD`), e o `#[ctor]` já roda automaticamente ANTES de
    /// qualquer teste começar — instalar o filtro de verdade no processo do
    /// harness de testes bloquearia `execve`/`execveat` para o resto da
    /// execução de `cargo test`, incluindo qualquer coisa que o harness
    /// precise exec'ar internamente. A prova ponta a ponta (biblioteca
    /// carregada via `LD_PRELOAD` num processo `python3` real, dentro de um
    /// `bwrap` real, exec de fato bloqueado) fica em
    /// `farol_core::sandbox::sandbox_integration_tests` (T007).
    #[test]
    fn ctor_already_ran_before_this_test_without_this_process_crashing() {
        // Só confirma que chegamos até aqui — se `install_exec_deny_seccomp_filter`
        // tivesse falhado, o processo de teste inteiro já teria sido
        // encerrado (`libc::_exit`) antes de qualquer teste rodar.
    }
}
