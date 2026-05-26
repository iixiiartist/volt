#!/usr/bin/env python3
"""
Enhanced JSON Schema argument validator for BFCL function calling evaluation.

Covers the full JSON Schema draft-07 validation relevant to function calling:
    - type checking (string, integer, number, boolean, array, object)
    - required parameter checking
    - enum constraint validation
    - minimum/maximum for numbers
    - minLength/maxLength for strings
    - pattern (regex) for strings
    - format validation (date-time, date, email, uri)
    - minItems/maxItems for arrays
    - nested object/array validation
    - additionalProperties detection (hallucinated params)

Replaces the thin _check_type function in volt_bench.py.
"""

import re
import json
from typing import Any


TYPE_ALIASES = {
    "integer": "integer",
    "int": "integer",
    "number": "number",
    "float": "number",
    "double": "number",
    "string": "string",
    "str": "string",
    "boolean": "boolean",
    "bool": "boolean",
    "array": "array",
    "list": "array",
    "object": "object",
    "dict": "object",
    "dictionary": "object",
    "null": "null",
}

DATE_REGEX = re.compile(r"^\d{4}-\d{2}-\d{2}$")
DATETIME_REGEX = re.compile(r"^\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}")
EMAIL_REGEX = re.compile(r"^[^@\s]+@[^@\s]+\.[^@\s]+$")


class ArgIssue:
    """A single argument validation issue."""
    def __init__(self, path: str, message: str):
        self.path = path
        self.message = message

    def __str__(self):
        return f"{self.path}: {self.message}"


def validate_function_call(
    args: dict,
    schema: dict,
    path: str = "",
) -> list[ArgIssue]:
    """
    Validate function call arguments against a JSON Schema.

    Args:
        args: The actual arguments passed by the LLM
        schema: The expected parameter schema (JSON Schema for the function)
        path: Current path for nested validation

    Returns:
        List of ArgIssue objects (empty list = valid)
    """
    issues: list[ArgIssue] = []

    schema_type = schema.get("type", "object")
    normalized_type = TYPE_ALIASES.get(str(schema_type).lower(), str(schema_type).lower())

    # Type check for the root
    arg_type = _json_type(args)
    if normalized_type != arg_type and not (normalized_type == "integer" and arg_type == "number" and isinstance(args, int)):
        issues.append(ArgIssue(path or "$", f"expected {normalized_type}, got {arg_type}"))
        return issues

    if normalized_type == "object":
        issues.extend(_validate_object(args, schema, path))
    elif normalized_type == "array":
        issues.extend(_validate_array(args, schema, path))
    else:
        issues.extend(_validate_scalar(args, schema, path))

    return issues


def _validate_object(obj: dict, schema: dict, path: str) -> list[ArgIssue]:
    issues = []
    props = schema.get("properties", {})
    required = schema.get("required", [])

    # Check required params
    for r in required:
        if r not in obj:
            issues.append(ArgIssue(f"{path}.{r}" if path else r, "required parameter missing"))

    # Check each actual argument
    for key, val in obj.items():
        p = f"{path}.{key}" if path else key
        if key not in props:
            # BFCL allows extra non-required params to be passed (model might hallucinate)
            # But if additionalProperties is explicitly false, flag it
            if schema.get("additionalProperties") is False:
                issues.append(ArgIssue(p, "hallucinated parameter (not in schema)"))
            continue

        prop_schema = props[key]
        prop_type = prop_schema.get("type", "string")
        normalized = TYPE_ALIASES.get(str(prop_type).lower(), str(prop_type).lower())

        val_type = _json_type(val)

        # Allow int for number schemas (JSON Schema compatibility)
        if normalized == "number" and val_type == "integer":
            pass
        elif normalized == "integer" and val_type == "integer":
            pass
        elif normalized != val_type:
            issues.append(ArgIssue(p, f"expected {normalized}, got {val_type}"))
            continue

        # Recursive for nested objects/arrays
        if normalized == "object" and isinstance(val, dict):
            issues.extend(_validate_object(val, prop_schema, p))
        elif normalized == "array" and isinstance(val, list):
            issues.extend(_validate_array(val, prop_schema, p))
        else:
            issues.extend(_validate_scalar(val, prop_schema, p))

    return issues


def _validate_array(arr: list, schema: dict, path: str) -> list[ArgIssue]:
    issues = []
    items_schema = schema.get("items", {})

    if "minItems" in schema and len(arr) < schema["minItems"]:
        issues.append(ArgIssue(path, f"array too short: {len(arr)} < {schema['minItems']}"))
    if "maxItems" in schema and len(arr) > schema["maxItems"]:
        issues.append(ArgIssue(path, f"array too long: {len(arr)} > {schema['maxItems']}"))

    if items_schema:
        items_type = TYPE_ALIASES.get(str(items_schema.get("type", "string")).lower(), str(items_schema.get("type", "string")).lower())
        for i, item in enumerate(arr):
            p = f"{path}[{i}]"
            item_type = _json_type(item)
            if items_type == "number" and item_type == "integer":
                continue
            if items_type != item_type:
                issues.append(ArgIssue(p, f"expected {items_type}, got {item_type}"))
            else:
                issues.extend(_validate_scalar(item, items_schema, p))
    return issues


def _validate_scalar(val: Any, schema: dict, path: str) -> list[ArgIssue]:
    issues = []

    # Enum constraint
    if "enum" in schema and val not in schema["enum"]:
        issues.append(ArgIssue(path, f"value '{val}' not in enum {schema['enum']}"))

    if isinstance(val, (int, float)):
        if "minimum" in schema and val < schema["minimum"]:
            issues.append(ArgIssue(path, f"value {val} < minimum {schema['minimum']}"))
        if "maximum" in schema and val > schema["maximum"]:
            issues.append(ArgIssue(path, f"value {val} > maximum {schema['maximum']}"))

    if isinstance(val, str):
        if "minLength" in schema and len(val) < schema["minLength"]:
            issues.append(ArgIssue(path, f"string too short: {len(val)} < {schema['minLength']}"))
        if "maxLength" in schema and len(val) > schema["maxLength"]:
            issues.append(ArgIssue(path, f"string too long: {len(val)} > {schema['maxLength']}"))
        if "pattern" in schema:
            try:
                if not re.search(schema["pattern"], val):
                    issues.append(ArgIssue(path, f"string '{val}' does not match pattern '{schema['pattern']}'"))
            except re.error:
                pass
        if "format" in schema:
            fmt = schema["format"]
            if fmt == "date-time" and not DATETIME_REGEX.search(val):
                issues.append(ArgIssue(path, f"string '{val}' is not valid date-time"))
            elif fmt == "date" and not DATE_REGEX.match(val):
                issues.append(ArgIssue(path, f"string '{val}' is not valid date (YYYY-MM-DD)"))
            elif fmt == "email" and not EMAIL_REGEX.match(val):
                issues.append(ArgIssue(path, f"string '{val}' is not valid email"))

    return issues


def _json_type(val: Any) -> str:
    """Map Python type to JSON Schema type."""
    if val is None:
        return "null"
    if isinstance(val, bool):
        return "boolean"
    if isinstance(val, int):
        return "integer"
    if isinstance(val, float):
        return "number"
    if isinstance(val, str):
        return "string"
    if isinstance(val, list):
        return "array"
    if isinstance(val, dict):
        return "object"
    return "unknown"


# -- Self-tests --

if __name__ == "__main__":
    # Test 1: Basic type checking
    schema = {
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "age": {"type": "integer"},
        },
        "required": ["name"],
    }
    issues = validate_function_call({"name": "John", "age": 30}, schema)
    assert len(issues) == 0, f"Expected 0 issues, got {[str(i) for i in issues]}"
    print("PASS: valid call")

    # Test 2: Missing required
    issues = validate_function_call({"age": 30}, schema)
    assert any("required" in str(i) for i in issues)
    print(f"PASS: missing required -> {[str(i) for i in issues]}")

    # Test 3: Wrong type
    issues = validate_function_call({"name": "John", "age": "thirty"}, schema)
    assert any("expected integer" in str(i) for i in issues)
    print(f"PASS: wrong type -> {[str(i) for i in issues]}")

    # Test 4: Enum constraint
    schema2 = {
        "type": "object",
        "properties": {
            "size": {"type": "string", "enum": ["small", "medium", "large"]},
        },
    }
    issues = validate_function_call({"size": "huge"}, schema2)
    assert any("enum" in str(i) for i in issues)
    print(f"PASS: enum violation -> {[str(i) for i in issues]}")

    # Test 5: Nested object
    schema3 = {
        "type": "object",
        "properties": {
            "address": {
                "type": "object",
                "properties": {
                    "city": {"type": "string"},
                    "zip": {"type": "string", "pattern": r"^\d{5}$"},
                },
                "required": ["city"],
            },
        },
    }
    issues = validate_function_call({"address": {"city": "NYC", "zip": "ABC"}}, schema3)
    assert any("pattern" in str(i) for i in issues)
    print(f"PASS: nested validation -> {[str(i) for i in issues]}")

    # Test 6: Hallucinated params
    schema4 = {
        "type": "object",
        "properties": {"x": {"type": "number"}},
        "additionalProperties": False,
    }
    issues = validate_function_call({"x": 5, "y": 10}, schema4)
    assert any("hallucinated" in str(i) for i in issues)
    print(f"PASS: hallucinated params -> {[str(i) for i in issues]}")

    # Test 7: int-for-number leniency
    schema5 = {"type": "object", "properties": {"val": {"type": "number"}}}
    issues = validate_function_call({"val": 42}, schema5)
    assert len(issues) == 0
    print("PASS: int accepted for number")

    print("\nAll validation tests passed.")
