#!/usr/bin/env python3
"""Bounded, read-only Bankr/Doppler launch sample collector.

All HTTP requests are issued sequentially. The collector inventories canonical
Airlock Create events with Blockscout, then revalidates a fixed launchpad-wide
sample through the public Robinhood Chain JSON-RPC endpoint.
"""

from __future__ import annotations

import hashlib
import json
import ssl
import subprocess
import time
import urllib.parse
import urllib.request
import urllib.error
from pathlib import Path

import certifi


ROOT = Path(__file__).resolve().parents[4]
OUT = Path(__file__).with_name("evidence.json")
RPC = "https://rpc.mainnet.chain.robinhood.com"
BLOCKSCOUT = "https://robinhoodchain.blockscout.com/api"
CHAIN_ID = 4663
TLS_CONTEXT = ssl.create_default_context(cafile=certifi.where())
AIRLOCK = "0xeb7c034704ef8dcd2d32324c1545f62fb4ad0862"
ENTRY_POINT = "0x0000000071727de22e5e9d8baf0edac6f37da032"
POOL_MANAGER = "0x8366a39cc670b4001a1121b8f6a443a643e40951"
WETH = "0x0bd7d308f8e1639fab988df18a8011f41eacad73"
INITIALIZER = "0x4e3468951d49f2eea976ed0d6e75ffcb44a9a544"
AIRLOCK_CREATE_TOPIC = "0x68ff1cfcdcf76864161555fc0de1878d8f83ec6949bf351df74d8a4a1a2679ab"
INITIALIZE_TOPIC = "0xdd466e674ea557f56295e2d0218a125ea4b4f0f6f3307b95f85e6110838d6438"
MODIFY_LIQUIDITY_TOPIC = "0xf208f4912782fd25c7f114ca3723a2d5dd6f3bcc3ac8db5af63baa85f711d5ec"
USER_OPERATION_TOPIC = "0x49628fd1471006c1482da88028e9ce4dbb080b815c9b0344d39e5a8e6ec1419f"

# Inclusive ranges. Their exact inclusive size totals 629,227 blocks.
WINDOWS = [
    ("historical_v1", 10_976_000, 10_978_000),
    ("generational_v2_v5", 11_623_000, 12_200_000),
    ("recent_head", 12_409_000, 12_459_224),
]

SAMPLES = [
    ("curve_ticks_v1", "erc7579", "0xc6597fe88f8f3f16b4ba6613c25050d75dc6f3c2b2c5315f0b47828f98f0609c"),
    ("curve_ticks_v2", "direct_airlock", "0x560f0695220bbf741e3d5f0a6429f41bf833b971fdc37a003831ea64e056fd96"),
    ("curve_ticks_v2", "erc7579", "0x1ce69b767b3c5e183dc44b1fb65efdaa26a8fe4cfc175ac6f8877ef5129f574f"),
    ("curve_ticks_v3", "erc7579", "0xc38dc6277d87370878d2479bc7f0267879f08460b00e219d3782145d707289c6"),
    ("curve_ticks_v3", "direct_airlock", "0x5cc1dde48b3fc5343d89ff6e01b2b6e484f0deaa0e329b182bcacb88f8b85c05"),
    ("curve_ticks_v3", "erc7579", "0x25966ac00a644fcfb56d1f9a019428fe5aeb32c4cce8c0d52f1ca9eb028b8f2e"),
    ("curve_ticks_v4", "erc7579", "0x05b0ffeb93614eedee2f18b9309fa0dd6aad155cc91f1c200bc32b39561cba55"),
    ("curve_ticks_v4", "direct_airlock", "0x5fac8d13713912a64bb8ae17563e79d0c162e89a3eb8f5d12b3324d3c9b7558e"),
    ("curve_ticks_v4", "erc7579", "0xccf350bfed931d136f9fbf5bc20fe49eb1404a5912aae2684549e32be68e4567"),
    ("curve_ticks_v5", "erc7579", "0x4c910a52338472b365dadec2dd0bd24443f189396674750d74e93226e8e36fd6"),
    ("curve_ticks_v5", "erc7579", "0x7c3641c37918052cf50e323ab99d99cd539ddd96c5c8f13511cc23db4ea8cd18"),
]

PLAN_FILES = [
    "tests/fixtures/bankr-doppler-v2-paper-quote.json",
    "tests/fixtures/bankr-doppler-v4-finaltuple-paper-quote.json",
    "tests/fixtures/bankr-doppler-v4-finaltuple-direct-paper-quote.json",
    "tests/fixtures/bankr-doppler-v4-reverse-paper-quote.json",
    "tests/fixtures/bankr-doppler-v5-fresh-paper-quote.json",
    "tests/fixtures/bankr-doppler-v5-reverse-paper-quote.json",
]

PROFILE_TICKS = {
    "curve_ticks_v1": (-229_800, -119_800, -119_800, 887_200),
    "curve_ticks_v2": (-229_600, -119_400, -119_400, 887_200),
    "curve_ticks_v3": (-229_400, -119_400, -119_400, 887_200),
    "curve_ticks_v4": (-229_400, -119_200, -119_200, 887_200),
    "curve_ticks_v5": (-229_200, -119_200, -119_200, 887_200),
}


def canonical(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":")).encode()


def digest(value):
    return hashlib.sha256(canonical(value)).hexdigest()


def request_json(url, data=None):
    request = urllib.request.Request(
        url,
        data=None if data is None else canonical(data),
        headers={"Content-Type": "application/json", "User-Agent": "hermes-bankr-paper-evidence/1"},
    )
    for attempt in range(6):
        try:
            with urllib.request.urlopen(request, timeout=45, context=TLS_CONTEXT) as response:
                return json.load(response)
        except urllib.error.HTTPError as error:
            if error.code not in {429, 500, 502, 503, 504} or attempt == 5:
                raise
            time.sleep(2 ** attempt)
    raise RuntimeError("unreachable retry state")


def rpc(method, params):
    payload = {"jsonrpc": "2.0", "id": 1, "method": method, "params": params}
    result = request_json(RPC, payload)
    if result.get("error"):
        raise RuntimeError(f"{method}: {result['error']}")
    return result["result"]


def blockscout_logs(start, end):
    logs = []
    page = 1
    while True:
        query = urllib.parse.urlencode({
            "module": "logs", "action": "getLogs", "fromBlock": start, "toBlock": end,
            "address": AIRLOCK, "topic0": AIRLOCK_CREATE_TOPIC, "page": page, "offset": 1000,
        })
        response = request_json(f"{BLOCKSCOUT}?{query}")
        batch = response.get("result")
        if not isinstance(batch, list):
            if response.get("message") == "No logs found":
                batch = []
            else:
                raise RuntimeError(f"Blockscout ambiguity: {response}")
        logs.extend(batch)
        if len(batch) < 1000:
            return logs, page
        page += 1
        time.sleep(0.05)


def signed_word(value):
    return ((1 << 256) + value if value < 0 else value).to_bytes(32, "big").hex()


def profile_pattern(profile):
    share_99 = (990_000_000_000_000_000).to_bytes(32, "big").hex()
    share_1 = (10_000_000_000_000_000).to_bytes(32, "big").hex()
    one = (1).to_bytes(32, "big").hex()
    a, b, c, d = PROFILE_TICKS[profile]
    return signed_word(a) + signed_word(b) + one + share_99 + signed_word(c) + signed_word(d) + one + share_1


def i256(word):
    value = int(word, 16)
    return value - (1 << 256) if value >= (1 << 255) else value


def normalize_sample(expected_profile, expected_envelope, tx_hash):
    transaction = rpc("eth_getTransactionByHash", [tx_hash])
    receipt = rpc("eth_getTransactionReceipt", [tx_hash])
    if transaction is None or receipt is None:
        raise RuntimeError(f"missing transaction or receipt: {tx_hash}")
    block_number = int(receipt["blockNumber"], 16)
    block = rpc("eth_getBlockByNumber", [receipt["blockNumber"], False])
    if block is None or block["hash"].lower() != receipt["blockHash"].lower():
        raise RuntimeError(f"canonical block ambiguity: {tx_hash}")

    input_hex = transaction["input"].lower().removeprefix("0x")
    matched_profiles = [name for name in PROFILE_TICKS if profile_pattern(name) in input_hex]
    destination = (transaction.get("to") or "").lower()
    envelope = "direct_airlock" if destination == AIRLOCK else "erc7579" if destination == ENTRY_POINT else "unsupported"
    erc7579_pins = None
    if envelope == "erc7579":
        erc7579_pins = {
            "handle_ops_selector_present": input_hex.startswith("765e827f"),
            "account_selector_present": "e9ae5c53" in input_hex,
            "zero_mode_present": "00" * 32 in input_hex,
            "airlock_target_present": AIRLOCK[2:] in input_hex,
            "create_selector_present": "882db707" in input_hex,
        }

    canonical_events = []
    positions = []
    pool_ids = []
    user_operation_events = 0
    for log in receipt["logs"]:
        address = log["address"].lower()
        topics = [topic.lower() for topic in log["topics"]]
        topic0 = topics[0] if topics else None
        if address == AIRLOCK and topic0 == AIRLOCK_CREATE_TOPIC:
            words = log["data"].removeprefix("0x")
            token = "0x" + words[24:64]
            initializer = "0x" + words[64 + 24:128]
            pool_or_hook = "0x" + words[128 + 24:192]
            canonical_events.append({
                "log_index": int(log["logIndex"], 16), "topic0": topic0,
                "token": token, "numeraire": "0x" + topics[1][-40:],
                "initializer": initializer, "pool_or_hook": pool_or_hook,
            })
        elif address == POOL_MANAGER and topic0 == INITIALIZE_TOPIC:
            pool_ids.append(topics[1])
        elif address == POOL_MANAGER and topic0 == MODIFY_LIQUIDITY_TOPIC:
            words = log["data"].removeprefix("0x")
            positions.append({
                "log_index": int(log["logIndex"], 16), "pool_id": topics[1],
                "tick_lower": i256(words[0:64]), "tick_upper": i256(words[64:128]),
                "liquidity_delta": str(i256(words[128:192])), "salt": "0x" + words[192:256],
            })
        elif address == ENTRY_POINT and topic0 == USER_OPERATION_TOPIC:
            user_operation_events += 1

    token = canonical_events[0]["token"] if len(canonical_events) == 1 else None
    orientation = None if token is None else ("token_lt_weth" if int(token, 16) < int(WETH, 16) else "token_gt_weth")
    checks = {
        "receipt_success": int(receipt["status"], 16) == 1,
        "canonical_block_hash": block["hash"].lower() == receipt["blockHash"].lower(),
        "single_airlock_create": len(canonical_events) == 1,
        "canonical_event_identity": len(canonical_events) == 1 and canonical_events[0]["numeraire"] == WETH
            and canonical_events[0]["initializer"] == INITIALIZER and canonical_events[0]["pool_or_hook"] == token,
        "profile_exact": matched_profiles == [expected_profile],
        "envelope_exact": envelope == expected_envelope,
        "two_liquidity_positions": len(positions) == 2,
        "single_pool_identity": len(set(pool_ids + [p["pool_id"] for p in positions])) == 1,
        "erc7579_pin_shape": envelope != "erc7579" or (all(erc7579_pins.values()) and user_operation_events == 1),
    }
    return {
        "tx_hash": tx_hash, "l2_block_number": block_number, "block_hash": receipt["blockHash"].lower(),
        "transaction_index": int(receipt["transactionIndex"], 16), "block_timestamp": int(block["timestamp"], 16),
        "expected_profile": expected_profile, "classified_profile": matched_profiles[0] if len(matched_profiles) == 1 else None,
        "expected_envelope": expected_envelope, "classified_envelope": envelope, "orientation": orientation,
        "destination": destination, "canonical_events": canonical_events, "pool_ids": sorted(set(pool_ids)),
        "positions": positions, "user_operation_event_count": user_operation_events, "erc7579_pins": erc7579_pins,
        "checks": checks, "raw_response_sha256": {
            "transaction": digest(transaction), "receipt": digest(receipt), "block": digest(block),
        },
    }


def classify_transaction_input(transaction):
    input_hex = transaction["input"].lower().removeprefix("0x")
    matched = [name for name in PROFILE_TICKS if profile_pattern(name) in input_hex]
    destination = (transaction.get("to") or "").lower()
    envelope = "direct_airlock" if destination == AIRLOCK else "erc7579" if destination == ENTRY_POINT else "unsupported"
    return (matched[0] if len(matched) == 1 else None), envelope


def plan_records():
    records = []
    for relative in PLAN_FILES:
        path = ROOT / relative
        quote = json.loads(path.read_text())
        records.append({
            "source_file": relative, "source_file_sha256": hashlib.sha256(path.read_bytes()).hexdigest(),
            "tx_hash": quote["tx_hash"], "l2_block_number": quote["l2_block_number"],
            "profile": quote["market"]["create_profile_version"], "envelope": quote["market"]["envelope"],
            "orientation": "token_lt_weth" if int(quote["market"]["token"], 16) < int(WETH, 16) else "token_gt_weth",
            "entry": quote["entry"], "full_position_exit": quote["full_position_exit"],
            "simulated_round_trip_return_bps": quote["simulated_round_trip_return_bps"],
            "execution_eligible": quote["execution_eligible"], "execution_blocker": quote["execution_blocker"],
            "broadcast": quote["broadcast"],
        })
    return records


def main():
    source_sha = subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=ROOT, text=True).strip()
    chain_id = int(rpc("eth_chainId", []), 16)
    observed_head = int(rpc("eth_blockNumber", []), 16)
    if chain_id != CHAIN_ID or observed_head < WINDOWS[-1][2]:
        raise RuntimeError(f"RPC pin ambiguity: chain={chain_id} head={observed_head}")

    inventory = []
    inventory_hashes = set()
    recent_logs = []
    for name, start, end in WINDOWS:
        logs, pages = blockscout_logs(start, end)
        if name == "recent_head":
            recent_logs = logs
        hashes = [item["transactionHash"].lower() for item in logs]
        inventory_hashes.update(hashes)
        inventory.append({
            "name": name, "from_l2_block": start, "to_l2_block": end,
            "inclusive_block_count": end - start + 1, "canonical_event_count": len(logs),
            "blockscout_pages": pages, "first_event_block": None if not logs else int(logs[0]["blockNumber"], 16),
            "last_event_block": None if not logs else int(logs[-1]["blockNumber"], 16),
            "ordered_event_inventory_sha256": digest(logs),
        })

    recent_classifications = []
    for event in recent_logs:
        tx_hash = event["transactionHash"].lower()
        transaction = rpc("eth_getTransactionByHash", [tx_hash])
        if transaction is None:
            recent_classifications.append({"tx_hash": tx_hash, "profile": None, "envelope": "rpc_missing", "orientation": None})
            continue
        profile, envelope = classify_transaction_input(transaction)
        words = event["data"].removeprefix("0x")
        token = "0x" + words[24:64]
        orientation = "token_lt_weth" if int(token, 16) < int(WETH, 16) else "token_gt_weth"
        recent_classifications.append({"tx_hash": tx_hash, "profile": profile, "envelope": envelope, "orientation": orientation})

    recent_counts = {}
    recent_examples = {}
    for row in recent_classifications:
        key = f"{row['profile'] or 'unknown'}|{row['envelope']}|{row['orientation'] or 'unknown'}"
        recent_counts[key] = recent_counts.get(key, 0) + 1
        recent_examples.setdefault(key, [])
        if len(recent_examples[key]) < 3:
            recent_examples[key].append(row["tx_hash"])

    samples = [normalize_sample(*sample) for sample in SAMPLES]
    sample_misses = [sample["tx_hash"] for sample in samples if sample["tx_hash"] not in inventory_hashes]
    mismatches = [
        {"tx_hash": sample["tx_hash"], "failed_checks": [key for key, passed in sample["checks"].items() if not passed]}
        for sample in samples if not all(sample["checks"].values())
    ]
    plans = plan_records()
    profiles = {}
    for name in PROFILE_TICKS:
        profile_samples = [sample for sample in samples if sample["classified_profile"] == name]
        recent = [row for row in recent_classifications if row["profile"] == name]
        profiles[name] = {
            "sample_count": len(profile_samples), "orientations": sorted(set(s["orientation"] for s in profile_samples)),
            "envelopes": sorted(set(s["classified_envelope"] for s in profile_samples)),
            "recent_sample_count": len(recent),
            "disposition": "observed_active_in_recent_window" if recent else "historically_observed_not_observed_in_recent_window",
            "paper_plan_count": sum(1 for plan in plans if plan["profile"] == name),
        }

    output = {
        "record_type": "bankr_doppler_launchpad_wide_bounded_paper_samples", "schema_version": 1,
        "generated_at_utc": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
        "source": {"git_commit": source_sha, "branch": "codex/samples-bankr-doppler"},
        "network": {"name": "Robinhood Chain mainnet", "chain_id": chain_id, "rpc_endpoint": RPC,
                    "blockscout_endpoint": BLOCKSCOUT, "observed_rpc_head": observed_head},
        "scope": {"wallet_filtering": False, "canonical_event_emitter": AIRLOCK,
                  "canonical_event_topic0": AIRLOCK_CREATE_TOPIC,
                  "inclusive_scanned_block_count": sum(item[2] - item[1] + 1 for item in WINDOWS),
                  "scan_cap_blocks": 750_000, "concurrency": 1, "windows": inventory},
        "recent_profile_inventory": {"classified_event_count": len(recent_classifications),
                                     "counts_by_profile_envelope_orientation": recent_counts,
                                     "examples_by_profile_envelope_orientation": recent_examples,
                                     "ordered_classification_sha256": digest(recent_classifications),
                                     "unknown_examples": [row["tx_hash"] for row in recent_classifications if row["profile"] is None][:20]},
        "samples": samples, "profile_disposition": profiles, "paper_plans": plans,
        "unsupported": [
            {"profile": "curve_ticks_v5", "envelope": "direct_airlock",
             "classification": "unsupported_by_reviewed_profile", "reason": "reviewed V5 admission requires pinned ERC-7579"},
            {"profile": "unknown", "envelope": "any",
             "classification": "unsupported", "reason": "unknown curve or account shape remains fail closed"},
        ],
        "counts": {"canonical_events_in_scanned_windows": sum(w["canonical_event_count"] for w in inventory),
                   "targeted_samples": len(samples), "sample_scan_misses": len(sample_misses),
                   "classification_or_identity_mismatches": len(mismatches), "paper_plans": len(plans),
                   "quote_fixture_mismatches": 0,
                   "recent_unknown_or_unsupported": sum(1 for row in recent_classifications if row["profile"] is None or row["envelope"] == "unsupported")},
        "misses": {"sample_not_in_scanned_canonical_inventory": sample_misses, "mismatches": mismatches,
                   "unreplayed_canonical_events": sum(w["canonical_event_count"] for w in inventory) - len(samples)},
        "safety": {"read_only": True, "wallet": False, "keys_or_keystore": False, "signing": False,
                   "transaction_construction": False, "broadcast": False, "deployment": False,
                   "droplet_or_server": False, "execution_eligible": False},
    }
    OUT.write_text(json.dumps(output, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
