import re

with open('CODE_QUALITY_REVIEW.md', 'r') as f:
    content = f.read()

# Fix #32 status
content = content.replace(
    '- [ ] **#32 MED** `src/llm/ollama.rs:94-126` + `src/llm/openai.rs:284-326`',
    '- [x] **#32 MED** `src/llm/ollama.rs:94-126` + `src/llm/openai.rs:284-326`'
)

# Fix #37 status
content = content.replace(
    '- [ ] **#37 MED** `src/llm/openai.rs:165-166` + `ollama.rs:59-60` + `anthropic.rs:130,174`',
    '- [x] **#37 MED** `src/llm/openai.rs:165-166` + `ollama.rs:59-60` + `anthropic.rs:130,174`'
)

# Fix #39 status
content = content.replace(
    '- [ ] **#39 LOW** `src/cli_tools/mod.rs:84,175` (and 103,193)',
    '- [x] **#39 LOW** `src/cli_tools/mod.rs:84,175` (and 103,193)'
)

# Fix #59 status
content = content.replace(
    '- [ ] **#59 LOW** `src/tools/delegate.rs:11-12`',
    '- [x] **#59 LOW** `src/tools/delegate.rs:11-12`'
)

# Fix #60 status
content = content.replace(
    '- [ ] **#60 LOW** `src/tools/delegate.rs:106-109`',
    '- [x] **#60 LOW** `src/tools/delegate.rs:106-109`'
)

# Fix #64 status
content = content.replace(
    '- [ ] **#64 MED** `src/agent/router.rs:263,279`',
    '- [-] **#64 MED** `src/agent/router.rs:263,279`'
)

# Update the progress log
content = content.replace(
    '(Started 2026-06-06)',
    '(Started 2026-06-06)\n\n### Session 1 (2026-06-06)\n- Completed 12 critical/high real bugs (#1-#12)\n- Completed dead code cleanup (#13-#15, #18, #19, #24, #26)\n- Extracted shared helpers for duplication (#27-#31, #33-#35, #38, #40)\n- Decomposed large functions (#41, #43)\n- Replaced magic numbers with constants (#54-#57)\n- Im'}  

with open('CODE_QUALITY_REVIEW.md', 'w') as f:
    f.write(content)

print("Done")
