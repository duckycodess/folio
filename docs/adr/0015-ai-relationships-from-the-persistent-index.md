# AI relationships from the persistent index

Issue #46 derives AI relationships from the persistent index in #27's
stored-chunk embedding space for the selected installed search model, resolved
natively from its descriptor. Discovery is bounded, progressive and resumable;
coverage is recorded separately from retained edges, which are displayed as a
bounded read-time union top-K and flagged when storage overflow truncates
candidates. Shared-fact candidates require conservative clause-level subject
corroboration, so a link alone never qualifies. Generated summaries and
explanations are cited, labelled and incomplete when their supplied evidence
is incomplete; the existing webview-writable vector-store commands remain
intact, so `embedding` provenance means vectors stored in the active space,
not native-provider authorship.
