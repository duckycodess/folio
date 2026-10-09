#!/usr/bin/env python3
"""Writes a Model Lab results table to the job summary.

Usage: model_lab_summary.py RESULTS.json

It only reports what the records say. Model task outcomes are data, not job
failures, and summaries stay Not reviewed until a person reviews them.
"""
import json
import os
import sys


def peak(record):
    parts = []
    for entry in record["memory"]:
        if entry["peakBytes"] is not None:
            parts.append(f'{entry["process"]} {entry["peakBytes"]:,} B')
        else:
            parts.append(f'{entry["process"]} unavailable ({entry.get("unavailableReason", "no reason")})')
    return "; ".join(parts) or "unavailable"


def outcome(record):
    if record["correctness"] is None:
        return "Not reviewed" if record["task"] == "summary" else "not graded"
    return "matches labels" if record["correctness"] else "does not match labels"


def main(path):
    data = json.load(open(path, encoding="utf-8"))
    lines = ["### Model Lab results", ""]
    for run in data["runs"]:
        host = run["host"]
        lines.append(
            f'Run `{run["runId"]}`: **{run["status"]}**. Host: {host["os"]} {host["arch"]}, '
            f'{host.get("cpuBrand") or "CPU model unavailable"}, '
            f'{host["logicalCpus"]} logical CPUs, installed RAM {host.get("installedRamBytes")} B (capacity, not usage).'
        )
        if run.get("error"):
            lines.append(f'Run error: {run["error"]}')
    lines += [
        "",
        "Hosted-runner measurements: not an 8 GB device, not installed size, not desktop or GUI evidence. "
        "A task a model got wrong is a recorded measurement, not a failed job. Summaries are Not reviewed. "
        "One cold/repeat pair per case is an initial observation, not a stable estimate. "
        "Peaks are process-lifetime peaks, not per-task memory.",
        "",
        "| Task | Case | Model | Request | Start ms | Task ms | Outcome (labels) | Peak memory |",
        "| --- | --- | --- | --- | ---: | ---: | --- | --- |",
    ]
    for r in data["records"]:
        position = "first after restart" if r["cold"] else "immediate repeat"
        start = r["timing"]["processStartMs"]
        lines.append(
            f'| {r["task"]} | {r["caseId"]} | `{r["modelId"]}` | {position} | '
            f'{"n/a" if start is None else start} | {r["taskDurationMs"]} | {outcome(r)} | {peak(r)} |'
        )
    text = "\n".join(lines) + "\n"
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(text)


if __name__ == "__main__":
    main(sys.argv[1])
