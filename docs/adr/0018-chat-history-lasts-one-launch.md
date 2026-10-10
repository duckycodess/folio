# Chat history lasts for one launch of the app

ADR 0013 kept Olio chat history on the device in `localStorage`, so it survived quitting Folio. The user decided that every launch or relaunch should start with no chat history. History now lives in the window's `sessionStorage`: it survives a reload of the window, but not quitting and reopening Folio. On start, Folio deletes the history that earlier versions left in `localStorage`, because it holds requests, file paths and quoted passages in plain text.

Within one launch nothing else changes from ADR 0013: one store serves the compact chat and the full Ask & Act page, conversations stay per folder with the same caps, History lists the launch's other conversations, and "Delete all conversations" clears them. The trade-off is that a past answer can't be reopened after a restart; asking again re-reads the files, which may have changed since.

This supersedes only the persistence part of ADR 0013.
