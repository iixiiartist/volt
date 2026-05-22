# Marketplace

Volt's marketplace model enables a centralized registry where verified creators publish agent tool packages. The runtime provisions from this registry and executes packages locally.

## Registry API

```text
GET /packages/{pkg_id}
```

Returns a `RegistryManifest` JSON object.

## Package format

Each package includes:

- `tool_name` — unique identifier
- `description` — human-readable summary (used for embedding search)
- `language` — one of: rust, python, wasm, bash, javascript
- `source_code` — the tool implementation
- `parameter_schema` — JSON Schema for expected parameters
- `signature` — optional cryptographic signature
- `source_sha256` — optional declared hash for integrity verification
- `relationships` — dependency/extension edges to other tools
- `metadata` — arbitrary key-value metadata

## Commercial model

The open-source core remains free. Monetization is designed around a centralized exchange and registry clearing fee for premium or enterprise-validated packages.