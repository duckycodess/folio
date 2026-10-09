#!/usr/bin/env python3
"""Fetch and verify the pinned public artifacts used by the R8 acceptance job."""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import subprocess
import tempfile
from pathlib import Path
from typing import Any


REQUESTS = (
    ("e5_model", "model", "multilingual-e5-small-int8", "onnx/model_quantized.onnx", "e5-model.onnx"),
    ("e5_tokenizer", "model", "multilingual-e5-small-int8", "tokenizer.json", "tokenizer.json"),
    (
        "llama_runtime",
        "runtime",
        "llama-b11524-ubuntu-x64",
        "llama-b11524-bin-ubuntu-x64.tar.gz",
        "llama-b11524-bin-ubuntu-x64.tar.gz",
    ),
)


DEFAULT_GENERATION_MODEL = "qwen3-0.6b-q4-k-m"


def generation_request(manifest: dict[str, Any]) -> tuple[str, str, str, str, str]:
    """The pinned GGUF of the generation model chosen by FOLIO_R8_GENERATION_MODEL."""
    model_id = os.environ.get("FOLIO_R8_GENERATION_MODEL") or DEFAULT_GENERATION_MODEL
    item = descriptor(manifest, "model", model_id)
    ggufs = [file["path"] for file in item["files"] if file["path"].endswith(".gguf")]
    if len(ggufs) != 1:
        raise ValueError(f"{model_id} must pin exactly one GGUF file")
    return ("qwen_model", "model", model_id, ggufs[0], ggufs[0])


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser()
    parser.add_argument("--manifest", type=Path, required=True)
    parser.add_argument("--output-dir", type=Path, required=True)
    parser.add_argument("--metadata", type=Path, required=True)
    return parser.parse_args()


def descriptor(manifest: dict[str, Any], kind: str, model_id: str) -> dict[str, Any]:
    collection = "models" if kind == "model" else "runtimes"
    for item in manifest[collection]:
        if item["id"] == model_id:
            return item
    raise ValueError(f"{model_id} is missing from manifest.{collection}")


def manifest_file(item: dict[str, Any], path: str) -> dict[str, Any]:
    for file in item["files"]:
        if file["path"] == path:
            return file
    raise ValueError(f"{path} is missing from manifest entry {item['id']}")


def sha256(path: Path) -> str:
    digest = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            digest.update(block)
    return digest.hexdigest()


def verify(path: Path, expected: dict[str, Any]) -> None:
    if not path.is_file():
        raise ValueError(f"{path} does not exist")
    actual_bytes = path.stat().st_size
    if actual_bytes != expected["bytes"]:
        raise ValueError(
            f"{path} has {actual_bytes} bytes; expected {expected['bytes']}"
        )
    actual_sha256 = sha256(path)
    if actual_sha256 != expected["sha256"]:
        raise ValueError(
            f"{path} has SHA-256 {actual_sha256}; expected {expected['sha256']}"
        )


def fetch(path: Path, expected: dict[str, Any]) -> None:
    try:
        verify(path, expected)
        return
    except ValueError:
        pass

    url = expected.get("downloadUrl")
    if not url:
        raise ValueError(f"manifest does not provide a download URL for {path.name}")
    temporary_fd, temporary_name = tempfile.mkstemp(
        dir=path.parent, prefix=f".{path.name}.", suffix=".part"
    )
    os.close(temporary_fd)
    temporary = Path(temporary_name)
    try:
        subprocess.run(
            [
                "curl",
                "--fail",
                "--location",
                "--retry",
                "3",
                "--retry-delay",
                "2",
                "--connect-timeout",
                "30",
                "--max-time",
                "900",
                "--silent",
                "--show-error",
                "--output",
                str(temporary),
                url,
            ],
            check=True,
        )
        verify(temporary, expected)
        os.replace(temporary, path)
    finally:
        temporary.unlink(missing_ok=True)


def main() -> None:
    args = parse_args()
    manifest = json.loads(args.manifest.read_text(encoding="utf-8"))
    args.output_dir.mkdir(parents=True, exist_ok=True)
    artifacts: list[dict[str, Any]] = []
    for name, kind, model_id, manifest_path, output_name in (*REQUESTS, generation_request(manifest)):
        item = descriptor(manifest, kind, model_id)
        expected = manifest_file(item, manifest_path)
        output_path = (args.output_dir / output_name).resolve()
        fetch(output_path, expected)
        verify(output_path, expected)
        artifacts.append(
            {
                "name": name,
                "kind": kind,
                "id": item["id"],
                "revision": item.get("revision"),
                "manifestPath": manifest_path,
                "path": str(output_path),
                "sha256": expected["sha256"],
                "bytes": expected["bytes"],
                "downloadUrl": expected.get("downloadUrl"),
            }
        )

    metadata = {
        "schemaVersion": 1,
        "manifest": str(args.manifest.resolve()),
        "artifacts": artifacts,
    }
    args.metadata.parent.mkdir(parents=True, exist_ok=True)
    args.metadata.write_text(json.dumps(metadata, indent=2) + "\n", encoding="utf-8")


if __name__ == "__main__":
    main()
