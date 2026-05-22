# Volt Examples

This directory contains production-ready skill packages that demonstrate the **Compiled Manifest Pattern** and **Unified RAG Loop** in action.

## Quick Start

### 1. Compile a Skill into the Database

```bash
# Compile the GitHub PR Reviewer skill
volt provision-skill --path ./examples/github-pr-reviewer/SKILL.md

# Verify it was stored
volt list-skills
```

### 2. Run the Agent with RAG-Enabled Tool Selection

```bash
# The agent now dynamically selects tools based on context
volt agent-run --input "Review this PR for security issues"

# Or start an interactive chat session
volt agent-chat
```

### 3. Verify the RAG Loop

The agent will:
1. Embed your input query
2. Search for relevant tools (top-8 by similarity)
3. Search for relevant skills (top-3 by similarity)
4. Search for relevant memories (top-5 by similarity)
5. Inject only the relevant context into the LLM call
6. Execute the selected tools

## Example Skills

### GitHub PR Reviewer (`github-pr-reviewer/`)

A comprehensive code review agent that:
- Analyzes PR diffs for security vulnerabilities
- Checks coding standards and style
- Searches project history for similar patterns
- Generates structured review comments

**Test it:**
```bash
cd examples/github-pr-reviewer
python test_skill.py
volt provision-skill --path SKILL.md
```

## Creating Your Own Skill

1. Create a new directory: `mkdir examples/my-skill`
2. Add a `SKILL.md` file with the required frontmatter:
   ```markdown
   ---
   name: "my_skill"
   version: "1.0.0"
   description: "Brief description of what this skill does"
   mcp_servers: ["optional-server"]
   ---
   # My Skill

   Detailed description...

   ## Allowed Tools
   - `tool_name` - what it does
   ```
3. Test it: `python test_skill.py`
4. Compile it: `volt provision-skill --path SKILL.md`

## Architecture

Each skill goes through the **Compiled Manifest Pattern**:

```
SKILL.md (Human-Readable) 
    → volt provision-skill (Compiler)
    → PostgreSQL + pgvector (Runtime)
    → Agent RAG Loop (Execution)
```

This ensures:
- **Fast lookups**: HNSW vector index for sub-millisecond search
- **Security**: Immutable records with foreign key constraints
- **Flexibility**: Easy to author in Markdown, hard to break at runtime

## Testing

All examples include a `test_skill.py` script:

```bash
cd examples/github-pr-reviewer
python test_skill.py
```

This verifies:
- Required frontmatter fields are present
- Expected sections exist
- Tool references are valid

## Contributing

To add a new skill:
1. Create a new directory under `examples/`
2. Add `SKILL.md` and `test_skill.py`
3. Update this README with your skill's description
4. Submit a pull request

## Production Deployment

For production use:
1. Compile all skills into the database
2. Set up the HNSW index on the `skills` table
3. Configure your embedding provider (Ollama, NVIDIA, etc.)
4. Run `volt agent-run` or `volt agent-chat`

The agent will automatically use the compiled skills for context-aware tool selection.