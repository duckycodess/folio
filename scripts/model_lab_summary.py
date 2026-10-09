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


def offload(record):
    backend = (record["runtimeDetail"] or {}).get("backend")
    if not backend:
        return "n/a (in-process)"
    done = backend.get("gpuLayersOffloaded")
    total = backend.get("layersTotal")
    seen = "not reported" if done is None else f"{done}/{total} layers on GPU"
    return f'asked {backend["gpuOffload"]}; server {seen}'


def catalog(record):
    return "evaluation candidate" if record["model"]["evaluationOnly"] else "product"


def outcome(record):
    if record["outcomeKind"] != "valid":
        return f'FAILED: {record["outcomeKind"]} (retry needed)' if record["retryNeeded"] else record["outcomeKind"]
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
    counts = {}
    for r in data["records"]:
        per_model = counts.setdefault(r["modelId"], {})
        per_model[r["outcomeKind"]] = per_model.get(r["outcomeKind"], 0) + 1
    lines += ["", "Outcomes per model (a failed case is not an ungraded one):", ""]
    for model, kinds in counts.items():
        lines.append(f"- `{model}`: " + ", ".join(f"{k} {n}" for k, n in sorted(kinds.items())))
    lines += [
        "",
        "Hosted-runner measurements: not an 8 GB device, not installed size, not desktop or GUI evidence. "
        "A task a model got wrong is a recorded measurement, not a failed job. Summaries are Not reviewed. "
        "One cold/repeat pair per case is an initial observation, not a stable estimate. "
        "Peaks are process-lifetime peaks, not per-task memory.",
        "",
        "| Task | Case | Model | Catalog | Request | Start ms | Task ms | Outcome (labels) | GPU offload | Peak memory |",
        "| --- | --- | --- | --- | --- | ---: | ---: | --- | --- | --- |",
    ]
    for r in data["records"]:
        position = "first after restart" if r["cold"] else "immediate repeat"
        start = r["timing"]["processStartMs"]
        lines.append(
            f'| {r["task"]} | {r["caseId"]} | `{r["modelId"]}` | {catalog(r)} | {position} | '
            f'{"n/a" if start is None else start} | {r["taskDurationMs"]} | {outcome(r)} | {offload(r)} | {peak(r)} |'
        )
    text = "\n".join(lines) + "\n"
    print(text)
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(text)


if __name__ == "__main__":
    main(sys.argv[1])
