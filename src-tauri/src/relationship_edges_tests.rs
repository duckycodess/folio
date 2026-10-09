//! Retained-edge semantics (stored candidates, read-time displayed union
//! top-K) and the active-space filter Ripple and the UI share (R3-1).

use std::collections::BTreeSet;

use folio_core::contracts::{OffsetUnit, SourcePassage};
use folio_core::relationships::{
    AiRelationshipKind, DiscoveredRelationship, MAX_AI_EDGES_PER_DOC,
    MAX_STORED_CANDIDATES_PER_ENDPOINT,
};
use rusqlite::{params, Connection, TransactionBehavior};

use crate::db;
use crate::index::{self, displayed_ai_cte, EmbeddingSpace, Relationship};
use crate::ripple;

const WORKSPACE: &str = "w";

fn workspace() -> Connection {
    let conn = db::open_in_memory().unwrap();
    conn.execute(
        "INSERT INTO workspaces (id, root_path, authorized_at) VALUES (?1, '/w', '0')",
        [WORKSPACE],
    )
    .unwrap();
    conn
}

fn document_id(number: usize) -> String {
    format!("doc-{number:03}")
}

fn add_documents(conn: &Connection, count: usize) -> Vec<String> {
    (0..count)
        .map(|number| {
            let id = document_id(number);
            conn.execute(
                "INSERT INTO documents (id, workspace_id, relative_path, content_hash, media_type, size_bytes, modified_at, name) VALUES (?1, ?2, ?3, ?4, 'text/markdown', 1, '0', ?3)",
                params![id, WORKSPACE, format!("{id}.md"), hash(number)],
            )
            .unwrap();
            id
        })
        .collect()
}

fn hash(number: usize) -> String {
    format!("sha256:doc-{number:03}")
}

fn space(conn: &Connection, revision: &str) -> String {
    index::register_space(
        conn,
        &EmbeddingSpace {
            model_id: "m".into(),
            revision: revision.into(),
            quantization: "q".into(),
            dimensions: 2,
            preprocessing_fingerprint: "p".into(),
        },
    )
    .unwrap()
}

fn passage(number: usize) -> SourcePassage {
    SourcePassage {
        document_id: document_id(number),
        document_content_hash: hash(number),
        offset_unit: OffsetUnit::Utf8Byte,
        start: 0,
        end: 5,
        text: "hello".into(),
        page: None,
    }
}

/// An edge between two documents; the lower id is the source, as discovery does.
fn edge(
    kind: AiRelationshipKind,
    left: usize,
    right: usize,
    cosine: f32,
    space: &str,
) -> DiscoveredRelationship {
    let (source, target) = (left.min(right), left.max(right));
    DiscoveredRelationship {
        kind,
        source_id: document_id(source),
        target_id: document_id(target),
        source_content_hash: hash(source),
        target_content_hash: hash(target),
        space_fingerprint: space.to_owned(),
        score: matches!(kind, AiRelationshipKind::Similarity).then_some(cosine),
        confidence: None,
        discovery_cosine: cosine,
        source_evidence: vec![passage(source)],
        target_evidence: vec![passage(target)],
    }
}

fn insert(conn: &mut Connection, space: &str, edges: &[DiscoveredRelationship]) {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    index::insert_candidate_edges(&tx, WORKSPACE, space, edges, 1).unwrap();
    tx.commit().unwrap();
}

fn displayed_ids(conn: &Connection, space: &str) -> BTreeSet<String> {
    let sql = format!("{}SELECT id FROM displayed", displayed_ai_cte());
    conn.prepare(&sql)
        .unwrap()
        .query_map(params!["", space], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

fn stored_ids(conn: &Connection, space: &str) -> BTreeSet<String> {
    conn.prepare("SELECT id FROM relationships WHERE space_fingerprint = ?1")
        .unwrap()
        .query_map([space], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// Independent statement of the display rule: an edge shows when it is among
/// the top `MAX_AI_EDGES_PER_DOC` at either endpoint, ranked by (cosine
/// descending, other document id ascending).
fn reference_displayed(edges: &[(usize, usize, f32)]) -> BTreeSet<(usize, usize)> {
    let mut shown = BTreeSet::new();
    let documents: BTreeSet<usize> = edges.iter().flat_map(|e| [e.0, e.1]).collect();
    for document in documents {
        let mut mine: Vec<(f32, usize, (usize, usize))> = edges
            .iter()
            .filter(|e| e.0 == document || e.1 == document)
            .map(|e| {
                let other = if e.0 == document { e.1 } else { e.0 };
                (e.2, other, (e.0.min(e.1), e.0.max(e.1)))
            })
            .collect();
        mine.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
        shown.extend(mine.into_iter().take(MAX_AI_EDGES_PER_DOC).map(|m| m.2));
    }
    shown
}

fn complete_graph(count: usize) -> Vec<(usize, usize, f32)> {
    let mut edges = Vec::new();
    for left in 0..count {
        for right in left + 1..count {
            // Deterministic, with deliberate ties (modulus 23).
            let cosine = 0.80 + ((left * 7 + right * 13) % 23) as f32 / 200.0;
            edges.push((left, right, cosine));
        }
    }
    edges
}

fn ids_for(conn: &Connection, space: &str, pairs: &BTreeSet<(usize, usize)>) -> BTreeSet<String> {
    pairs
        .iter()
        .map(|(left, right)| {
            conn.query_row(
                "SELECT id FROM relationships WHERE space_fingerprint = ?1 AND source_document_id = ?2 AND target_document_id = ?3 AND relationship_type = 'similarity'",
                params![space, document_id(*left), document_id(*right)],
                |row| row.get(0),
            )
            .unwrap()
        })
        .collect()
}

fn shuffled<T: Clone>(items: &[T], seed: u64) -> Vec<T> {
    let mut items = items.to_vec();
    let mut state = seed;
    for index in (1..items.len()).rev() {
        state = state
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        items.swap(index, (state >> 33) as usize % (index + 1));
    }
    items
}

fn build(order: &[(usize, usize, f32)], count: usize) -> (Connection, String) {
    let mut conn = workspace();
    add_documents(&conn, count);
    let space = space(&conn, "r1");
    let edges: Vec<_> = order
        .iter()
        .map(|&(left, right, cosine)| {
            edge(AiRelationshipKind::Similarity, left, right, cosine, &space)
        })
        .collect();
    insert(&mut conn, &space, &edges);
    (conn, space)
}

#[test]
fn displayed_edges_do_not_depend_on_processing_order_when_nothing_overflowed() {
    let edges = complete_graph(10);
    assert!(
        edges.len() > 9,
        "each document has 9 candidates, above the display bound"
    );
    let expected = reference_displayed(&edges);
    assert!(expected.len() < edges.len(), "the rule hides something");

    let mut previous: Option<BTreeSet<String>> = None;
    for seed in 0..6 {
        let order = if seed == 0 {
            edges.clone()
        } else {
            shuffled(&edges, seed)
        };
        let (conn, space) = build(&order, 10);
        let overflow: i64 = conn
            .query_row(
                "SELECT count(*) FROM ai_relationship_coverage WHERE candidate_overflow = 1",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(overflow, 0);
        assert_eq!(
            stored_ids(&conn, &space).len(),
            edges.len(),
            "every candidate is stored below the cap"
        );
        let shown = displayed_ids(&conn, &space);
        assert_eq!(
            shown,
            ids_for(&conn, &space, &expected),
            "seed {seed} matches the stated rule"
        );
        if let Some(previous) = &previous {
            assert_eq!(&shown, previous, "seed {seed} equals the first order");
        }
        previous = Some(shown);
    }
}

#[test]
fn list_relationships_shows_exactly_the_displayed_edges() {
    let edges = complete_graph(10);
    let (conn, space) = build(&edges, 10);
    let listed: BTreeSet<(String, String)> =
        index::list_relationships(&conn, WORKSPACE, Some(&space))
            .unwrap()
            .into_iter()
            .filter_map(|relationship| match relationship {
                Relationship::Similarity(edge) => Some((edge.source_id, edge.target_id)),
                _ => None,
            })
            .collect();
    let expected: BTreeSet<(String, String)> = reference_displayed(&edges)
        .into_iter()
        .map(|(left, right)| (document_id(left), document_id(right)))
        .collect();
    assert_eq!(listed, expected);
}

#[test]
fn overflow_is_bounded_flagged_on_both_endpoints_and_deterministic_for_a_schedule() {
    let leaves = MAX_STORED_CANDIDATES_PER_ENDPOINT + 2;
    // Document 0 is the hub; leaf n has cosine rising with n, so leaves 1 and 2
    // are the weakest and are the ones evicted.
    let edges_for = |space: &str| -> Vec<DiscoveredRelationship> {
        (1..=leaves)
            .map(|leaf| {
                edge(
                    AiRelationshipKind::Similarity,
                    0,
                    leaf,
                    0.80 + leaf as f32 / 1000.0,
                    space,
                )
            })
            .collect()
    };
    let run = |one_by_one: bool| {
        let mut conn = workspace();
        add_documents(&conn, leaves + 1);
        let space = space(&conn, "r1");
        for number in 0..=leaves {
            conn.execute(
                "INSERT INTO ai_relationship_coverage (workspace_id, space_id, document_id, content_hash, seq, updated_at) VALUES (?1, ?2, ?3, ?4, ?5, '0')",
                params![WORKSPACE, space, document_id(number), hash(number), number as i64 + 1],
            )
            .unwrap();
        }
        let edges = edges_for(&space);
        if one_by_one {
            for single in &edges {
                insert(&mut conn, &space, std::slice::from_ref(single));
            }
        } else {
            insert(&mut conn, &space, &edges);
        }
        let stored: i64 = conn
            .query_row(
                "SELECT count(*) FROM relationships WHERE space_fingerprint = ?1",
                [&space],
                |row| row.get(0),
            )
            .unwrap();
        let flagged: BTreeSet<String> = conn
            .prepare(
                "SELECT document_id FROM ai_relationship_coverage WHERE candidate_overflow = 1",
            )
            .unwrap()
            .query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        let kept: BTreeSet<String> = conn
            .prepare("SELECT target_document_id FROM relationships WHERE space_fingerprint = ?1")
            .unwrap()
            .query_map([&space], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap();
        (stored as usize, flagged, kept)
    };

    let (stored, flagged, kept) = run(false);
    assert_eq!(
        stored, MAX_STORED_CANDIDATES_PER_ENDPOINT,
        "the hub never holds more than the cap"
    );
    assert_eq!(
        flagged,
        BTreeSet::from([document_id(0), document_id(1), document_id(2)]),
        "the hub and both evicted leaves are flagged, and nobody else"
    );
    assert!(!kept.contains(&document_id(1)) && !kept.contains(&document_id(2)));
    assert!(kept.contains(&document_id(3)));
    // The same candidates in a different schedule evict the same weakest edges.
    assert_eq!(run(true), (stored, flagged, kept));
}

#[test]
fn an_edit_removes_candidates_and_resurfaces_hidden_ones() {
    let edges = complete_graph(10);
    let (mut conn, space) = build(&edges, 10);
    let shown = displayed_ids(&conn, &space);
    let hidden: Vec<_> = edges
        .iter()
        .filter(|e| !reference_displayed(&edges).contains(&(e.0, e.1)))
        .collect();
    let (hidden_left, hidden_right, _) = *hidden[0];
    // Edit a document that is neither endpoint of the hidden edge.
    let edited = (0..10)
        .find(|n| *n != hidden_left && *n != hidden_right)
        .unwrap();
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .unwrap();
    index::clear_derived(&tx, &document_id(edited)).unwrap();
    tx.commit().unwrap();

    let remaining: Vec<(usize, usize, f32)> = edges
        .iter()
        .copied()
        .filter(|e| e.0 != edited && e.1 != edited)
        .collect();
    let after = displayed_ids(&conn, &space);
    assert_eq!(
        after,
        ids_for(&conn, &space, &reference_displayed(&remaining))
    );
    let resurfaced = ids_for(
        &conn,
        &space,
        &BTreeSet::from([(hidden_left, hidden_right)]),
    );
    assert!(
        !shown.is_superset(&resurfaced),
        "it was hidden before the edit"
    );
    assert!(
        after.is_superset(&resurfaced),
        "a stored candidate comes back once it fits within the bound"
    );
}

fn deletion_candidates(
    conn: &Connection,
    document: usize,
    active: Option<&str>,
) -> BTreeSet<String> {
    let target = index::get_document(conn, WORKSPACE, &document_id(document)).unwrap();
    ripple::deletion_impacts(conn, WORKSPACE, &target, active)
        .unwrap()
        .into_iter()
        .map(|candidate| candidate.document_id)
        .collect()
}

#[test]
fn ripple_and_the_list_use_the_identical_displayed_set() {
    let edges = complete_graph(10);
    let (conn, space) = build(&edges, 10);
    let listed = index::list_relationships(&conn, WORKSPACE, Some(&space)).unwrap();
    for document in 0..10 {
        let id = document_id(document);
        let from_list: BTreeSet<String> = listed
            .iter()
            .filter_map(|relationship| match relationship {
                Relationship::Similarity(edge) if edge.source_id == id => {
                    Some(edge.target_id.clone())
                }
                Relationship::Similarity(edge) if edge.target_id == id => {
                    Some(edge.source_id.clone())
                }
                _ => None,
            })
            .collect();
        assert_eq!(
            deletion_candidates(&conn, document, Some(&space)),
            from_list,
            "document {document}"
        );
    }
}

#[test]
fn ripple_ignores_other_spaces_hidden_candidates_and_everything_without_an_active_space() {
    let edges = complete_graph(10);
    let (mut conn, space) = build(&edges, 10);
    let old_space = space_for(&conn, "r0");
    // A row from a superseded space touching document 0 and a document it has no edge to.
    let unrelated = 10;
    add_documents_from(&conn, unrelated, 1);
    insert(
        &mut conn,
        &old_space,
        &[edge(
            AiRelationshipKind::Similarity,
            0,
            unrelated,
            0.99,
            &old_space,
        )],
    );

    assert!(
        !deletion_candidates(&conn, 0, Some(&space)).contains(&document_id(unrelated)),
        "an old-space row is ignored"
    );
    assert!(
        deletion_candidates(&conn, 0, Some(&old_space)).contains(&document_id(unrelated)),
        "it is the active space's own row when that space is active"
    );
    assert!(
        deletion_candidates(&conn, 0, None).is_empty(),
        "no active space, no AI rows"
    );

    // A stored candidate outside the displayed set is not used.
    let hidden = edges
        .iter()
        .find(|e| !reference_displayed(&edges).contains(&(e.0, e.1)))
        .unwrap();
    let (a, b) = (hidden.0, hidden.1);
    assert!(!deletion_candidates(&conn, a, Some(&space)).contains(&document_id(b)));
    assert!(!deletion_candidates(&conn, b, Some(&space)).contains(&document_id(a)));

    // Links are read regardless of any space.
    conn.execute(
        "INSERT INTO relationships (id, source_document_id, target_document_id, relationship_type, evidence_json, provenance, confidence, source_content_hash, target_content_hash, created_at) VALUES ('link', ?1, ?2, 'explicitReference', '{}', 'documentLink', NULL, ?3, ?4, '0')",
        params![document_id(unrelated), document_id(a), hash(unrelated), hash(a)],
    )
    .unwrap();
    assert!(deletion_candidates(&conn, a, None).contains(&document_id(unrelated)));
    assert!(deletion_candidates(&conn, a, Some(&space)).contains(&document_id(unrelated)));
}

fn space_for(conn: &Connection, revision: &str) -> String {
    space(conn, revision)
}

fn add_documents_from(conn: &Connection, first: usize, count: usize) {
    for number in first..first + count {
        let id = document_id(number);
        conn.execute(
            "INSERT INTO documents (id, workspace_id, relative_path, content_hash, media_type, size_bytes, modified_at, name) VALUES (?1, ?2, ?3, ?4, 'text/markdown', 1, '0', ?3)",
            params![id, WORKSPACE, format!("{id}.md"), hash(number)],
        )
        .unwrap();
    }
}
