import { FolderOpen, FolderPlus } from "lucide-react";
import type { WorkspaceState } from "../app/useWorkspace";
import { Button } from "../ui/Button";

/** Says whose files are on screen, and how to switch to the user's own. */
export function WorkspaceSource({ workspace }: { workspace: WorkspaceState }) {
  const sample = !workspace.workspace;
  return (
    <div className="workspace-source">
      <div className="workspace-source-text">
        <span className="workspace-source-title">
          {sample ? "Showing sample files" : "Your folder"}
        </span>
        <span
          className="workspace-source-detail"
          title={workspace.workspace?.rootPath}
        >
          {sample
            ? workspace.nativeAvailable
              ? "Choose a folder to see your own documents."
              : "Folder access works in the desktop app. This preview uses sample files only."
            : workspace.workspace?.rootPath}
        </span>
      </div>
      <Button
        variant={sample ? "primary" : "secondary"}
        icon={sample ? <FolderPlus size={18} /> : <FolderOpen size={18} />}
        disabled={!workspace.nativeAvailable || workspace.busy}
        onClick={workspace.selectFolder}
      >
        {sample ? "Add folder" : "Change folder"}
      </Button>
    </div>
  );
}
