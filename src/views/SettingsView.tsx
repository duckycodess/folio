import { useId } from "react";
import type { LocalAiStatus } from "../app/localAi";
import { THEME_LABELS, type ThemePreference } from "../app/theme";
import type { WorkspaceState } from "../app/useWorkspace";
import { Button } from "../ui/Button";

const THEMES: ThemePreference[] = ["system", "light", "dark"];

/**
 * Settings & style, as in the brandkit mockup: appearance, local AI, the
 * workspace folder and the setup guide, each on its own row.
 */
export function SettingsView({
  workspace,
  theme,
  onTheme,
  aiStatus,
  aiLabel,
  onOpenModelLab,
  onOpenSetup,
}: {
  workspace: WorkspaceState;
  theme: ThemePreference;
  onTheme: (theme: ThemePreference) => void;
  aiStatus: LocalAiStatus;
  aiLabel: string;
  onOpenModelLab: () => void;
  /** Only in the desktop app, where first-run setup exists. */
  onOpenSetup?: () => void;
}) {
  const id = useId();
  const sample = !workspace.workspace;
  return (
    <div className="view">
      <header className="page-header">
        <h1 className="page-title">The Folio feel.</h1>
        <p className="page-tagline">
          A warm, focused workspace with a little personality.
        </p>
      </header>

      <div className="setting">
        <div className="setting-text">
          <h2 className="setting-title" id={`${id}-theme`}>
            Appearance
          </h2>
          <p className="setting-detail">
            The warm light theme, the charcoal theme, or follow this computer.
          </p>
        </div>
        <select
          className="select"
          aria-labelledby={`${id}-theme`}
          value={theme}
          onChange={(event) => onTheme(event.target.value as ThemePreference)}
        >
          {THEMES.map((value) => (
            <option key={value} value={value}>
              {THEME_LABELS[value]}
            </option>
          ))}
        </select>
      </div>

      <div className="setting">
        <div className="setting-text">
          <h2 className="setting-title">Local AI</h2>
          <p className="setting-detail">
            <span
              className="status-dot"
              data-status={aiStatus}
              aria-hidden="true"
            />
            {aiLabel}. Models run on this computer; choose and download them in
            Model Lab.
          </p>
        </div>
        <Button variant="secondary" onClick={onOpenModelLab}>
          Open Model Lab
        </Button>
      </div>

      <div className="setting">
        <div className="setting-text">
          <h2 className="setting-title">Workspace folder</h2>
          <p
            className="setting-detail setting-path"
            title={workspace.workspace?.rootPath}
          >
            {sample
              ? workspace.nativeAvailable
                ? "Showing sample files. Choose a folder to see your own documents."
                : "Folder access works in the desktop app. This preview uses sample files only."
              : workspace.workspace?.rootPath}
          </p>
        </div>
        <Button
          variant={sample ? "primary" : "secondary"}
          disabled={!workspace.canChooseFolder || workspace.busy}
          onClick={workspace.selectFolder}
        >
          {sample ? "Add folder" : "Change folder"}
        </Button>
      </div>

      {onOpenSetup && (
        <div className="setting">
          <div className="setting-text">
            <h2 className="setting-title">Setup guide</h2>
            <p className="setting-detail">
              Go through first-run setup again: folder, local AI and privacy.
            </p>
          </div>
          <Button variant="secondary" onClick={onOpenSetup}>
            Open setup guide
          </Button>
        </div>
      )}
    </div>
  );
}
