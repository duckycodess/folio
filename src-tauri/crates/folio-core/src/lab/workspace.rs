//! Disposable lab workspaces.
//!
//! Each model runs against a fresh copy of the corpus under
//! `<data dir>/model-lab/runs/<run id>/workspace`. A marker file records that
//! Folio made the directory; nothing without the marker is ever deleted, and no
//! path outside the runs directory is ever touched. A user's workspace is never
//! read or written here.

use crate::error::{CoreError, CoreResult};
use crate::lab::suite::Corpus;
use crate::models::sha256_bytes;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Component, Path, PathBuf};

pub const MARKER_FILE: &str = ".folio-model-lab";
const MARKER_TEXT: &str = "Created by Folio's Model Lab. Safe to delete with its run folder.\n";

/// Relative path to content hash, for every file but the marker.
pub type Snapshot = BTreeMap<String, String>;

fn refuse(message: impl Into<String>) -> CoreError {
    CoreError::Message(format!("Model Lab workspace: {}", message.into()))
}

fn valid_run_id(run_id: &str) -> bool {
    !run_id.is_empty()
        && run_id.len() <= 64
        && run_id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

fn safe_relative(path: &str) -> bool {
    !path.is_empty()
        && !path.contains('\\')
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)))
}

#[derive(Clone, Debug)]
pub struct LabWorkspace {
    root: PathBuf,
}

impl LabWorkspace {
    pub fn root(&self) -> &Path {
        &self.root
    }
}

#[derive(Clone, Debug)]
pub struct LabWorkspaces {
    runs_dir: PathBuf,
}

impl LabWorkspaces {
    pub fn new(data_dir: impl AsRef<Path>) -> Self {
        Self {
            runs_dir: data_dir.as_ref().join("model-lab").join("runs"),
        }
    }

    fn run_dir(&self, run_id: &str) -> CoreResult<PathBuf> {
        if !valid_run_id(run_id) {
            return Err(refuse("the run id is not a plain identifier"));
        }
        Ok(self.runs_dir.join(run_id))
    }

    /// Makes a fresh copy of the corpus for one model, replacing the previous
    /// copy of the same run if Folio made it. An existing directory without the
    /// marker is refused, not replaced.
    pub fn create(&self, run_id: &str, corpus: &Corpus) -> CoreResult<LabWorkspace> {
        let root = self.run_dir(run_id)?.join("workspace");
        if fs::symlink_metadata(&root).is_ok() {
            self.remove_root(&root)?;
        }
        fs::create_dir_all(&root)?;
        for document in &corpus.documents {
            if !safe_relative(&document.relative_path) {
                return Err(refuse(format!(
                    "{} is not a safe relative path",
                    document.relative_path
                )));
            }
            let target = root.join(&document.relative_path);
            if let Some(parent) = target.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::write(&target, document.content.as_bytes())?;
        }
        fs::write(root.join(MARKER_FILE), MARKER_TEXT)?;
        Ok(LabWorkspace { root })
    }

    /// Hashes every file below the workspace. A symbolic link is refused: the
    /// copy holds only files Folio wrote.
    pub fn snapshot(&self, workspace: &LabWorkspace) -> CoreResult<Snapshot> {
        let mut snapshot = Snapshot::new();
        let mut pending = vec![workspace.root.clone()];
        while let Some(dir) = pending.pop() {
            for entry in fs::read_dir(&dir)? {
                let path = entry?.path();
                let metadata = fs::symlink_metadata(&path)?;
                if metadata.file_type().is_symlink() {
                    return Err(refuse(format!("{} is a symbolic link", path.display())));
                }
                if metadata.is_dir() {
                    pending.push(path);
                } else if metadata.is_file() {
                    let relative = path
                        .strip_prefix(&workspace.root)
                        .map_err(|_| refuse("a file escaped the workspace"))?;
                    let relative = relative.to_string_lossy().replace('\\', "/");
                    if relative == MARKER_FILE {
                        continue;
                    }
                    snapshot.insert(relative, sha256_bytes(&fs::read(&path)?));
                }
            }
        }
        Ok(snapshot)
    }

    /// The snapshot a fresh copy of `corpus` must have.
    pub fn expected_snapshot(corpus: &Corpus) -> Snapshot {
        corpus
            .documents
            .iter()
            .map(|document| {
                (
                    document.relative_path.clone(),
                    sha256_bytes(document.content.as_bytes()),
                )
            })
            .collect()
    }

    /// Fails, naming each difference, unless the files are exactly `expected`.
    pub fn verify_unchanged(
        &self,
        workspace: &LabWorkspace,
        expected: &Snapshot,
    ) -> CoreResult<()> {
        let current = self.snapshot(workspace)?;
        let mut differences = Vec::new();
        for (path, hash) in expected {
            match current.get(path) {
                None => differences.push(format!("{path} was removed")),
                Some(now) if now != hash => differences.push(format!("{path} was modified")),
                Some(_) => {}
            }
        }
        for path in current.keys() {
            if !expected.contains_key(path) {
                differences.push(format!("{path} was added"));
            }
        }
        if differences.is_empty() {
            Ok(())
        } else {
            Err(refuse(format!(
                "a proposal-only run changed the disposable copy: {}",
                differences.join("; ")
            )))
        }
    }

    /// Deletes one workspace, only if Folio made it.
    pub fn remove(&self, workspace: &LabWorkspace) -> CoreResult<()> {
        self.remove_root(&workspace.root)
    }

    /// Deletes a run's workspace and then its folder if nothing else is in it.
    pub fn remove_run(&self, run_id: &str) -> CoreResult<()> {
        let run_dir = self.run_dir(run_id)?;
        let root = run_dir.join("workspace");
        if fs::symlink_metadata(&root).is_ok() {
            self.remove_root(&root)?;
        }
        let _ = fs::remove_dir(&run_dir);
        Ok(())
    }

    fn remove_root(&self, root: &Path) -> CoreResult<()> {
        let metadata = fs::symlink_metadata(root)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(refuse("the workspace is not a plain directory"));
        }
        let canonical_root = fs::canonicalize(root)?;
        let canonical_runs = fs::canonicalize(&self.runs_dir)?;
        if !canonical_root.starts_with(&canonical_runs) || canonical_root == canonical_runs {
            return Err(refuse("the workspace is outside the Model Lab runs folder"));
        }
        let marker = fs::symlink_metadata(root.join(MARKER_FILE));
        if !marker.map(|m| m.is_file()).unwrap_or(false) {
            return Err(refuse(
                "the folder has no Model Lab marker, so Folio did not make it and will not delete it",
            ));
        }
        fs::remove_dir_all(root)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lab::suite::CorpusDocument;

    fn corpus() -> Corpus {
        Corpus::from_documents(vec![
            CorpusDocument {
                relative_path: "projects/plan.md".into(),
                content: "Deadline: October 20\n".into(),
            },
            CorpusDocument {
                relative_path: "notes/tala.md".into(),
                content: "Huling araw: October 20\n".into(),
            },
        ])
    }

    fn workspaces() -> (tempfile::TempDir, LabWorkspaces) {
        let dir = tempfile::tempdir().unwrap();
        let workspaces = LabWorkspaces::new(dir.path());
        (dir, workspaces)
    }

    #[test]
    fn a_fresh_copy_holds_exactly_the_corpus_and_a_marker() {
        let (_dir, workspaces) = workspaces();
        let workspace = workspaces.create("run-1", &corpus()).unwrap();
        assert!(workspace.root().join(MARKER_FILE).is_file());
        assert_eq!(
            workspaces.snapshot(&workspace).unwrap(),
            LabWorkspaces::expected_snapshot(&corpus())
        );
    }

    #[test]
    fn verification_names_a_modified_added_or_removed_file() {
        let (_dir, workspaces) = workspaces();
        let workspace = workspaces.create("run-1", &corpus()).unwrap();
        let expected = LabWorkspaces::expected_snapshot(&corpus());
        workspaces.verify_unchanged(&workspace, &expected).unwrap();

        fs::write(workspace.root().join("projects/plan.md"), "October 23\n").unwrap();
        fs::write(workspace.root().join("extra.md"), "new").unwrap();
        fs::remove_file(workspace.root().join("notes/tala.md")).unwrap();
        let message = workspaces
            .verify_unchanged(&workspace, &expected)
            .unwrap_err()
            .to_string();
        assert!(
            message.contains("projects/plan.md was modified"),
            "{message}"
        );
        assert!(message.contains("extra.md was added"), "{message}");
        assert!(message.contains("notes/tala.md was removed"), "{message}");
    }

    #[test]
    fn creating_again_resets_the_copy_for_the_next_model() {
        let (_dir, workspaces) = workspaces();
        let workspace = workspaces.create("run-1", &corpus()).unwrap();
        fs::write(workspace.root().join("projects/plan.md"), "tampered").unwrap();
        fs::write(workspace.root().join("stray.md"), "stray").unwrap();

        let fresh = workspaces.create("run-1", &corpus()).unwrap();
        assert_eq!(
            workspaces.snapshot(&fresh).unwrap(),
            LabWorkspaces::expected_snapshot(&corpus())
        );
    }

    #[test]
    fn a_run_id_that_is_not_a_plain_identifier_is_refused() {
        let (_dir, workspaces) = workspaces();
        for bad in ["", "../escape", "a/b", "a\\b", "with space", ".."] {
            assert!(workspaces.create(bad, &corpus()).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn a_folder_without_the_marker_is_never_replaced_or_deleted() {
        let (dir, workspaces) = workspaces();
        let root = dir.path().join("model-lab/runs/run-1/workspace");
        fs::create_dir_all(&root).unwrap();
        fs::write(root.join("keep.txt"), "not ours").unwrap();

        assert!(workspaces.create("run-1", &corpus()).is_err());
        assert!(workspaces.remove_run("run-1").is_err());
        assert_eq!(
            fs::read_to_string(root.join("keep.txt")).unwrap(),
            "not ours"
        );
    }

    #[test]
    fn removal_stays_inside_the_runs_folder() {
        let (dir, workspaces) = workspaces();
        workspaces.create("run-1", &corpus()).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join(MARKER_FILE), "marker").unwrap();
        fs::write(outside.join("keep.txt"), "mine").unwrap();

        let stray = LabWorkspace {
            root: outside.clone(),
        };
        assert!(workspaces.remove(&stray).is_err());
        assert!(outside.join("keep.txt").is_file());

        let runs = LabWorkspace {
            root: dir.path().join("model-lab/runs"),
        };
        assert!(workspaces.remove(&runs).is_err());
    }

    #[test]
    fn remove_run_deletes_only_what_the_lab_made() {
        let (dir, workspaces) = workspaces();
        workspaces.create("run-1", &corpus()).unwrap();
        workspaces.create("run-2", &corpus()).unwrap();
        workspaces.remove_run("run-1").unwrap();
        assert!(!dir.path().join("model-lab/runs/run-1").exists());
        assert!(dir.path().join("model-lab/runs/run-2/workspace").is_dir());
    }

    #[cfg(unix)]
    #[test]
    fn a_symbolic_link_is_refused_and_its_target_survives() {
        use std::os::unix::fs::symlink;
        let (dir, workspaces) = workspaces();
        let workspace = workspaces.create("run-1", &corpus()).unwrap();
        let outside = dir.path().join("outside");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "private").unwrap();
        symlink(&outside, workspace.root().join("link")).unwrap();
        assert!(workspaces.snapshot(&workspace).is_err());

        let runs_dir = dir.path().join("model-lab/runs/run-3");
        fs::create_dir_all(&runs_dir).unwrap();
        fs::write(outside.join(MARKER_FILE), "marker").unwrap();
        symlink(&outside, runs_dir.join("workspace")).unwrap();
        assert!(workspaces.remove_run("run-3").is_err());
        assert_eq!(
            fs::read_to_string(outside.join("secret.txt")).unwrap(),
            "private"
        );
    }
}
