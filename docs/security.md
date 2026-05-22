# Security and Sandboxing

The Volt architecture requires multi-tier containment for third-party package execution.

## Tier 1: Static validation

The validator rejects common unsafe patterns:

- Environment enumeration
- Direct `/etc`, `/proc`, or `/dev` reads
- Shell escape patterns
- Network clients
- Unsafe Rust blocks

This is a baseline filter, not a complete security boundary.

## Tier 2: Runtime isolation

The Rust runner uses:

- Environment clearing
- Explicit `PATH`
- Process timeout
- Output truncation
- Optional working directory isolation

For untrusted package execution, use `scripts/run_sandbox.sh` or containerized execution with:

```bash
docker run --rm --network none --read-only --memory 64m --cpus 1 --pids-limit 64 ...
```

## Tier 3: Deterministic resource thresholds

Production defaults:

- Timeout: 5000ms
- Memory ceiling: 64MB
- No outbound network
- Read-only root filesystem
- Temporary writable working directory
- Structured stdout/stderr/error traces

## Production hardening backlog

- Replace command execution with WASI or microVM-backed workers
- Use gVisor, Firecracker, or hardened OCI runtime profiles
- Add syscall filtering through seccomp
- Add per-package capability manifests
- Verify signatures with Ed25519 or Sigstore
- Store immutable package blobs outside mutable database rows
- Add package behavior provenance and audit trails