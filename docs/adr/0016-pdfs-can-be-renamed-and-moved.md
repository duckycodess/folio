# PDFs can be renamed and moved, but their contents stay read-only

People asked Ask & Act to rename their PDFs ("rename the VILAR_Resume.pdf to Larvi.pdf", "rename 201_Barangay Clearance to Police Clearance"), and Folio answered "Which file should I use?" because the interpreter saw only TXT and Markdown files. Renaming a document is one of the most common things a person wants to do with it, and a rename or move doesn't touch the file's bytes.

Folio now renames and moves PDFs through the same exact preview, approval, history and Undo as text files. The plan builder allows a rename or move of a TXT, Markdown or PDF file when the destination keeps its type; the writer and Undo already relocate files without reading them. Editing a PDF's text and deleting a PDF stay unavailable: a request to edit one is answered plainly instead of being guessed at.

A rename proposal must carry the file's current revision. The interpreter hashes only the PDFs whose names share a word with the request, so a large folder is not read on every request; a PDF it didn't hash gets a clarification instead of a proposal.

A rename names only the file, so it stays in its own folder, and a name given without an extension keeps the file's own. A move to a bare folder name puts the file inside that folder.

Decided with Louise on 2026-10-10.
