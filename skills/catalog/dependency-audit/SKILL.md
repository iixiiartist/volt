---
name: "dependency-audit"
version: "1.0.0"
description: "Audit project dependencies for security vulnerabilities, outdated packages, and license compliance"
mcp_servers: []
---
# Dependency Audit

Audit a project's dependencies across multiple languages (Rust/Cargo, Node/npm, Python/pip). Checks for known CVEs, outdated packages, license compatibility, and deprecated dependencies.

## Allowed Tools
- `read` - Read lock files and manifests
- `bash` - Run audit tools (cargo audit, npm audit, pip-audit)
- `grep` - Search for specific dependency patterns
- `glob` - Find manifest files

## Supported Ecosystems
- Rust: Cargo.toml / Cargo.lock (cargo audit)
- Node: package.json / package-lock.json (npm audit)
- Python: requirements.txt / Pipfile (pip-audit)
- Docker: Dockerfile base image tags

## Report Sections
1. Manifest versions (pinned vs ranged)
2. Known vulnerabilities (critical/high/medium/low)
3. Outdated dependencies (major/minor/patch behind)
4. License summary (permissive vs copyleft)
5. Recommendations
