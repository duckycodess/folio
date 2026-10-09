# Home is the file browser

Home and Files showed the same folder note, the same file list and the same search, so the separate Files page is removed. Home is now the file browser, like the home screen of a phone's Files app. It lists every file, and each file's actions are on its row: Open, Rename, Move to folder and Show related. The search field moves from the top bar of every page into Home, centred under the header. ⌘K or Ctrl K still works from anywhere and opens Home. Journey A becomes "Home → Browse or Search Files → Select File → …", with the same steps.

Acting on a file from its row doesn't relax ADR 0001. Rename and Move show the exact native plan and change nothing until the user approves it. They use the same flow as Organize, including Undo, so there is no one-click silent rename. Summarize is not a row action. It lives in each file's Summary tab, and Ask & Act can also start one (#20), so Summarize stays reachable without the assistant.

We accepted that search is no longer one keystroke away inside other pages: the query only filters Home's list, and Organize and Graph show all files. Organize keeps journey B (Analyze and suggestions). Its single-file Rename panel moved to Home's rows.

Decided with Louise on 2026-10-10 for issues #42 and #43.
