# Error states

Every error the app can receive has one plain-language message and, where there is one, a next step. The wording lives in `src/app/recovery.ts`; this page lists it so each state can be checked (#9).

## Practice mode

Open the browser preview (`npm run dev`, which serves on port 1420) with `?simulate=<code>`, for example `http://localhost:1420/?simulate=destinationExists`, and follow the steps for that code. A banner says the problem is simulated. Practice mode never runs in the desktop app.

Without practice mode, Ask & Act always shows `modelNotInstalled`, in the browser and the desktop app, because this version has no local model.

| Code                       | Tone    | Message title                                          | Next step            | How to trigger                                              |
| -------------------------- | ------- | ------------------------------------------------------ | -------------------- | ----------------------------------------------------------- |
| `workspaceNotAuthorized`   | danger  | Folio no longer has access to this folder              | Add the folder again | Home → **Add folder**                                       |
| `workspaceUnavailable`     | danger  | This folder can't be reached                           | Add the folder again | Home → **Add folder**                                       |
| `pathNotRelative`          | danger  | Folio couldn't find that file in your folder           | —                    | Open any file in the list                                   |
| `pathEscapesWorkspace`     | danger  | That file is outside the folders you added             | —                    | Open any file in the list                                   |
| `pathUnsupportedEncoding`  | warning | This file name can't be read                           | —                    | Open any file in the list                                   |
| `documentUnavailable`      | danger  | This file couldn't be opened                           | Try again            | Open any file in the list                                   |
| `documentTooLarge`         | warning | This file is too large to open here                    | —                    | Open any file in the list                                   |
| `documentNotText`          | warning | This file doesn't contain readable text                | —                    | Open any file in the list                                   |
| `unsupportedMediaType`     | warning | Folio can't read this type of file                     | —                    | Open any file in the list                                   |
| `planUnknown`              | warning | This preview is no longer available                    | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `planEmpty`                | info    | There's nothing to change                              | —                    | Organize → choose a file → type a name → **Preview rename** |
| `planExpired`              | warning | This preview has expired                               | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `planStateInvalid`         | warning | This preview was already used or cancelled             | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `planDigestMismatch`       | danger  | The approved change doesn't match the preview          | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `approvalRequired`         | info    | This change needs your approval                        | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `approvalStale`            | warning | Your approval is out of date                           | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `duplicateOperationTarget` | warning | Two changes affect the same file                       | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `targetMissing`            | warning | A file in this change is missing                       | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `targetChanged`            | warning | A file changed since the preview                       | Preview again        | Organize → choose a file → type a name → **Preview rename** |
| `destinationExists`        | warning | A file with that name already exists                   | Choose another name  | Organize → choose a file → type a name → **Preview rename** |
| `operationUnsupported`     | warning | Folio can't make this kind of change                   | —                    | Organize → choose a file → type a name → **Preview rename** |
| `historyRequired`          | danger  | This file was changed, but Undo isn't available for it | —                    | Organize → choose a file → type a name → **Preview rename** |
| `historyUnknown`           | warning | Folio couldn't find this change in its history         | —                    | Organize → choose a file → type a name → **Preview rename** |
| `undoConflict`             | warning | These files changed after Folio's change               | —                    | Organize → choose a file → type a name → **Preview rename** |
| `writerNotImplemented`     | info    | Saving changes isn't available yet                     | —                    | Organize → choose a file → type a name → **Preview rename** |
| `modelNotInstalled`        | warning | This needs a local AI model                            | Open Model Lab       | Ask & Act → type a request → **Preview actions**            |
| `modelLoadFailed`          | danger  | The local AI model couldn't start                      | Open Model Lab       | Ask & Act → type a request → **Preview actions**            |
| `providerBusy`             | info    | Folio is still working on another request              | Try again            | Ask & Act → type a request → **Preview actions**            |
| `cancelled`                | info    | Stopped                                                | Try again            | Ask & Act → type a request → **Preview actions**            |
| `contextOverflow`          | warning | That's too much text to work with at once              | —                    | Ask & Act → type a request → **Preview actions**            |
| `embeddingSpaceMismatch`   | warning | Search needs to be refreshed                           | Open Model Lab       | Type in the search field                                    |
| `evidenceInvalid`          | warning | The passage this relied on has changed                 | Try again            | Type in the search field                                    |
| `internal`                 | danger  | Something went wrong                                   | Try again            | Open any file in the list                                   |

Danger notices use `role="alert"`. Other notices are announced through the polite live region of the view that caused them, which is replaced when you switch views. Change-related messages depend on when the error arrived (`recoveryFor(error, stage)` and `<RecoveryNotice stage>`): refused before any write (the default) ends with "No file was changed."; partway through an approved batch, "This file wasn't changed. Earlier changes in this batch were kept and can be undone."; partway through an Undo, "Folio undid part of this change, then stopped. Preview Undo again to finish." `historyRequired` only ever follows a write and says so. Practice mode shows the refused wording; requests to the local model say "Your request is kept." Typed drafts (the Ask & Act request and rename names, per file) are kept across errors and view changes.
