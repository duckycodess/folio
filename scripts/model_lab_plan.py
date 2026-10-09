#!/usr/bin/env python3
"""Checks a Model Lab dispatch before anything is downloaded.

Fails, without downloading, when a requested model or runtime is not in the
pinned manifest, a model is repeated, or the disk cannot hold the files. The
byte totals it prints are informational: there is no download cap, and nothing
is substituted for a model that was asked for.

Environment: LAB_EMBEDDING, LAB_MODELS (comma separated), LAB_RUNTIME_ID,
LAB_DATA_DIR (where files will be written), GITHUB_STEP_SUMMARY (optional).
"""
import json
import os
import shutil
import sys

MANIFEST = os.path.join("src-tauri", "resources", "model-manifest.json")


def fail(message):
    print(f"::error::{message}")
    sys.exit(1)


def main():
    manifest = json.load(open(MANIFEST, encoding="utf-8"))
    models = {m["id"]: m for m in manifest["models"]}
    runtimes = {r["id"]: r for r in manifest["runtimes"]}

    embedding = os.environ["LAB_EMBEDDING"].strip()
    generation = [i.strip() for i in os.environ["LAB_MODELS"].split(",") if i.strip()]
    runtime_id = os.environ["LAB_RUNTIME_ID"].strip()

    if not generation:
        fail("Choose at least one generation model.")
    if len(set(generation)) != len(generation):
        fail("A generation model was chosen twice.")
    if embedding not in models or models[embedding]["role"] != "embedding":
        fail(f"{embedding!r} is not an embedding model in the pinned manifest.")
    for model_id in generation:
        if model_id not in models or models[model_id]["role"] != "generation":
            fail(f"{model_id!r} is not a generation model in the pinned manifest.")
    if runtime_id not in runtimes:
        fail(f"{runtime_id!r} is not a runtime in the pinned manifest.")

    def size(item):
        return sum(f["bytes"] for f in item["files"])

    rows = [("embedding model", embedding, size(models[embedding]))]
    rows += [("generation model", i, size(models[i])) for i in generation]
    rows.append(("llama.cpp runtime archive", runtime_id, size(runtimes[runtime_id])))
    total = sum(r[2] for r in rows)

    lines = [
        "### Model Lab download plan",
        "",
        "Pinned manifest sizes, shown for information. There is no download cap; each file is size- and SHA-256-verified when installed.",
        "",
        "| Item | Id | Bytes |",
        "| --- | --- | ---: |",
    ]
    lines += [f"| {kind} | `{item}` | {n:,} |" for kind, item, n in rows]
    lines.append(f"| **total** | | **{total:,}** |")

    data_dir = os.environ["LAB_DATA_DIR"]
    os.makedirs(data_dir, exist_ok=True)
    free = shutil.disk_usage(data_dir).free
    needed = 2 * total  # downloads plus extraction headroom
    lines += ["", f"Free disk at `{data_dir}`: {free:,} bytes; needed for this plan: {needed:,} bytes."]
    summary = os.environ.get("GITHUB_STEP_SUMMARY")
    text = "\n".join(lines) + "\n"
    print(text)
    if summary:
        with open(summary, "a", encoding="utf-8") as handle:
            handle.write(text)
    if free < needed:
        fail(f"Not enough free disk: {free:,} bytes free, {needed:,} needed.")


if __name__ == "__main__":
    main()
