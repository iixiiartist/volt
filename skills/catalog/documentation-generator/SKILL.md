---
name: "documentation-generator"
version: "1.0.0"
description: "Generate project documentation from source code: API docs, README, architecture overview"
mcp_servers: []
---
# Documentation Generator

Analyze source code to generate comprehensive documentation. Extracts module structure, public API surfaces, function signatures, and architecture patterns to produce README, API reference, and architecture documentation.

## Allowed Tools
- `read` - Read source files
- `grep` - Extract doc comments and signatures
- `glob` - Find all source files
- `write` - Write documentation files
- `bash` - Run documentation generators

## Documentation Outputs
1. README.md: Project overview, setup, usage, API
2. API Reference: Function/module signatures with descriptions
3. Architecture: Module hierarchy and data flow
4. Changelog: Recent changes from git log
