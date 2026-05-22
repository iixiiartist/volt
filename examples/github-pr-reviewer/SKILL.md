---
name: "github_pr_reviewer"
version: "1.0.0"
description: "Automated GitHub Pull Request reviewer that analyzes code changes, checks for security issues, and provides actionable feedback."
mcp_servers: ["github-api", "code-search"]
---
# GitHub PR Reviewer

An intelligent agent that performs comprehensive code reviews on GitHub Pull Requests. It analyzes diff content, checks for security vulnerabilities, validates coding standards, and provides detailed feedback before merge.

## Capabilities

- **Code Analysis**: Reads and parses changed files to understand context
- **Security Scanning**: Identifies potential security issues (SQL injection, XSS, hardcoded secrets)
- **Style Checking**: Validates adherence to project coding standards
- **Context Awareness**: Searches project history for similar patterns
- **Actionable Feedback**: Generates structured review comments with line references

## Allowed Tools

- `read` - Read changed files and project context
- `grep` - Search for security patterns and coding violations
- `web_fetch` - Fetch GitHub PR metadata and issue references
- `write` - Create review comments or update documentation
- `glob` - Locate related test files and configuration

## Usage

When a user requests a PR review, this skill:
1. Fetches PR metadata via `web_fetch`
2. Reads changed files using `read`
3. Searches for security patterns with `grep`
4. Compiles findings into a structured review
5. Optionally writes review comments back

## Security Constraints

- Never execute arbitrary code from PRs
- Sanitize all external inputs
- Flag hardcoded credentials immediately
- Require human approval for destructive operations