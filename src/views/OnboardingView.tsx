import { FolderPlus, Link2, Copy } from "lucide-react";
import { useEffect, useRef, useState, type RefObject } from "react";
import {
  exactSize,
  memorySize,
  modelName,
  ROLE_LABELS,
  setupAdvice,
} from "../app/models";
import { useModels, type ModelsController } from "../app/useModels";
import type { RelationshipsState } from "../app/useRelationships";
import type { WorkspaceState } from "../app/useWorkspace";
import {
  firstFindings,
  nextStep,
  ONBOARDING_STEPS,
  previousStep,
  type OnboardingStep,
} from "../domain/onboarding";
import type { ViewId } from "../shell/navigation";
import { Button } from "../ui/Button";
import { Notice } from "../ui/Notice";
import { Olio } from "../ui/Olio";
import { Progress } from "../ui/Progress";
import { RecoveryNotice } from "../ui/RecoveryNotice";
import { ModelCard } from "./ModelCard";

const PHASES: Record<string, string> = {
  discovering: "Finding files",
  indexing: "Reading text from your files",
  linking: "Finding links between files",
  done: "Finishing",
  cancelled: "Stopping",
};

interface OnboardingProps {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  /** Ends onboarding (now or later it won't show again) and opens a view. */
  onFinish: (view?: ViewId) => void;
}

/**
 * First-run setup (#14), in five skippable steps: welcome, a folder, local AI,
 * indexing, and what Folio found. Nothing is read before the user picks a
 * folder in the system picker, and a model downloads only from its own
 * button, after its exact size and revision are shown.
 */
export function OnboardingView({
  workspace,
  relations,
  onFinish,
}: OnboardingProps) {
  const [step, setStep] = useState<OnboardingStep>("welcome");
  const heading = useRef<HTMLHeadingElement>(null);
  const index = ONBOARDING_STEPS.indexOf(step);
  // Held here so a download in progress keeps the steps from changing under
  // it; Cancel download stays available.
  const models = useModels();
  const downloading = models.installing !== null;

  // Each step's heading takes focus, so keyboard and screen-reader users
  // start at the top of it.
  useEffect(() => {
    heading.current?.focus();
  }, [step]);

  const go = (target: OnboardingStep) => setStep(target);
  const next = () => go(nextStep(step));

  return (
    <div className="onboarding">
      <main className="onboarding-card" aria-labelledby="onboarding-title">
        <p className="onboarding-step">
          Step {index + 1} of {ONBOARDING_STEPS.length}
        </p>
        {step === "welcome" && (
          <Welcome headingRef={heading} onNext={next} onFinish={onFinish} />
        )}
        {step === "folder" && (
          <ChooseFolder workspace={workspace} headingRef={heading} />
        )}
        {step === "ai" && <LocalAi headingRef={heading} models={models} />}
        {step === "index" && (
          <IndexFolder
            workspace={workspace}
            headingRef={heading}
            onContinueAtHome={() => onFinish("home")}
          />
        )}
        {step === "found" && (
          <Found
            workspace={workspace}
            relations={relations}
            headingRef={heading}
            onFinish={onFinish}
          />
        )}
        {step !== "welcome" && (
          <div className="onboarding-actions">
            <Button
              variant="ghost"
              disabled={downloading}
              onClick={() => go(previousStep(step))}
            >
              Back
            </Button>
            <span className="onboarding-spacer" />
            {step !== "found" && (
              <>
                <Button variant="ghost" disabled={downloading} onClick={next}>
                  Skip for now
                </Button>
                <Button
                  variant="primary"
                  onClick={next}
                  disabled={
                    downloading ||
                    (step === "folder" && workspace.source !== "folder") ||
                    (step === "index" && workspace.search.index !== "ready")
                  }
                >
                  Continue
                </Button>
              </>
            )}
            {step === "found" && (
              <Button variant="primary" onClick={() => onFinish("home")}>
                Go to Home
              </Button>
            )}
          </div>
        )}
      </main>
    </div>
  );
}

type HeadingRef = RefObject<HTMLHeadingElement | null>;

function Title({
  headingRef,
  children,
}: {
  headingRef: HeadingRef;
  children: string;
}) {
  return (
    <h1
      id="onboarding-title"
      ref={headingRef}
      tabIndex={-1}
      className="onboarding-title"
    >
      {children}
    </h1>
  );
}

function Welcome({
  headingRef,
  onNext,
  onFinish,
}: {
  headingRef: HeadingRef;
  onNext: () => void;
  onFinish: (view?: ViewId) => void;
}) {
  return (
    <>
      <Olio pose="waving" size={160} />
      <Title headingRef={headingRef}>Welcome to Folio</Title>
      <p className="onboarding-tagline">Search. Organize. Summarize.</p>
      <ul className="onboarding-points">
        <li>No account required.</li>
        <li>AI features run on this computer, not in the cloud.</li>
        <li>
          Your files stay where they are. Folio changes them only after you
          approve.
        </li>
      </ul>
      <div className="onboarding-actions">
        <Button variant="ghost" onClick={() => onFinish()}>
          Skip setup
        </Button>
        <span className="onboarding-spacer" />
        <Button variant="primary" onClick={onNext}>
          Get started
        </Button>
      </div>
    </>
  );
}

function ChooseFolder({
  workspace,
  headingRef,
}: {
  workspace: WorkspaceState;
  headingRef: HeadingRef;
}) {
  const chosen = workspace.workspace;
  return (
    <>
      <Title headingRef={headingRef}>Choose a folder</Title>
      <p>
        Start with one folder, such as Documents, Desktop, or a school or work
        folder. Folio reads only the folders you choose in the next window, and
        you can change it any time.
      </p>
      {workspace.failure && (
        <RecoveryNotice
          error={workspace.failure.error}
          actions={{
            retry: workspace.failure.retry,
            chooseFolder: () => void workspace.selectFolder(),
          }}
        />
      )}
      {chosen ? (
        <div className="onboarding-result">
          <p>
            <strong>{chosen.rootPath}</strong>
            <br />
            {workspace.documents.length === 0
              ? "This folder has no files Folio can read (text, Markdown or text-based PDF)."
              : workspace.documents.length === 1
                ? "1 file Folio can read."
                : `${workspace.documents.length} files Folio can read.`}
          </p>
          <Button
            icon={<FolderPlus size={18} />}
            disabled={workspace.busy}
            onClick={() => void workspace.selectFolder()}
          >
            Choose a different folder
          </Button>
        </div>
      ) : (
        <Button
          variant="primary"
          icon={<FolderPlus size={18} />}
          disabled={workspace.busy}
          onClick={() => void workspace.selectFolder()}
        >
          Choose a folder
        </Button>
      )}
    </>
  );
}

function LocalAi({
  headingRef,
  models,
}: {
  headingRef: HeadingRef;
  models: ModelsController;
}) {
  const advice = setupAdvice(models.groups, models.setup, models.runtime);
  const recommended = new Set(advice.rows.map((row) => row.descriptor.id));
  const others = models.groups.flatMap((group) =>
    group.rows.filter((row) => !recommended.has(row.descriptor.id)),
  );
  const memory = models.setup?.deviceMemoryBytes ?? null;
  const free = models.setup?.availableDiskBytes ?? null;
  const size = (bytes: number) => exactSize(bytes).split(" (")[0];

  return (
    <>
      <Title headingRef={headingRef}>Local AI (optional)</Title>
      <p>
        Summaries, finding files by meaning, and Ask &amp; Act use local AI
        models that run on this computer. Nothing downloads until you press a
        model's Download button.
      </p>
      <p className="muted">
        Without a model, browsing your files and keyword search work as usual,
        and AI features explain what they need. You can set models up later in
        Model Lab.
      </p>
      {models.error && (
        <RecoveryNotice
          error={models.error}
          actions={{ retry: models.reload }}
          onDismiss={models.dismiss}
        />
      )}
      {models.notice && <Notice tone="info">{models.notice}</Notice>}
      {models.load === "loading" ? (
        <Progress label="Checking this computer and the installed models" />
      ) : models.load === "ready" ? (
        <>
          <dl className="model-facts onboarding-device">
            <dt>This computer's RAM</dt>
            <dd>
              {memory === null
                ? "Unknown"
                : `${memorySize(memory)} (the whole computer)`}
            </dd>
            <dt>Free space</dt>
            <dd>
              {free === null
                ? "Unknown"
                : `${size(free)} on the disk Folio keeps models on`}
            </dd>
          </dl>
          {advice.belowRamTarget && memory !== null && (
            <Notice tone="warning">
              Folio is designed for computers with at least 8 GB of RAM, and
              this one has {memorySize(memory)}. The models may run slowly or
              not at all. Speeds on this computer haven't been measured.
            </Notice>
          )}
          {advice.rows.length > 0 && (
            <>
              <h2 className="section-title">Recommended</h2>
              {advice.pending.length === 0 ? (
                <p>
                  A model is set up for each job. You can change them in Model
                  Lab.
                </p>
              ) : (
                <p>
                  {advice.pending
                    .map(
                      (row) =>
                        `${ROLE_LABELS[row.descriptor.role].title}: ${modelName(row.descriptor)}`,
                    )
                    .join(". ")}
                  . {advice.pending.length > 1 ? "Together they" : "It"}{" "}
                  {advice.pending.length > 1 ? "download" : "downloads"}{" "}
                  {size(advice.downloadBytes)}
                  {advice.runtimeUnknown
                    ? ", plus a runtime whose size isn't listed"
                    : ""}
                  {advice.withinBudget ? ", within Folio's 1 GB target" : ""}.
                  {advice.pending.length > 1 &&
                    " Each downloads separately, so you can set up just one."}
                </p>
              )}
              {advice.shortOfSpace && free !== null && (
                <Notice tone="warning">
                  The disk Folio uses has {size(free)} free, less than the{" "}
                  {size(advice.downloadBytes)} these need. Free up space, or
                  skip for now.
                </Notice>
              )}
              <ul className="model-list">
                {advice.rows.map((row) => (
                  <ModelCard
                    key={row.descriptor.id}
                    row={row}
                    models={models}
                  />
                ))}
              </ul>
            </>
          )}
          {others.length > 0 && (
            <details className="onboarding-more">
              <summary>Other models, including larger optional ones</summary>
              <ul className="model-list">
                {others.map((row) => (
                  <ModelCard
                    key={row.descriptor.id}
                    row={row}
                    models={models}
                  />
                ))}
              </ul>
            </details>
          )}
          <p className="muted">
            Sizes are the exact downloads from Folio's pinned list. The space a
            model takes once installed isn't measured.
          </p>
        </>
      ) : models.load === "desktopOnly" ? (
        <p className="muted">Model setup works in the desktop app.</p>
      ) : null}
    </>
  );
}

function IndexFolder({
  workspace,
  headingRef,
  onContinueAtHome,
}: {
  workspace: WorkspaceState;
  headingRef: HeadingRef;
  onContinueAtHome: () => void;
}) {
  const search = workspace.search;
  const progress = search.progress;
  return (
    <>
      <Title headingRef={headingRef}>Index your folder</Title>
      <p>
        Indexing reads the text of your files so search can look inside them and
        Folio can find links and duplicates. It doesn't change any file.
      </p>
      {workspace.source !== "folder" ? (
        <p className="muted">Add a folder first, or skip this step.</p>
      ) : search.index === "ready" ? (
        <p className="onboarding-result">This folder is indexed.</p>
      ) : search.index === "indexing" ? (
        <div className="onboarding-result">
          <Progress
            label={progress ? PHASES[progress.phase] : "Indexing this folder"}
            value={
              progress && progress.total > 0
                ? (100 * progress.processed) / progress.total
                : undefined
            }
          />
          <div className="onboarding-inline-actions">
            <Button variant="ghost" onClick={search.cancelIndex}>
              Stop
            </Button>
            <Button onClick={onContinueAtHome}>
              Continue to Home while indexing
            </Button>
          </div>
        </div>
      ) : search.index === "failed" && search.indexFailure ? (
        <RecoveryNotice
          error={search.indexFailure}
          actions={{ retry: search.buildIndex }}
        />
      ) : search.index === "checking" ? (
        <p className="muted">Checking this folder…</p>
      ) : (
        <Button variant="primary" onClick={search.buildIndex}>
          Index this folder
        </Button>
      )}
    </>
  );
}

function Found({
  workspace,
  relations,
  headingRef,
  onFinish,
}: {
  workspace: WorkspaceState;
  relations: RelationshipsState;
  headingRef: HeadingRef;
  onFinish: (view?: ViewId) => void;
}) {
  const { request, refresh } = relations;
  const indexed = workspace.search.index === "ready";
  // Read the index's links and duplicates now that it exists.
  useEffect(() => {
    request();
    if (indexed) refresh();
  }, [request, refresh, indexed]);
  const findings =
    relations.coverage === "indexed"
      ? firstFindings(
          relations.relationships,
          relations.duplicates,
          workspace.documents,
        )
      : [];
  return (
    <>
      <Title headingRef={headingRef}>What Folio found</Title>
      {relations.coverage === "loading" ? (
        <p className="muted">Looking at your indexed files…</p>
      ) : relations.coverage === "failed" ? (
        // A failed read is not a result: say so, and still offer the way on.
        <>
          {relations.failure && (
            <RecoveryNotice
              error={relations.failure}
              actions={{ retry: relations.refresh }}
            />
          )}
          <div className="onboarding-inline-actions">
            <Button onClick={() => onFinish("home")}>Search your files</Button>
            <Button onClick={() => onFinish("organize")}>Organize</Button>
            <Button onClick={() => onFinish("assistant")}>Ask &amp; Act</Button>
          </div>
        </>
      ) : findings.length ? (
        <>
          <p>From the files in your folder:</p>
          <ul className="onboarding-findings">
            {findings.map((finding) => (
              <li
                key={`${finding.kind}-${finding.files.map((file) => file.id).join("|")}`}
              >
                <p className="finding-title">
                  {finding.kind === "duplicate" ? (
                    <Copy size={16} aria-hidden="true" />
                  ) : (
                    <Link2 size={16} aria-hidden="true" />
                  )}
                  {finding.title}
                </p>
                <ul className="finding-files">
                  {finding.files.map((file) => (
                    <li key={file.id}>{file.relativePath}</li>
                  ))}
                </ul>
                {finding.evidence && (
                  <p className="finding-evidence">“{finding.evidence}”</p>
                )}
              </li>
            ))}
          </ul>
        </>
      ) : (
        <>
          <p>
            {indexed
              ? "Folio didn't find links between files or identical files in this folder yet."
              : "Index a folder to see links between files and identical files here."}
          </p>
          <div className="onboarding-inline-actions">
            <Button onClick={() => onFinish("home")}>Search your files</Button>
            <Button onClick={() => onFinish("organize")}>Organize</Button>
            <Button onClick={() => onFinish("assistant")}>Ask &amp; Act</Button>
          </div>
        </>
      )}
    </>
  );
}
