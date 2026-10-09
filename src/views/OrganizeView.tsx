import { useEffect, useRef } from "react";
import type { CollectionsController } from "../app/useCollections";
import type { OrganizeController } from "../app/useOrganize";
import type { WorkspaceState } from "../app/useWorkspace";
import { CollectionsPanel } from "./CollectionsPanel";
import { OrganizeFlowPanel } from "./OrganizeFlowPanel";

/**
 * Journey B: analyze a folder or a collection and act on suggestions.
 * Renaming or moving one file lives on Home's file rows instead (#42).
 */
export function OrganizeView({
  workspace,
  organize,
  collections,
}: {
  workspace: WorkspaceState;
  organize: OrganizeController;
  collections: CollectionsController;
}) {
  const top = useRef<HTMLDivElement>(null);
  const { target, setTarget } = organize;
  const removed =
    target !== null &&
    !collections.collections.some((collection) => collection.id === target);
  // A collection removed while it was the target: analyze the folder instead.
  useEffect(() => {
    if (removed) setTarget(null);
  }, [removed, setTarget]);

  return (
    <div className="view" ref={top}>
      <header className="page-header page-header-compact">
        <h1 className="page-title">Organize</h1>
        <p className="page-tagline">
          Find duplicates, clearer names and collections. Nothing changes on
          disk without your approval.
        </p>
      </header>

      <OrganizeFlowPanel
        workspace={workspace}
        organize={organize}
        collections={collections}
      />

      <CollectionsPanel
        workspace={workspace}
        collections={collections}
        onAnalyze={(collectionId) => {
          organize.setTarget(collectionId);
          top.current?.scrollIntoView({ block: "start" });
          top.current
            ?.querySelector<HTMLSelectElement>("#organize-target")
            ?.focus();
        }}
      />
    </div>
  );
}
