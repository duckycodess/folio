# Back off retrying unreadable documents

Since #11, Local Sync re-reads every `failed` and `stale` document on every scan, so a released lock or a finished cloud download recovers on its own. A folder with many permanently damaged PDFs then pays the full extraction cost on every scan. Folio now retries such a document when there is a reason to expect a different result, and otherwise backs off without ever giving up.

A document is read again when Folio wrote it, the user asks to check it again, its change signature differs from the one it last failed with, the extractor version changed, or its wait is over. The change signature is size and modification time, plus change time and mode on Unix and file attributes on Windows. The first two scans after a failure still retry. From the third consecutive identical failure, Folio waits 10 minutes, then 20, 40 and so on, up to 6 hours. A wait longer than 6 hours from now means the clock went back, and it is treated as over. A successful read clears the count.

The backoff applies to every failure, including files that could not be opened. Stable Rust does not expose the Windows change time, so on Windows a permission (ACL) change or a released lock is noticed only when the wait ends or when the user asks Folio to check again. We accepted that rather than add unsafe platform code. Users can ask from a document ("Check again", `recheck_documents`) or for a whole scan (`recheckUnreadable`). A deferred document still counts as `failed` or `stale` in the scan summary, and also in a separate `deferred` count.

Decided with Gab on 2026-10-09 for issue #28.
