---
name: "system-diagnostics"
version: "1.0.0"
description: "Local system health checks: CPU, memory, disk, network diagnostics and reporting"
mcp_servers: []
---
# System Diagnostics

Run comprehensive system health diagnostics including CPU usage, memory consumption, disk space, network connectivity, and running processes. Generates a structured health report.

## Allowed Tools
- `bash` - Execute diagnostic commands
- `read` - Read system files (e.g., /proc, /sys)
- `write` - Write diagnostic reports
- `grep` - Search log files

## Diagnostic Commands
- CPU: `wmic cpu get loadpercentage` (Windows) or `top -bn1 | grep Cpu` (Linux)
- Memory: `wmic OS get FreePhysicalMemory,TotalVisibleMemorySize /Value` or `free -m`
- Disk: `wmic logicaldisk get size,freespace,caption` or `df -h`
- Network: `ipconfig` or `ifconfig`
- Processes: `tasklist` or `ps aux`
