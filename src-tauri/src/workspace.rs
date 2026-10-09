use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use serde::Serialize;
use walkdir::WalkDir;

const MAX_TEXT_BYTES: u64 = 2 * 1024 * 1024;
const MAX_DOCUMENTS: usize = 5000;

#[derive(Clone)]
pub struct ScopedRoot {
    pub id: String,
    pub path: PathBuf,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentMetadata {
    pub id: String,
    pub relative_path: String,
    pub name: String,
    pub size_bytes: u64,
}

pub fn resolve_document(root: &Path, relative: &str) -> Result<PathBuf, String> {
    if Path::new(relative).is_absolute() || relative.is_empty() {
        return Err("A relative document path is required.".into());
    }
    let root = root.canonicalize().map_err(|_| "The authorized folder is unavailable.")?;
    let resolved = root.join(relative).canonicalize().map_err(|_| "The document is unavailable.")?;
    if !resolved.starts_with(&root) || !resolved.is_file() {
        return Err("The document is outside the authorized folder.".into());
    }
    Ok(resolved)
}

pub fn list_documents(root: &Path) -> Result<Vec<DocumentMetadata>, String> {
    let root = root.canonicalize().map_err(|_| "The authorized folder is unavailable.")?;
    let mut documents = Vec::new();
    for entry in WalkDir::new(&root).follow_links(false) {
        let entry = entry.map_err(|error| error.to_string())?;
        if !entry.file_type().is_file() { continue; }
        let extension = entry.path().extension().and_then(|part| part.to_str()).unwrap_or("").to_lowercase();
        if !matches!(extension.as_str(), "txt" | "md" | "pdf") { continue; }
        let path = entry.path().strip_prefix(&root).map_err(|_| "Document path escaped the folder.")?.to_string_lossy().replace('\\', "/");
        documents.push(DocumentMetadata { id: path.clone(), relative_path: path, name: entry.file_name().to_string_lossy().into_owned(), size_bytes: entry.metadata().map_err(|error| error.to_string())?.len() });
        if documents.len() > MAX_DOCUMENTS { return Err("This starter supports up to 5,000 documents per selected folder. Choose a smaller folder.".into()); }
    }
    documents.sort_by(|a, b| a.relative_path.cmp(&b.relative_path));
    Ok(documents)
}

pub fn read_text(root: &Path, relative: &str) -> Result<String, String> {
    let path = resolve_document(root, relative)?;
    let extension = path.extension().and_then(|part| part.to_str()).unwrap_or("").to_lowercase();
    if !matches!(extension.as_str(), "txt" | "md") { return Err("PDF extraction is not connected yet. This reader supports TXT and Markdown.".into()); }
    let file = fs::File::open(path).map_err(|error| error.to_string())?;
    let mut bytes = Vec::new();
    file.take(MAX_TEXT_BYTES + 1).read_to_end(&mut bytes).map_err(|error| error.to_string())?;
    if bytes.len() as u64 > MAX_TEXT_BYTES { return Err("This starter reads text files up to 2 MiB.".into()); }
    String::from_utf8(bytes).map_err(|_| "This document is not valid UTF-8 text.".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_authorized_filipino_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("tala.md"), "Ang pagpupulong ay sa Biyernes.").unwrap();
        assert_eq!(read_text(root.path(), "tala.md").unwrap(), "Ang pagpupulong ay sa Biyernes.");
    }

    #[test]
    fn rejects_parent_escape_and_absolute_paths() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        fs::write(parent.path().join("outside.md"), "private").unwrap();
        assert!(resolve_document(&root, "../outside.md").is_err());
        assert!(resolve_document(&root, parent.path().join("outside.md").to_str().unwrap()).is_err());
    }

    #[test]
    fn refuses_pdf_as_plain_text() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("paper.pdf"), "%PDF").unwrap();
        assert!(read_text(root.path(), "paper.pdf").is_err());
    }

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_escape() {
        let parent = tempfile::tempdir().unwrap();
        let root = parent.path().join("allowed");
        fs::create_dir(&root).unwrap();
        let outside = parent.path().join("outside.md");
        fs::write(&outside, "private").unwrap();
        std::os::unix::fs::symlink(outside, root.join("link.md")).unwrap();
        assert!(read_text(&root, "link.md").is_err());
    }
}
