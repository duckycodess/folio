//! Virtual collections (#78, ADR 0013): named groups of document references
//! that leave every file where it is. Keeping or editing one changes no file,
//! so it is a plain native command, not an action plan, and it is not recorded
//! in Activity. Members follow Folio's own renames, moves and deletions; a file
//! changed outside Folio becomes a missing member and is never re-found by guess.

use std::collections::HashSet;

use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::ai_boundary::parse_document_id;
use crate::db::NativeResult;
use crate::error::{error, ErrorCode, FolioError};
use crate::identity::document_id;
use crate::workspace::{self, ScopedRoot};

pub const MAX_COLLECTION_NAME_CHARS: usize = 80;
pub const MAX_COLLECTIONS: usize = 200;
pub const MAX_MEMBERS: usize = 500;

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct CollectionMember {
    pub document_id: String,
    pub relative_path: String,
    /// The file is no longer at this path and Folio didn't move it: it was
    /// renamed, moved or deleted outside Folio.
    pub missing: bool,
}

#[derive(Serialize, Debug, Clone, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualCollection {
    pub id: String,
    pub workspace_id: String,
    pub name: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub members: Vec<CollectionMember>,
}

/// A member of a suggested collection, with the revision the analysis read.
#[derive(Deserialize, Debug, Clone)]
#[serde(rename_all = "camelCase")]
pub struct KeptMember {
    pub document_id: String,
    pub expected_content_hash: String,
}

fn invalid(message: &str) -> FolioError {
    error(ErrorCode::OperationUnsupported, message)
}

/// Whitespace collapsed; 1 to 80 characters and no control characters.
pub fn clean_name(raw: &str) -> NativeResult<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(invalid("Give the collection a name.").with_detail("reason", "nameEmpty"));
    }
    if name.chars().count() > MAX_COLLECTION_NAME_CHARS || name.chars().any(char::is_control) {
        return Err(invalid("Collection names are at most 80 characters, without control characters.").with_detail("reason", "nameInvalid"));
    }
    Ok(name)
}

fn new_id(workspace_id: &str, name: &str, now: i64, conn: &Connection) -> NativeResult<String> {
    let count: i64 = conn.query_row("SELECT count(*) FROM collections", [], |row| row.get(0))?;
    let mut hasher = Sha256::new();
    for part in [workspace_id, name, &now.to_string(), &count.to_string(), &format!("{:?}", std::time::Instant::now())] {
        hasher.update(part.as_bytes());
        hasher.update([0]);
    }
    Ok(format!("collection-{}", &hex::encode(hasher.finalize())[..16]))
}

/// The collection, if it belongs to this workspace.
fn owned(conn: &Connection, workspace_id: &str, collection_id: &str) -> NativeResult<()> {
    let found: Option<String> = conn
        .query_row("SELECT workspace_id FROM collections WHERE id = ?1", [collection_id], |row| row.get(0))
        .optional()?;
    match found {
        Some(owner) if owner == workspace_id => Ok(()),
        _ => Err(error(ErrorCode::TargetMissing, "That collection no longer exists.").with_detail("collectionId", collection_id)),
    }
}

fn unique_ids<'a>(ids: impl Iterator<Item = &'a str>) -> NativeResult<Vec<&'a str>> {
    let mut seen = HashSet::new();
    let ids = ids.filter(|id| seen.insert(*id)).collect::<Vec<_>>();
    if ids.len() > MAX_MEMBERS {
        return Err(invalid("A collection holds at most 500 files.").with_detail("reason", "tooManyMembers"));
    }
    Ok(ids)
}

/// Keeps a suggested collection. Each member must still have the revision the
/// analysis read, so a file that changed since is not filed by a stale reason.
pub fn keep(conn: &mut Connection, root: &ScopedRoot, name: &str, members: &[KeptMember], now: i64) -> NativeResult<VirtualCollection> {
    let name = clean_name(name)?;
    let ids = unique_ids(members.iter().map(|member| member.document_id.as_str()))?;
    if ids.len() < 2 {
        return Err(invalid("A collection needs at least two files.").with_detail("reason", "tooFewMembers"));
    }
    let mut paths = Vec::new();
    for member in members.iter().filter(|member| ids.contains(&member.document_id.as_str())) {
        if paths.iter().any(|(id, _): &(String, String)| id == &member.document_id) {
            continue;
        }
        let relative_path = parse_document_id(&root.id, &member.document_id)?;
        let current = workspace::document_hash(&root.path, &relative_path)?;
        if current != member.expected_content_hash {
            return Err(error(ErrorCode::TargetChanged, "A file in this group changed since Folio analyzed it. Analyze again.").with_detail("path", relative_path.as_str()));
        }
        paths.push((member.document_id.clone(), relative_path));
    }
    create(conn, root, &name, &paths, now)
}

fn create(conn: &mut Connection, root: &ScopedRoot, name: &str, paths: &[(String, String)], now: i64) -> NativeResult<VirtualCollection> {
    let count: i64 = conn.query_row("SELECT count(*) FROM collections WHERE workspace_id = ?1", [&root.id], |row| row.get(0))?;
    if count as usize >= MAX_COLLECTIONS {
        return Err(invalid("This folder already has 200 collections. Remove one first.").with_detail("reason", "tooManyCollections"));
    }
    let id = new_id(&root.id, name, now, conn)?;
    let tx = conn.transaction()?;
    tx.execute("INSERT INTO collections (id, workspace_id, name, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", params![id, root.id, name, now.to_string()])?;
    for (document, relative_path) in paths {
        tx.execute("INSERT INTO collection_members (collection_id, document_id, relative_path, added_at) VALUES (?1, ?2, ?3, ?4)", params![id, document, relative_path, now.to_string()])?;
    }
    tx.commit()?;
    get(conn, root, &id)
}

pub fn rename(conn: &Connection, root: &ScopedRoot, collection_id: &str, name: &str, now: i64) -> NativeResult<VirtualCollection> {
    owned(conn, &root.id, collection_id)?;
    let name = clean_name(name)?;
    conn.execute("UPDATE collections SET name = ?1, updated_at = ?2 WHERE id = ?3", params![name, now.to_string(), collection_id])?;
    get(conn, root, collection_id)
}

/// Removes the collection. Its files stay exactly where they are.
pub fn remove(conn: &Connection, workspace_id: &str, collection_id: &str) -> NativeResult<()> {
    owned(conn, workspace_id, collection_id)?;
    conn.execute("DELETE FROM collections WHERE id = ?1", [collection_id])?;
    Ok(())
}

pub fn add_members(conn: &mut Connection, root: &ScopedRoot, collection_id: &str, document_ids: &[String], now: i64) -> NativeResult<VirtualCollection> {
    owned(conn, &root.id, collection_id)?;
    let ids = unique_ids(document_ids.iter().map(String::as_str))?;
    let present: i64 = conn.query_row("SELECT count(*) FROM collection_members WHERE collection_id = ?1", [collection_id], |row| row.get(0))?;
    if present as usize + ids.len() > MAX_MEMBERS {
        return Err(invalid("A collection holds at most 500 files.").with_detail("reason", "tooManyMembers"));
    }
    let mut paths = Vec::new();
    for id in ids {
        let relative_path = parse_document_id(&root.id, id)?;
        workspace::resolve_document(&root.path, &relative_path)?;
        paths.push((id, relative_path));
    }
    let tx = conn.transaction()?;
    for (id, relative_path) in paths {
        // Adding a file Folio had deleted and then restored again simply clears the mark.
        tx.execute(
            "INSERT INTO collection_members (collection_id, document_id, relative_path, added_at) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT (collection_id, document_id) DO UPDATE SET removed_by_history_id = NULL, relative_path = excluded.relative_path",
            params![collection_id, id, relative_path, now.to_string()],
        )?;
    }
    tx.execute("UPDATE collections SET updated_at = ?1 WHERE id = ?2", params![now.to_string(), collection_id])?;
    tx.commit()?;
    get(conn, root, collection_id)
}

/// Takes files out of the collection. The files themselves are not touched.
pub fn remove_members(conn: &Connection, root: &ScopedRoot, collection_id: &str, document_ids: &[String], now: i64) -> NativeResult<VirtualCollection> {
    owned(conn, &root.id, collection_id)?;
    for id in unique_ids(document_ids.iter().map(String::as_str))? {
        conn.execute("DELETE FROM collection_members WHERE collection_id = ?1 AND document_id = ?2", params![collection_id, id])?;
    }
    conn.execute("UPDATE collections SET updated_at = ?1 WHERE id = ?2", params![now.to_string(), collection_id])?;
    get(conn, root, collection_id)
}

pub fn get(conn: &Connection, root: &ScopedRoot, collection_id: &str) -> NativeResult<VirtualCollection> {
    owned(conn, &root.id, collection_id)?;
    list(conn, root)?
        .into_iter()
        .find(|collection| collection.id == collection_id)
        .ok_or_else(|| error(ErrorCode::TargetMissing, "That collection no longer exists.").with_detail("collectionId", collection_id))
}

/// Every collection of the workspace, newest first. Whether each member is
/// missing is checked against the folder now.
pub fn list(conn: &Connection, root: &ScopedRoot) -> NativeResult<Vec<VirtualCollection>> {
    let mut collections = {
        let mut statement = conn.prepare("SELECT id, name, created_at, updated_at FROM collections WHERE workspace_id = ?1 ORDER BY CAST(created_at AS INTEGER) DESC, rowid DESC")?;
        let rows = statement.query_map([&root.id], |row| {
            let created: String = row.get(2)?;
            let updated: String = row.get(3)?;
            Ok(VirtualCollection {
                id: row.get(0)?,
                workspace_id: root.id.clone(),
                name: row.get(1)?,
                created_at: created.parse().unwrap_or_default(),
                updated_at: updated.parse().unwrap_or_default(),
                members: Vec::new(),
            })
        })?;
        rows.collect::<Result<Vec<_>, _>>()?
    };
    let mut statement = conn.prepare("SELECT document_id, relative_path FROM collection_members WHERE collection_id = ?1 AND removed_by_history_id IS NULL ORDER BY relative_path")?;
    for collection in &mut collections {
        let rows = statement.query_map([&collection.id], |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)))?;
        for row in rows {
            let (document_id, relative_path) = row?;
            let missing = workspace::resolve_document(&root.path, &relative_path).is_err();
            collection.members.push(CollectionMember { document_id, relative_path, missing });
        }
    }
    Ok(collections)
}

/// The members Folio can still find, for analyzing a collection.
pub fn present_member_ids(conn: &Connection, root: &ScopedRoot, collection_id: &str) -> NativeResult<HashSet<String>> {
    Ok(get(conn, root, collection_id)?
        .members
        .into_iter()
        .filter(|member| !member.missing)
        .map(|member| member.document_id)
        .collect())
}

/* ----------------------------------------------- following Folio's own changes */

/// Folio renamed or moved a file (or undid that): its memberships move with it.
pub fn follow_relocation(conn: &Connection, workspace_id: &str, from: &str, to: &str) -> NativeResult<()> {
    let (old, new) = (document_id(workspace_id, from), document_id(workspace_id, to));
    // A stale member already using the new identity gives way to the one that moved.
    conn.execute(
        "DELETE FROM collection_members WHERE document_id = ?2 AND collection_id IN (SELECT collection_id FROM collection_members WHERE document_id = ?1)",
        params![old, new],
    )?;
    conn.execute("UPDATE collection_members SET document_id = ?2, relative_path = ?3 WHERE document_id = ?1", params![old, new, to])?;
    Ok(())
}

/// Folio deleted a file: it leaves its collections until that deletion is undone.
pub fn follow_deletion(conn: &Connection, workspace_id: &str, relative_path: &str, history_entry_id: &str) -> NativeResult<()> {
    conn.execute(
        "UPDATE collection_members SET removed_by_history_id = ?2 WHERE document_id = ?1 AND removed_by_history_id IS NULL",
        params![document_id(workspace_id, relative_path), history_entry_id],
    )?;
    Ok(())
}

/// Undo restored a file Folio deleted: it returns to the collections it left.
pub fn follow_restore(conn: &Connection, history_entry_id: &str) -> NativeResult<()> {
    conn.execute("UPDATE collection_members SET removed_by_history_id = NULL WHERE removed_by_history_id = ?1", [history_entry_id])?;
    Ok(())
}

/// Undo removed a file Folio created: it leaves its collections for good.
pub fn follow_removal(conn: &Connection, workspace_id: &str, relative_path: &str) -> NativeResult<()> {
    conn.execute("DELETE FROM collection_members WHERE document_id = ?1", [document_id(workspace_id, relative_path)])?;
    Ok(())
}

/// Memberships of deleted files whose deletion can no longer be undone.
pub fn purge_unrecoverable(conn: &Connection) -> NativeResult<()> {
    conn.execute(
        "DELETE FROM collection_members WHERE removed_by_history_id IN (SELECT id FROM history WHERE recoverable = 0 AND undone_at IS NULL)",
        [],
    )?;
    Ok(())
}

/// The writer calls these after a file change has already happened, so a failure
/// is reported and never undoes or hides the change; the member shows as missing.
pub fn best_effort(result: NativeResult<()>, what: &str) {
    if let Err(failure) = result {
        eprintln!("Folio changed {what} but could not update its collections: {}", failure.message);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::index::tests::{fixture_workspace, id_of, scan};
    use std::fs;

    const PLAN: &str = "projects/project-plan.md";
    const NOTES: &str = "meetings/meeting-notes.md";

    fn kept(root: &ScopedRoot, paths: &[&str]) -> Vec<KeptMember> {
        paths
            .iter()
            .map(|path| KeptMember { document_id: id_of(root, path), expected_content_hash: workspace::document_hash(&root.path, path).unwrap() })
            .collect()
    }

    fn member_paths(collection: &VirtualCollection) -> Vec<(&str, bool)> {
        collection.members.iter().map(|member| (member.relative_path.as_str(), member.missing)).collect()
    }

    fn listing(folder: &std::path::Path) -> Vec<(String, Vec<u8>)> {
        let mut files = walkdir::WalkDir::new(folder)
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().is_file())
            .map(|entry| (entry.path().display().to_string(), fs::read(entry.path()).unwrap()))
            .collect::<Vec<_>>();
        files.sort();
        files
    }

    #[test]
    fn keeping_a_collection_changes_no_file_and_stores_references() {
        let (folder, mut conn, root) = fixture_workspace();
        scan(&mut conn, &root);
        let before = listing(folder.path());
        let collection = keep(&mut conn, &root, "  Project   deadlines ", &kept(&root, &[PLAN, NOTES]), 10).unwrap();
        assert_eq!(collection.name, "Project deadlines");
        assert_eq!(member_paths(&collection), [(NOTES, false), (PLAN, false)]);
        assert_eq!(listing(folder.path()), before, "a collection never moves or copies a file");
        let history: i64 = conn.query_row("SELECT count(*) FROM history", [], |row| row.get(0)).unwrap();
        assert_eq!(history, 0, "keeping a collection is not a file change in Activity");
        assert_eq!(list(&conn, &root).unwrap(), vec![collection]);
    }

    #[test]
    fn a_member_that_changed_since_the_analysis_is_refused() {
        let (_folder, mut conn, root) = fixture_workspace();
        let members = kept(&root, &[PLAN, NOTES]);
        fs::write(root.path.join(PLAN), "changed after the analysis").unwrap();
        let failure = keep(&mut conn, &root, "Deadlines", &members, 10).unwrap_err();
        assert_eq!(failure.code, ErrorCode::TargetChanged);
        assert!(list(&conn, &root).unwrap().is_empty());
    }

    #[test]
    fn members_must_belong_to_the_folder_and_names_are_checked() {
        let (_folder, mut conn, root) = fixture_workspace();
        let mut members = kept(&root, &[PLAN, NOTES]);
        members[1].document_id = "other-workspace:notes/paalala.md".into();
        assert_eq!(keep(&mut conn, &root, "Deadlines", &members, 1).unwrap_err().code, ErrorCode::WorkspaceNotAuthorized);
        let escape = vec![KeptMember { document_id: format!("{}:../outside.md", root.id), expected_content_hash: "sha256:x".into() }, kept(&root, &[PLAN]).remove(0)];
        assert!(keep(&mut conn, &root, "Deadlines", &escape, 1).is_err());
        assert_eq!(keep(&mut conn, &root, "   ", &kept(&root, &[PLAN, NOTES]), 1).unwrap_err().details.unwrap()["reason"], "nameEmpty");
        assert_eq!(keep(&mut conn, &root, &"x".repeat(81), &kept(&root, &[PLAN, NOTES]), 1).unwrap_err().details.unwrap()["reason"], "nameInvalid");
        assert_eq!(keep(&mut conn, &root, "One", &kept(&root, &[PLAN, PLAN]), 1).unwrap_err().details.unwrap()["reason"], "tooFewMembers");
        assert!(list(&conn, &root).unwrap().is_empty());
    }

    #[test]
    fn collections_may_share_files_and_edits_keep_files_in_place() {
        let (folder, mut conn, root) = fixture_workspace();
        let before = listing(folder.path());
        let deadlines = keep(&mut conn, &root, "Deadlines", &kept(&root, &[PLAN, NOTES]), 1).unwrap();
        let thesis = keep(&mut conn, &root, "Thesis", &kept(&root, &[PLAN, "notes/paalala.md"]), 2).unwrap();
        assert_eq!(list(&conn, &root).unwrap().iter().map(|collection| collection.name.as_str()).collect::<Vec<_>>(), ["Thesis", "Deadlines"]);

        let renamed = rename(&conn, &root, &deadlines.id, "Mga deadline", 3).unwrap();
        assert_eq!((renamed.name.as_str(), renamed.updated_at), ("Mga deadline", 3));
        let fewer = remove_members(&conn, &root, &thesis.id, &[id_of(&root, PLAN)], 4).unwrap();
        assert_eq!(member_paths(&fewer), [("notes/paalala.md", false)]);
        let more = add_members(&mut conn, &root, &thesis.id, &[id_of(&root, "courses/math-review.md")], 5).unwrap();
        assert_eq!(more.members.len(), 2);
        remove(&conn, &root.id, &thesis.id).unwrap();
        assert_eq!(list(&conn, &root).unwrap().len(), 1);
        assert_eq!(remove(&conn, &root.id, &thesis.id).unwrap_err().code, ErrorCode::TargetMissing);
        assert_eq!(listing(folder.path()), before);
    }

    #[test]
    fn a_collection_from_another_folder_cannot_be_changed() {
        let (_folder, mut conn, root) = fixture_workspace();
        let collection = keep(&mut conn, &root, "Deadlines", &kept(&root, &[PLAN, NOTES]), 1).unwrap();
        assert_eq!(remove(&conn, "another-workspace", &collection.id).unwrap_err().code, ErrorCode::TargetMissing);
        assert_eq!(list(&conn, &root).unwrap().len(), 1);
    }

    #[test]
    fn members_follow_folio_relocations_and_deletions_and_their_undo() {
        let (_folder, mut conn, root) = fixture_workspace();
        let collection = keep(&mut conn, &root, "Deadlines", &kept(&root, &[PLAN, NOTES]), 1).unwrap();
        follow_relocation(&conn, &root.id, PLAN, "projects/deadline-plan.md").unwrap();
        fs::rename(root.path.join(PLAN), root.path.join("projects/deadline-plan.md")).unwrap();
        assert_eq!(member_paths(&get(&conn, &root, &collection.id).unwrap()), [(NOTES, false), ("projects/deadline-plan.md", false)]);

        follow_deletion(&conn, &root.id, NOTES, "history-p-0").unwrap();
        assert_eq!(member_paths(&get(&conn, &root, &collection.id).unwrap()), [("projects/deadline-plan.md", false)]);
        follow_restore(&conn, "history-p-0").unwrap();
        assert_eq!(get(&conn, &root, &collection.id).unwrap().members.len(), 2);

        follow_removal(&conn, &root.id, NOTES).unwrap();
        assert_eq!(member_paths(&get(&conn, &root, &collection.id).unwrap()), [("projects/deadline-plan.md", false)]);
    }

    #[test]
    fn a_file_renamed_outside_folio_is_a_missing_member_and_is_not_rematched() {
        let (_folder, mut conn, root) = fixture_workspace();
        let collection = keep(&mut conn, &root, "Deadlines", &kept(&root, &[PLAN, NOTES]), 1).unwrap();
        // Same bytes under a new name: Folio still doesn't guess that it's the member.
        fs::rename(root.path.join(PLAN), root.path.join("projects/renamed-elsewhere.md")).unwrap();
        assert_eq!(member_paths(&get(&conn, &root, &collection.id).unwrap()), [(NOTES, false), (PLAN, true)]);
        assert_eq!(present_member_ids(&conn, &root, &collection.id).unwrap(), HashSet::from([id_of(&root, NOTES)]));
        let cleaned = remove_members(&conn, &root, &collection.id, &[id_of(&root, PLAN)], 2).unwrap();
        assert_eq!(member_paths(&cleaned), [(NOTES, false)]);
    }
}
