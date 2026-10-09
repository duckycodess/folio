"""Generate the cross-language golden fixtures for Folio's frozen contracts.

This is a third implementation of the encodings documented in
`docs/contracts.md`, written from that document rather than from either
production implementation. The TypeScript and Rust suites are both checked
against its output, so neither language can drift and still look correct.

Run it from anywhere; it rewrites `contract-cases.json` beside itself:

    python3 fixtures/contracts/generate-contract-cases.py

Regenerate only together with a deliberate, announced contract change, and
re-run `npm test` and `cargo test` afterwards.
"""
import hashlib
import json
import os
import unicodedata

OUT = os.path.dirname(os.path.abspath(__file__))

def sha256_hex(data: bytes) -> str:
    return hashlib.sha256(data).hexdigest()

def content_hash(text: str) -> str:
    return "sha256:" + sha256_hex(text.encode("utf-8"))

def field(value: str) -> bytes:
    raw = value.encode("utf-8")
    return str(len(raw)).encode("ascii") + b":" + raw + b"\n"

def canonical_plan(plan) -> bytes:
    out = b"FOLIO-PLAN-V1\n"
    out += field(plan["id"])
    out += field(plan["workspaceId"])
    out += field(str(plan["createdAt"]))
    out += field(str(plan["expiresAt"]))
    out += field(str(len(plan["operations"])))
    for op in plan["operations"]:
        out += field(op["kind"])
        if op["kind"] == "create":
            out += field(op["destinationRelativePath"])
            out += field(op["mediaType"])
            out += field(op["content"])
        elif op["kind"] == "edit":
            out += field(op["documentId"])
            out += field(op["relativePath"])
            out += field(op["expectedContentHash"])
            out += field(op["after"])
        elif op["kind"] in ("rename", "move"):
            out += field(op["documentId"])
            out += field(op["relativePath"])
            out += field(op["expectedContentHash"])
            out += field(op["destinationRelativePath"])
        elif op["kind"] == "delete":
            out += field(op["documentId"])
            out += field(op["relativePath"])
            out += field(op["expectedContentHash"])
        else:
            raise ValueError("unknown operation kind: " + op["kind"])
    return out

def space_fingerprint(space) -> str:
    def esc(v):
        return v.replace("%", "%25").replace("/", "%2F")
    return "/".join([
        "folio-space-v1",
        esc(space["modelId"]),
        esc(space["revision"]),
        esc(space["quantization"]),
        str(space["dimensions"]),
        esc(space["preprocessingFingerprint"]),
    ])

# --------------------------------------------------------------- identity ---
WS = sha256_hex(unicodedata.normalize("NFC", "/home/mag-aaral/Mga Dokumento").encode("utf-8"))
WS_WIN = sha256_hex(unicodedata.normalize("NFC", "\\\\?\\C:\\Users\\mag-aaral\\Documents").encode("utf-8"))

# "pagsasanay-ñ.md" written with a combining tilde (NFD), as macOS reports it.
NFD_PATH = "courses/pagsasanay-n\u0303.md"
NFC_PATH = unicodedata.normalize("NFC", NFD_PATH)

identity = {
    "workspaceId": [
        {"canonicalRootPath": "/home/mag-aaral/Mga Dokumento", "expected": WS},
        {"canonicalRootPath": "\\\\?\\C:\\Users\\mag-aaral\\Documents", "expected": WS_WIN},
    ],
    "normalizeRelativePath": {
        "accepted": [
            {"input": "projects/project-plan.md", "expected": "projects/project-plan.md"},
            {"input": NFD_PATH, "expected": NFC_PATH},
            {"input": "notes/tala sa proyekto.md", "expected": "notes/tala sa proyekto.md"},
            {"input": "a.md", "expected": "a.md"},
        ],
        "rejected": [
            {"input": "", "code": "pathNotRelative"},
            {"input": "/etc/passwd", "code": "pathNotRelative"},
            {"input": "C:/Users/mag-aaral/notes.md", "code": "pathNotRelative"},
            {"input": "notes\\paalala.md", "code": "pathNotRelative"},
            {"input": "notes/\u0000paalala.md", "code": "pathNotRelative"},
            {"input": "notes//paalala.md", "code": "pathNotRelative"},
            {"input": "../outside.md", "code": "pathEscapesWorkspace"},
            {"input": "notes/../../outside.md", "code": "pathEscapesWorkspace"},
            {"input": "notes/./paalala.md", "code": "pathEscapesWorkspace"},
        ],
    },
    "documentId": [
        {"workspaceId": WS, "relativePath": "projects/project-plan.md", "expected": WS + ":projects/project-plan.md"},
        {"workspaceId": WS, "relativePath": NFD_PATH, "expected": WS + ":" + NFC_PATH},
    ],
    "portableDestination": {
        "rejected": [
            {"input": "notes/plano?.md", "code": "operationUnsupported"},
            {"input": "notes/plano.md ", "code": "operationUnsupported"},
            {"input": "notes/plano:2026.md", "code": "operationUnsupported"},
        ]
    },
    "mediaType": [
        {"path": "notes/paalala.md", "expected": "text/markdown"},
        {"path": "notes/paalala.MD", "expected": "text/markdown"},
        {"path": "notes/paalala.txt", "expected": "text/plain"},
        {"path": "research/paper.pdf", "expected": "application/pdf"},
        {"path": "notes/paalala.docx", "expected": None},
        {"path": "notes/paalala", "expected": None},
    ],
    "embeddingSpaceFingerprint": [
        {
            "space": {"modelId": "intfloat/multilingual-e5-small", "revision": "r1", "quantization": "q8", "dimensions": 384, "preprocessingFingerprint": "query-passage-v1"},
            "expected": space_fingerprint({"modelId": "intfloat/multilingual-e5-small", "revision": "r1", "quantization": "q8", "dimensions": 384, "preprocessingFingerprint": "query-passage-v1"}),
        },
        {
            "space": {"modelId": "a%2Fb", "revision": "r1", "quantization": "q8", "dimensions": 384, "preprocessingFingerprint": "raw-text"},
            "expected": space_fingerprint({"modelId": "a%2Fb", "revision": "r1", "quantization": "q8", "dimensions": 384, "preprocessingFingerprint": "raw-text"}),
        },
    ],
}

# ---------------------------------------------------------------- offsets ---
offset_texts = [
    {
        "label": "filipino",
        "text": "Ang pagpupulong ay sa Biyernes, ika-20 ng Oktubre.",
    },
    {
        "label": "taglish-with-enye",
        "text": "Hanapin yung project plan ni Niña at palitan ang deadline.",
    },
    {
        "label": "combining-and-emoji",
        "text": "Deadline \U0001f4c5 na\u0303 October 20 \u2192 October 23",
    },
]
offsets = []
for case in offset_texts:
    text = case["text"]
    raw = text.encode("utf-8")
    # A passage starting at the first space and ending before the last character.
    start = len(text.split(" ")[0].encode("utf-8"))
    end = len(raw)
    offsets.append({
        "label": case["label"],
        "text": text,
        "utf8Length": len(raw),
        "utf16Length": len(text.encode("utf-16-le")) // 2,
        "passage": {"start": start, "end": end, "text": raw[start:end].decode("utf-8")},
        "contentHash": content_hash(text),
    })

# A deliberate mid-character offset: one byte into the emoji.
emoji_text = offset_texts[2]["text"]
emoji_byte = emoji_text.encode("utf-8").index(b"\xf0\x9f\x93\x85") + 1
offsets_invalid = [{"label": "mid-character", "text": emoji_text, "start": emoji_byte, "end": len(emoji_text.encode("utf-8"))}]

# ------------------------------------------------------------------ plans ---
plan_a_target = "Deadline: October 20\n"
plan_a = {
    "id": "plan-taglish-deadline",
    "workspaceId": WS,
    "createdAt": 1760000000000,
    "expiresAt": 1760000300000,
    "operations": [
        {
            "kind": "edit",
            "documentId": WS + ":projects/project-plan.md",
            "relativePath": "projects/project-plan.md",
            "expectedContentHash": content_hash(plan_a_target),
            "after": "Deadline: October 23\n",
        }
    ],
}
plan_b_note = "Paalala: ang huling araw ay ika-23 ng Oktubre \U0001f4c5\n"
plan_b = {
    "id": "plan-batch-filipino",
    "workspaceId": WS,
    "createdAt": 1760000000000,
    "expiresAt": 1760000300000,
    "operations": [
        {
            "kind": "edit",
            "documentId": WS + ":projects/project-plan.md",
            "relativePath": "projects/project-plan.md",
            "expectedContentHash": content_hash(plan_a_target),
            "after": "Deadline: October 23\n",
        },
        {
            "kind": "rename",
            "documentId": WS + ":notes/paalala.md",
            "relativePath": "notes/paalala.md",
            "expectedContentHash": content_hash("Paalala\n"),
            "destinationRelativePath": "notes/paalala-oktubre.md",
            "expectedDestination": "absent",
        },
        {
            "kind": "create",
            "destinationRelativePath": NFC_PATH,
            "mediaType": "text/markdown",
            "content": plan_b_note,
            "expectedDestination": "absent",
        },
    ],
}
# A deletion has no destination: its kind, identity, path and expected hash.
plan_c = {
    "id": "plan-delete-pagsasanay",
    "workspaceId": WS,
    "createdAt": 1760000000000,
    "expiresAt": 1760000300000,
    "operations": [
        {
            "kind": "delete",
            "documentId": WS + ":" + NFC_PATH,
            "relativePath": NFC_PATH,
            "expectedContentHash": content_hash(plan_b_note),
        }
    ],
}
plans = []
for plan in (plan_a, plan_b, plan_c):
    canonical = canonical_plan(plan)
    digest = "sha256:" + sha256_hex(canonical)
    plan_with_digest = dict(plan)
    plan_with_digest["impacts"] = []
    plan_with_digest["digest"] = digest
    plans.append({
        "plan": plan_with_digest,
        "canonical": canonical.decode("utf-8"),
        "canonicalByteLength": len(canonical),
        "digest": digest,
    })

# ----------------------------------------------------------------- hashes ---
hashes = [
    {"text": "", "expected": content_hash("")},
    {"text": "Ang pagpupulong ay sa Biyernes.", "expected": content_hash("Ang pagpupulong ay sa Biyernes.")},
    {"text": plan_b_note, "expected": content_hash(plan_b_note)},
]

# ------------------------------------------------------------ error codes ---
error_codes = [
    "workspaceNotAuthorized", "workspaceUnavailable", "pathNotRelative",
    "pathEscapesWorkspace", "pathUnsupportedEncoding", "documentUnavailable",
    "documentTooLarge", "documentNotText", "unsupportedMediaType",
    "planUnknown", "planEmpty", "planExpired", "planStateInvalid",
    "planDigestMismatch", "approvalRequired", "approvalStale",
    "duplicateOperationTarget", "targetMissing", "targetChanged",
    "destinationExists", "operationUnsupported", "historyRequired",
    "historyUnknown", "undoConflict", "writerNotImplemented",
    "modelNotInstalled", "modelLoadFailed", "providerBusy", "cancelled",
    "contextOverflow", "embeddingSpaceMismatch", "evidenceInvalid", "internal",
]

bundle = {
    "identity": identity,
    "offsets": {"cases": offsets, "invalid": offsets_invalid},
    "plans": plans,
    "hashes": hashes,
    "errorCodes": error_codes,
    "operationStatuses": ["succeeded", "failed", "cancelled", "notStarted"],
    "batchStopReasons": ["completed", "failed", "cancelled"],
    "offsetUnit": "utf8Byte",
}

with open(os.path.join(OUT, "contract-cases.json"), "w", encoding="utf-8") as handle:
    json.dump(bundle, handle, ensure_ascii=False, indent=2, sort_keys=False)
    handle.write("\n")
print(os.path.join(OUT, "contract-cases.json"))
