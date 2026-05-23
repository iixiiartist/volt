---
name: "data-pipeline"
version: "1.0.0"
description: "ETL data pipeline with extraction, transformation, loading, error handling, and validation"
mcp_servers: []
---
# Data Pipeline

Extract data from various sources (CSV, JSON, databases), transform it with filtering and aggregation, and load it into a target format. Includes error handling, validation rules, and logging at each stage.

## Allowed Tools
- `read` - Read source data files
- `write` - Write output data
- `bash` - Run data processing scripts (Python, PowerShell)
- `grep` - Search/filter data
- `glob` - Find data files by pattern

## Pipeline Stages
1. Extract: Read source files or connect to data sources
2. Validate: Check schema, types, required fields
3. Transform: Filter, map, aggregate, join
4. Load: Write to target format (CSV, JSON, Parquet)
5. Verify: Row count, checksum, sample comparison
