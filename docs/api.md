# CLI and Runtime API

## Commands

### Initialize database

```bash
volt init-db
```

Applies `migrations/0001_core.sql` to the configured PostgreSQL database.

### Validate package manifest

```bash
volt validate --manifest examples/manifests/cloud-devops-k8s-audit.json
```

Runs static checks against package source code and emits a JSON validation report.

### Provision local manifest

```bash
volt provision-file --manifest examples/manifests/cloud-devops-k8s-audit.json --marketplace-verified
```

Validates, embeds, and upserts a package into the local node.

### Provision remote package

```bash
volt provision --pkg-id cloud-devops-k8s-audit
```

Fetches a registry manifest and stores it locally after validation.

### List installed tools

```bash
volt list-tools
```

Outputs installed tools as JSON.

### Execute a tool

```bash
volt execute --tool cloud-devops-k8s-audit --params '{"items": []}'
```

Runs a provisioned tool's source code in the sandbox with the given parameters and records the execution.

### View execution history

```bash
volt history --limit 20
```

### Run sandbox command

```bash
volt sandbox --command "echo '{\"ok\": true}'" --timeout-ms 5000
```

Runs a command under the sandbox runner.

## Environment variables

See `.env.example`.