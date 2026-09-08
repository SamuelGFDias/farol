//! Allowlist de rede por host via `nftables`/`iptables` dentro do namespace
//! de rede do sandbox (feature `009-sandbox-hardening`, User Story 2, issue
//! #13).
//!
//! Vazio nesta fase (T002, Phase 1 Setup) — a implementação (resolução de
//! host→IP no processo pai e geração do script wrapper que aplica as regras
//! antes de `exec`ar o comando do plugin) é T009/T010, fora do escopo desta
//! rodada.
