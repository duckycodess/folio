# Navigation with Activity and a Home file browser

The updated product context (2026-10-09) gives every main page one job: Home finds, Graph explains, Ask & Act assists, and Activity keeps everything accountable. Folio's main navigation is now **Home, Organize, Graph, Ask & Act and Activity**. **Model Lab** moves to the sidebar's settings area, next to the theme switch and the local AI status.

**Files is removed (#42).** It repeated Home: the same folder note, file list and search. Home is now the file browser, and each file's actions are on its row: Open, Rename, Move to folder…, and Show related. Rename and Move still go through an exact preview and approval (ADR 0001). Search lives only on Home (#43), and ⌘K or Ctrl K opens Home from any page. Home's Folder, File type and Modified filters, pinned folders and recent files (#33) are plain, deterministic search aids that work without a model.

**Organize stays a separate page.** The updated context folds organization into Ask & Act. The user decided to keep Organize, so Smart Organize remains an entry point that works without chat, and the app doesn't collapse into the assistant. Organizing from Ask & Act is an additional route that ends in the same preview and approval.

**Activity is new (#34).** It lists what Folio actually changed, from the native history only, one entry per approved plan. Undo is offered only when the preview says it's safe. Previews, suggestions and summaries never appear as changes. Recording failed or cancelled attempts and the action's source needs native history work (#35).

**Summarize** lives in each file's Summary tab, and Ask & Act can ask for the same summary (#20). There's no separate Summarize page or row action.

Decided by the user on 2026-10-09 and 2026-10-10, for issues #33, #34, #39, #42 and #43.
