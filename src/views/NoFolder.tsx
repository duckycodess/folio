import { FolderPlus } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Button } from "../ui/Button";
import { EmptyState } from "../ui/EmptyState";
import { Olio } from "../ui/Olio";

/** The desktop app before any folder is added. */
export function NoFolder({ workspace }: { workspace: WorkspaceState }) {
  return (
    <EmptyState
      illustration={<Olio pose="waving" size={160} />}
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

/** A chosen folder with nothing Folio can read. */
export function EmptyFolder({
  workspace,
  showOlio,
}: {
  workspace: WorkspaceState;
  /** Off where the view already shows Olio elsewhere (one per view). */
  showOlio: boolean;
}) {
  return (
    <EmptyState
      illustration={showOlio && <Olio pose="peeking" size={160} />}
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
