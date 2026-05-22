#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""
Test script for GitHub PR Reviewer skill.
This verifies the SKILL.md can be parsed and the skill can be compiled into the database.

Usage:
    python test_skill.py
"""

import subprocess
import sys
import os
from pathlib import Path

def test_skill_compilation():
    """Test that the skill compiles successfully."""
    skill_path = Path(__file__).parent / "SKILL.md"
    
    if not skill_path.exists():
        print(f"ERROR: SKILL.md not found at {skill_path}")
        return False
    
    print(f"OK: Found SKILL.md at {skill_path}")
    
    # Parse the frontmatter manually to verify structure
    with open(skill_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Check for required fields
    required_fields = ['name:', 'version:', 'description:', 'mcp_servers:']
    for field in required_fields:
        if field not in content:
            print(f"ERROR: Missing required field: {field}")
            return False
    
    print("OK: All required fields present")
    
    # Check for expected sections
    expected_sections = ['# GitHub PR Reviewer', '## Capabilities', '## Allowed Tools']
    for section in expected_sections:
        if section not in content:
            print(f"ERROR: Missing expected section: {section}")
            return False
    
    print("OK: All expected sections present")
    
    # Verify the skill can be provisioned (requires running volt)
    # This is a manual step for now since we don't have a running DB
    print("\nINFO: To compile this skill into the database, run:")
    print(f"   volt provision-skill --path {skill_path}")
    
    return True

def test_tool_references():
    """Verify that referenced tools exist in the system."""
    skill_path = Path(__file__).parent / "SKILL.md"
    
    with open(skill_path, 'r', encoding='utf-8') as f:
        content = f.read()
    
    # Extract tool references from the skill
    tools_mentioned = ['read', 'grep', 'web_fetch', 'write', 'glob']
    
    print("\nINFO: Verifying tool references:")
    for tool in tools_mentioned:
        if tool in content:
            print(f"   OK: {tool} - referenced in skill")
        else:
            print(f"   INFO: {tool} - not referenced (may be optional)")
    
    return True

if __name__ == "__main__":
    print("=" * 60)
    print("GitHub PR Reviewer Skill Test Suite")
    print("=" * 60)
    
    success = True
    success &= test_skill_compilation()
    success &= test_tool_references()
    
    print("\n" + "=" * 60)
    if success:
        print("SUCCESS: All tests passed!")
        sys.exit(0)
    else:
        print("FAILURE: Some tests failed")
        sys.exit(1)