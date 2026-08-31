#!/usr/bin/env python3
import sys
import os
import json
import argparse
import time
import tempfile

EXIT_SUCCESS = 0
EXIT_INVALID_ARGS = 1
EXIT_CORRUPT_INPUT = 2
EXIT_TRACEABILITY_GAP = 3
EXIT_CHECKPOINT_ERROR = 4
EXIT_VERSION_MISMATCH = 5

MAX_INPUT_BYTES = 10 * 1024 * 1024
MAX_ITEMS = 5000
MAX_INVARIANTS_PER_ISSUE = 200
MAX_TESTS_PER_INVARIANT = 200
SUPPORTED_INPUT_VERSIONS = {"1.0.0"}


def parse_args():
    parser = argparse.ArgumentParser(description="LedgerLens Traceability Generator")
    parser.add_argument("--input", required=True)
    parser.add_argument("--output", required=True)
    parser.add_argument("--format", choices=["json", "human"], default="json")
    parser.add_argument("--dry-run", action="store_true")
    parser.add_argument("--resume-checkpoint")
    parser.add_argument("--strict", action="store_true")
    return parser.parse_args()


def fail(exit_code, message):
    sys.stderr.write(message + "\n")
    sys.exit(exit_code)


def validate_issue(item, index):
    if not isinstance(item, dict):
        fail(EXIT_CORRUPT_INPUT, f"Error: item at index {index} is not an object")

    issue_id = item.get("issue_id")
    if not isinstance(issue_id, str) or not issue_id.strip():
        fail(EXIT_CORRUPT_INPUT, f"Error: item at index {index} missing valid 'issue_id'")

    invariants = item.get("invariants")
    if not isinstance(invariants, list):
        fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' has invalid 'invariants' field")

    if len(invariants) > MAX_INVARIANTS_PER_ISSUE:
        fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' exceeds max invariants of {MAX_INVARIANTS_PER_ISSUE}")

    seen_invariant_ids = set()
    for inv_index, inv in enumerate(invariants):
        if not isinstance(inv, dict):
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' invariant at index {inv_index} is not an object")

        inv_id = inv.get("invariant_id")
        if not isinstance(inv_id, str) or not inv_id.strip():
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' invariant at index {inv_index} missing valid 'invariant_id'")

        if inv_id in seen_invariant_ids:
            fail(EXIT_CORRUPT_INPUT, f"Error: issue '{issue_id}' has duplicate invariant_id '{inv_id}'")
        seen_invariant_ids.add(inv_id)

        test_ids = inv.get("test_ids", [])
        if not isinstance(test_ids, list):
            fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' has invalid 'test_ids' field")

        if len(test_ids) > MAX_TESTS_PER_INVARIANT:
            fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' exceeds max test_ids of {MAX_TESTS_PER_INVARIANT}")

        for t_index, t_id in enumerate(test_ids):
            if not isinstance(t_id, (str, int)):
                fail(EXIT_CORRUPT_INPUT, f"Error: invariant '{inv_id}' in issue '{issue_id}' has invalid test_id at index {t_index}")

    return issue_id


def validate_input_file(path):
    if not os.path.exists(path):
        fail(EXIT_CORRUPT_INPUT, f"Error: File not found: {path}")

    file_size = os.path.getsize(path)
    if file_size > MAX_INPUT_BYTES:
        fail(EXIT_CORRUPT_INPUT, f"Error: Input file size {file_size} exceeds limit of {MAX_INPUT_BYTES} bytes")

    try:
        with open(path, "r", encoding="utf-8") as f:
            raw = json.load(f)
    except Exception as e:
        fail(EXIT_CORRUPT_INPUT, f"Error parsing JSON input: {str(e)}")

    if isinstance(raw, dict):
        version = raw.get("schema_version")
        if version not in SUPPORTED_INPUT_VERSIONS:
            fail(EXIT_VERSION_MISMATCH, f"Error: unsupported input schema_version '{version}'")
        data = raw.get("issues")
        if not isinstance(data, list):
            fail(EXIT_CORRUPT_INPUT, "Error: 'issues' field must be a list")
    elif isinstance(raw, list):
        data = raw
    else:
        fail(EXIT_CORRUPT_INPUT, "Error: root input JSON structure must be a list or a versioned object")
        return

    if len(data) > MAX_ITEMS:
        fail(EXIT_CORRUPT_INPUT, f"Error: Exceeded maximum item limit of {MAX_ITEMS}")

    seen_issue_ids = set()
    for index, item in enumerate(data):
        issue_id = validate_issue(item, index)
        if issue_id in seen_issue_ids:
            fail(EXIT_CORRUPT_INPUT, f"Error: duplicate issue_id '{issue_id}' at index {index}")
        seen_issue_ids.add(issue_id)

    return data


def build_report(processed_items, total_invariants, covered_invariants, all_test_ids):
    total_issues = len(processed_items)
    total_tests = len(all_test_ids)
    coverage = (covered_invariants / total_invariants * 100.0) if total_invariants > 0 else 0.0
    status = "PASSED" if (total_invariants == covered_invariants) else "FAILED"
    return {
        "version": "1.0.0",
        "timestamp": int(time.time()),
        "summary": {
            "total_issues": total_issues,
            "total_invariants": total_invariants,
            "total_tests": total_tests,
            "coverage_percentage": round(coverage, 2),
            "status": status
        },
        "items": processed_items
    }


def atomic_write(file_path, content):
    dir_name = os.path.dirname(os.path.abspath(file_path))
    os.makedirs(dir_name, exist_ok=True)
    fd, temp_name = tempfile.mkstemp(dir=dir_name)
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as tf:
            tf.write(content)
        os.replace(temp_name, file_path)
    except Exception:
        if os.path.exists(temp_name):
            os.remove(temp_name)
        raise


def generate_human_readable(report):
    lines = []
    lines.append("=== LedgerLens Traceability Report ===")
    lines.append(f"Status: {report['summary']['status']}")
    lines.append(f"Coverage: {report['summary']['coverage_percentage']}%")
    lines.append(f"Total Issues: {report['summary']['total_issues']}")
    lines.append(f"Total Invariants: {report['summary']['total_invariants']}")
    lines.append(f"Total Tests Executed: {report['summary']['total_tests']}")
    lines.append("=" * 38)
    for item in report["items"]:
        lines.append(f"\nIssue [{item['issue_id']}]: {item['title']}")
        for inv in item["invariants"]:
            v_str = "VERIFIED" if inv["verified"] else "MISSING TEST"
            lines.append(f"  - Invariant [{inv['invariant_id']}]: {inv['description']} ({v_str})")
            for t_id in inv["test_ids"]:
                lines.append(f"      * Test: {t_id}")
    return "\n".join(lines) + "\n"


def load_checkpoint(path):
    if not path or not os.path.exists(path):
        return None
    try:
        with open(path, "r", encoding="utf-8") as f:
            data = json.load(f)
    except Exception as e:
        fail(EXIT_CHECKPOINT_ERROR, f"Error reading checkpoint: {str(e)}")
        return None
    if not isinstance(data, dict) or "last_index" not in data or "processed_items" not in data:
        fail(EXIT_CHECKPOINT_ERROR, "Error: malformed checkpoint file")
    if not isinstance(data["last_index"], int) or not isinstance(data["processed_items"], list):
        fail(EXIT_CHECKPOINT_ERROR, "Error: malformed checkpoint file")
    return data


def write_checkpoint(path, last_index, processed_items):
    content = json.dumps({"last_index": last_index, "processed_items": processed_items}, sort_keys=True)
    atomic_write(path, content)


def clear_checkpoint(path):
    if path and os.path.exists(path):
        os.remove(path)


def main():
    args = parse_args()
    raw_data = validate_input_file(args.input)

    checkpoint_data = load_checkpoint(args.resume_checkpoint)

    processed_items = []
    total_invariants = 0
    covered_invariants = 0
    all_test_ids = set()
    start_index = 0

    sorted_items = sorted(raw_data, key=lambda x: x["issue_id"])

    if checkpoint_data:
        start_index = checkpoint_data["last_index"] + 1
        processed_items = checkpoint_data["processed_items"]

        if start_index > len(sorted_items):
            fail(EXIT_CHECKPOINT_ERROR, "Error: checkpoint index exceeds current input size")

        expected_prefix = [item["issue_id"] for item in sorted_items[:start_index]]
        actual_prefix = [item.get("issue_id") for item in processed_items]
        if expected_prefix != actual_prefix:
            fail(EXIT_CHECKPOINT_ERROR, "Error: checkpoint does not match current input data")

        for existing in processed_items:
            for inv in existing.get("invariants", []):
                total_invariants += 1
                if inv.get("verified"):
                    covered_invariants += 1
                for t_id in inv.get("test_ids", []):
                    all_test_ids.add(t_id)

    for index in range(start_index, len(sorted_items)):
        item = sorted_items[index]
        issue_id = item["issue_id"]
        title = str(item.get("title", ""))
        sorted_invariants = sorted(item.get("invariants", []), key=lambda x: x["invariant_id"])

        processed_invariants = []
        for inv in sorted_invariants:
            inv_id = inv["invariant_id"]
            desc = str(inv.get("description", ""))
            test_ids = sorted(str(t) for t in inv.get("test_ids", []))
            is_verified = len(test_ids) > 0
            total_invariants += 1
            if is_verified:
                covered_invariants += 1
            for t_id in test_ids:
                all_test_ids.add(t_id)
            processed_invariants.append({
                "invariant_id": inv_id,
                "description": desc,
                "test_ids": test_ids,
                "verified": is_verified
            })

        processed_items.append({
            "issue_id": issue_id,
            "title": title,
            "invariants": processed_invariants
        })

        if args.resume_checkpoint and not args.dry_run:
            write_checkpoint(args.resume_checkpoint, index, processed_items)

    report = build_report(processed_items, total_invariants, covered_invariants, all_test_ids)

    if args.strict and report["summary"]["status"] != "PASSED":
        if not args.dry_run:
            output_content = json.dumps(report, indent=2, sort_keys=True) if args.format == "json" else generate_human_readable(report)
            atomic_write(args.output, output_content)
            clear_checkpoint(args.resume_checkpoint)
        fail(EXIT_TRACEABILITY_GAP, "Strict Mode Error: Traceability gaps found.")

    if args.dry_run:
        sys.stdout.write("Dry-run execution completed successfully. No files written.\n")
        sys.exit(EXIT_SUCCESS)

    output_content = json.dumps(report, indent=2, sort_keys=True) if args.format == "json" else generate_human_readable(report)
    atomic_write(args.output, output_content)
    clear_checkpoint(args.resume_checkpoint)
    sys.exit(EXIT_SUCCESS)


if __name__ == "__main__":
    main()
