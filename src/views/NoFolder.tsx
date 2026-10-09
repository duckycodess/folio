import { FolderPlus } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";

/**
 * The desktop app before any folder is added. No Olio here: the floating
 * launcher is the view's one Olio now (#66).
 */
export function NoFolder({ workspace }: { workspace: WorkspaceState }) {
  return (
    <EmptyState
      title="Add a folder to get started"
      action={
        <div className="empty-state-actions">
          <Button
            variant="primary"
            icon={<FolderPlus size={18} />}
            disabled={workspace.busy}
            onClick={workspace.selectFolder}
          >
            Add folder
          </Button>
          <Button variant="ghost" onClick={workspace.showSamples}>
            Look at sample files
          </Button>
        </div>
      }
    >
      Folio reads only the folders you choose. Your files stay where they are.
    </EmptyState>
  );
}

/** A chosen folder with nothing Folio can read. No Olio: see `NoFolder`. */
export function EmptyFolder({ workspace }: { workspace: WorkspaceState }) {
  return (
    <EmptyState
      title="This folder has no files Folio can read"
      action={
        <Button
          icon={<FolderPlus size={18} />}
          disabled={workspace.busy}
          onClick={workspace.selectFolder}
        >
          Choose another folder
        </Button>
      }
    >
      Folio reads text, Markdown and text-based PDF files. Files in other
      formats are left alone.
    </EmptyState>
  );
}
