#!/usr/bin/env python3
"""
Proof-of-Concept: Soroban RPC Event Exporter for Replay & Forensics

Fetches contract events/transactions directly from a Soroban RPC endpoint
(e.g., testnet) and outputs them as a frozen NDJSON snapshot for tools/replay.
"""

import argparse
import json
import sys
import time
import urllib.error
import urllib.request

DEFAULT_RPC_URL = "https://soroban-testnet.stellar.org/"

def fetch_events(rpc_url, contract_id, start_ledger, limit=100):
    events = []
    cursor = None

    headers = {
        "Content-Type": "application/json",
        "User-Agent": "LedgerLens-Replay-PoC/1.0"
    }

    while True:
        params = {
            "filters": [{"type": "contract"}]
        }
        if contract_id:
            params["filters"][0]["contractIds"] = [contract_id]

        if cursor:
            params["pagination"] = {"cursor": cursor, "limit": limit}
        else:
            params["startLedger"] = start_ledger
            params["pagination"] = {"limit": limit}

        payload = {
            "jsonrpc": "2.0",
            "id": 1,
            "method": "getEvents",
            "params": params
        }

        data = json.dumps(payload).encode("utf-8")
        req = urllib.request.Request(rpc_url, data=data, headers=headers)

        try:
            with urllib.request.urlopen(req) as response:
                res = json.loads(response.read().decode("utf-8"))
        except urllib.error.URLError as e:
            print(f"RPC Error: {e}", file=sys.stderr)
            sys.exit(1)

        if "error" in res:
            print(f"RPC Response Error: {res['error']}", file=sys.stderr)
            sys.exit(1)

        result = res.get("result", {})
        page_events = result.get("events", [])
        if not page_events:
            break

        events.extend(page_events)

        # Check cursor for pagination
        latest_cursor = result.get("cursor")
        if not latest_cursor or latest_cursor == cursor or len(page_events) < limit:
            break
        cursor = latest_cursor
        time.sleep(0.1) # Small delay to respect rate limits

    return events

def main():
    parser = argparse.ArgumentParser(description="Export Soroban RPC events to NDJSON snapshot format.")
    parser.add_argument("--rpc-url", default=DEFAULT_RPC_URL, help="Soroban RPC endpoint URL")
    parser.add_argument("--contract-id", help="Target Soroban contract ID (optional)")
    parser.add_argument("--start-ledger", type=int, default=4324800, help="Start ledger number")
    parser.add_argument("--output", default="testnet_snapshot.ndjson", help="Output NDJSON file path")

    args = parser.parse_args()

    print(f"Fetching events from {args.rpc_url} starting at ledger {args.start_ledger}...")
    raw_events = fetch_events(args.rpc_url, args.contract_id, args.start_ledger)
    print(f"Fetched {len(raw_events)} events.")

    count = 0
    with open(args.output, "w") as f:
        for item in raw_events:
            ndjson_entry = {
                "event_id": item.get("id"),
                "ledger": item.get("ledger"),
                "ledger_closed_at": item.get("ledgerClosedAt"),
                "contract_id": item.get("contractId"),
                "tx_hash": item.get("txHash"),
                "topic": item.get("topic"),
                "value": item.get("value"),
                "in_successful_contract_call": item.get("inSuccessfulContractCall")
            }
            f.write(json.dumps(ndjson_entry) + "\n")
            count += 1

    print(f"Successfully exported {count} snapshot records to {args.output}")

if __name__ == "__main__":
    main()
