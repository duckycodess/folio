#!/usr/bin/env python3
"""Sample a process tree's resident memory for bounded R8 evidence."""

from __future__ import annotations

import json
import sys
import time
from pathlib import Path


INTERVAL_SECONDS = 0.25


def process_snapshot() -> dict[int, tuple[int, str, int]]:
    processes: dict[int, tuple[int, str, int]] = {}
    for entry in Path("/proc").glob("[0-9]*"):
        try:
            status = (entry / "status").read_text(encoding="utf-8")
        except (FileNotFoundError, PermissionError):
            continue
        parent = None
        name = entry.name
        rss_kb = 0
        for line in status.splitlines():
            if line.startswith("Name:"):
                name = line.split("\t", 1)[-1].strip()
            elif line.startswith("PPid:"):
                parent = int(line.split()[1])
            elif line.startswith("VmRSS:"):
                rss_kb = int(line.split()[1])
        if parent is not None:
            processes[int(entry.name)] = (parent, name, rss_kb)
    return processes


def tree_snapshot(root_pid: int) -> list[dict[str, int | str]]:
    processes = process_snapshot()
    children: dict[int, list[int]] = {}
    for pid, (parent, _, _) in processes.items():
        children.setdefault(parent, []).append(pid)
    pids = {root_pid}
    pending = [root_pid]
    while pending:
        parent = pending.pop()
        for child in children.get(parent, []):
            if child not in pids:
                pids.add(child)
                pending.append(child)
    result = []
    for pid in sorted(pids):
        if pid in processes:
            parent, name, rss_kb = processes[pid]
            result.append({"pid": pid, "ppid": parent, "name": name, "rssKb": rss_kb})
    return result


def main() -> None:
    if len(sys.argv) != 3:
        raise SystemExit("usage: r8_monitor_rss.py PID OUTPUT_JSON")
    root_pid = int(sys.argv[1])
    output_path = Path(sys.argv[2])
    peak_rss = 0
    peak_processes: list[dict[str, int | str]] = []
    samples = 0
    while True:
        processes = tree_snapshot(root_pid)
        if processes:
            samples += 1
            total = sum(int(process["rssKb"]) for process in processes)
            if total >= peak_rss:
                peak_rss = total
                peak_processes = processes
        if not Path(f"/proc/{root_pid}").exists():
            break
        time.sleep(INTERVAL_SECONDS)
    final_processes = tree_snapshot(root_pid)
    if final_processes:
        samples += 1
        total = sum(int(process["rssKb"]) for process in final_processes)
        if total >= peak_rss:
            peak_rss = total
            peak_processes = final_processes
    output_path.parent.mkdir(parents=True, exist_ok=True)
    output_path.write_text(
        json.dumps(
            {
                "rootPid": root_pid,
                "samplingIntervalMs": int(INTERVAL_SECONDS * 1000),
                "samples": samples,
                "peakAggregateRssKb": peak_rss,
                "peakProcesses": peak_processes,
                "observation": "process tree RSS only; not whole-device RAM",
            },
            indent=2,
        )
        + "\n",
        encoding="utf-8",
    )


if __name__ == "__main__":
    main()
