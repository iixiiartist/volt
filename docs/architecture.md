# Architecture

Volt is implemented as a compiled Rust control plane that runs against local PostgreSQL nodes. The design separates three concerns:

1. Cognitive synthesis and orchestration are external to the runtime.
2. Tool provisioning, validation, persistence, and execution are deterministic runtime responsibilities.
3. Local PostgreSQL with pgvector stores tool source, package metadata, dependency relationships, embeddings, and execution audit records.

## Core modules

### `db`

Initializes and writes the unified persistence schema. The schema maps agent tools, asset relationships, registry events, and execution events.

### `registry`

Fetches package manifests from the Volt registry and provisions them into the local node. The remote endpoint is modeled as:

```text
GET /packages/{pkg_id}
```

The returned manifest is validated before storage.

### `embedding`

Uses a Kimi-compatible embedding endpoint when `KIMI_API_KEY` is available. Otherwise it emits deterministic placeholder vectors so local development can run without external API calls.

### `validation`

Performs static source-code scanning before package storage. The denylist checks for environment access, broad filesystem access, direct shell escapes, network clients, device namespaces, and unsafe Rust blocks.

### `sandbox`

Runs bounded local subprocesses with timeout and output caps. Production execution should move this into stronger kernel isolation using a hardened OCI runtime, gVisor, Firecracker, or equivalent.

## Data model

The primary table is `agent_tools`, which stores tool name, description, language, source code, JSON parameter schema, 1536-dimensional vector embedding, marketplace verification status, cryptographic signature, source hash, and full manifest JSON.

`asset_relationships` represents package dependency and extension relationships. `tool_executions` records local execution audits with UUID tracking. `registry_events` records provisioning events.