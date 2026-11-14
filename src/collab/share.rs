//! A project shared through a folder: a network drive or a folder a sync tool keeps the same on
//! everyone's machine. No server and no account: who can reach the folder takes part.
//!
//! The folder holds each issue as a BCF file of its own (`issues/<guid>.bcf`), so two people
//! working on different issues never write the same file, a mark for each issue that was
//! removed (`issues/<guid>.deleted`), and the decisions on clashes (`clashes.json`). The models
//! stay where each has them: only what is said about them is shared.
//!
//! Syncing reads what the others wrote and writes what is new here. Of an issue both changed,
//! the one changed last stands, with the comments of both.

use bim_bcf::issues::{Issues, Topic};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::Path;

/// What a sync did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Synced {
    /// Issues taken over from the folder (new or newer there) and written to it.
    pub pulled: usize,
    pub pushed: usize,
    /// Issues removed here because another removed them.
    pub removed: usize,
    /// Decisions on clashes taken over and written.
    pub decisions_pulled: usize,
    pub decisions_pushed: usize,
}

impl Synced {
    pub fn nothing(&self) -> bool {
        *self == Synced::default()
    }
}

/// When an issue last changed: the latest of its own dates and those of its comments (ISO
/// dates compare as text).
fn stamp(t: &Topic) -> &str {
    let mut latest = t.creation_date.as_str().max(t.modified_date.as_str());
    for c in &t.comments {
        latest = latest.max(c.date.as_str()).max(c.modified_date.as_str());
    }
    latest
}

fn file_of(topic: &Topic) -> Result<Vec<u8>, String> {
    let mut one = Issues::new();
    let mut topic = topic.clone();
    topic.written_anew();
    one.topics.push(topic);
    one.write()
}

/// Writes a file whole: beside it first, then in its place, so a reader never sees half a file.
fn put(path: &Path, bytes: &[u8]) -> Result<(), String> {
    let temp = path.with_extension("part");
    std::fs::write(&temp, bytes).and_then(|()| std::fs::rename(&temp, path)).map_err(|e| format!("{}: {e}", path.display()))
}

/// Brings the issues here and those in the folder to the same: `removed` are the issues taken
/// away here since the last sync.
pub fn sync_issues(folder: &Path, issues: &mut Issues, removed: &[String]) -> Result<Synced, String> {
    let dir = folder.join("issues");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut done = Synced::default();
    for guid in removed {
        put(&dir.join(format!("{guid}.deleted")), b"")?;
        let _ = std::fs::remove_file(dir.join(format!("{guid}.bcf")));
    }
    let mut there: BTreeMap<String, std::path::PathBuf> = BTreeMap::new();
    let mut gone: HashSet<String> = HashSet::new();
    for entry in std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?.flatten() {
        let path = entry.path();
        let Some(guid) = path.file_stem().map(|s| s.to_string_lossy().into_owned()) else { continue };
        match path.extension().and_then(|e| e.to_str()) {
            Some("bcf") => {
                there.insert(guid, path);
            }
            Some("deleted") => {
                gone.insert(guid);
            }
            _ => {}
        }
    }
    // what another removed goes here too
    let before = issues.topics.len();
    issues.topics.retain(|t| !gone.contains(&t.guid));
    done.removed = before - issues.topics.len();
    for (guid, path) in &there {
        if gone.contains(guid) {
            continue;
        }
        let Some(theirs) = std::fs::read(path).ok().and_then(|bytes| Issues::read(&bytes).ok()).and_then(|mut i| (!i.topics.is_empty()).then(|| i.topics.remove(0))) else {
            // half written, or not an issue: the next sync sees it again
            continue;
        };
        match issues.topics.iter_mut().find(|t| &t.guid == guid) {
            None => {
                issues.topics.push(theirs);
                done.pulled += 1;
            }
            Some(mine) => {
                // the one changed last stands, with the comments of both
                let lacking = |of: &Topic, from: &Topic| -> Vec<_> { from.comments.iter().filter(|c| !of.comments.iter().any(|d| d.guid == c.guid)).cloned().collect() };
                let (from_them, from_me) = (lacking(mine, &theirs), lacking(&theirs, mine));
                let newer_there = stamp(&theirs) > stamp(mine);
                let newer_here = stamp(&theirs) < stamp(mine);
                if newer_there {
                    let own = std::mem::take(&mut mine.comments);
                    *mine = theirs;
                    mine.comments.extend(own.into_iter().filter(|c| from_me.iter().any(|d| d.guid == c.guid)));
                } else {
                    mine.comments.extend(from_them.iter().cloned());
                }
                mine.comments.sort_by(|a, b| a.date.cmp(&b.date));
                if newer_there || !from_them.is_empty() {
                    done.pulled += 1;
                }
                // the folder gets what it lacks
                if newer_here || !from_me.is_empty() {
                    put(path, &file_of(mine)?)?;
                    done.pushed += 1;
                }
            }
        }
    }
    for topic in issues.topics.iter().filter(|t| !there.contains_key(&t.guid)) {
        put(&dir.join(format!("{}.bcf", topic.guid)), &file_of(topic)?)?;
        done.pushed += 1;
    }
    Ok(done)
}

/// A decision on a clash as the folder keeps it.
#[derive(Debug, Clone, Default, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct Shared {
    pub decision: String,
    pub by: String,
    pub date: String,
}

/// Brings the decisions on clashes here and in the folder to the same. `mine` are the clashes
/// here by their lasting key with their decision; `known` is what the folder said at the last
/// sync: a decision that differs from it was made here and goes to the folder, another that
/// differs from the folder's is taken over. Returns the decisions to take over.
pub fn sync_decisions(folder: &Path, mine: &[(String, String)], known: &mut HashMap<String, String>, by: &str, now: &str, done: &mut Synced) -> Result<Vec<(String, String)>, String> {
    let path = folder.join("clashes.json");
    let mut there: BTreeMap<String, Shared> = std::fs::read(&path).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let mut take = Vec::new();
    let mut changed = false;
    for (key, decision) in mine.iter().filter(|(key, _)| !key.is_empty()) {
        let before = known.get(key).map_or("open", String::as_str);
        let theirs = there.get(key).map_or("open", |s| s.decision.as_str());
        if decision != before {
            if decision != theirs {
                there.insert(key.clone(), Shared { decision: decision.clone(), by: by.to_string(), date: now.to_string() });
                changed = true;
                done.decisions_pushed += 1;
            }
        } else if theirs != decision {
            take.push((key.clone(), theirs.to_string()));
            done.decisions_pulled += 1;
        }
    }
    if changed {
        put(&path, &serde_json::to_vec_pretty(&there).map_err(|e| e.to_string())?)?;
    }
    *known = there.into_iter().map(|(k, s)| (k, s.decision)).collect();
    Ok(take)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("o3v-share-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    fn topic(title: &str, date: &str) -> Topic {
        let mut t = Topic::new(title, "an");
        t.creation_date = date.into();
        t.modified_date = date.into();
        t
    }

    #[test]
    fn two_people_see_each_others_issues_and_the_last_change_stands() {
        let dir = folder("issues");
        let (mut an, mut bo) = (Issues::new(), Issues::new());
        an.topics.push(topic("Leiding door wand", "2026-11-14T09:00:00Z"));
        an.topics.push(topic("Deur te smal", "2026-11-14T09:05:00Z"));
        assert_eq!(sync_issues(&dir, &mut an, &[]).unwrap(), Synced { pushed: 2, ..Synced::default() });
        assert_eq!(sync_issues(&dir, &mut bo, &[]).unwrap(), Synced { pulled: 2, ..Synced::default() });
        assert_eq!(bo.topics.len(), 2);
        // bo closes one and comments; an comments on the same before syncing
        let guid = an.topics[0].guid.clone();
        let theirs = bo.topics.iter_mut().find(|t| t.guid == guid).unwrap();
        theirs.status = "Closed".into();
        theirs.modified_date = "2026-11-14T10:00:00Z".into();
        theirs.comment("Verlegd", "bo", None);
        theirs.comments[0].date = "2026-11-14T10:00:00Z".into();
        an.topics[0].comment("Wie pakt dit op?", "an", None);
        an.topics[0].comments[0].date = "2026-11-14T09:30:00Z".into();
        an.topics[0].modified_date = "2026-11-14T09:30:00Z".into();
        assert_eq!(sync_issues(&dir, &mut bo, &[]).unwrap().pushed, 1);
        let done = sync_issues(&dir, &mut an, &[]).unwrap();
        assert_eq!((done.pulled, an.topics[0].status.as_str(), an.topics[0].comments.len()), (1, "Closed", 2));
        // nothing new: nothing happens
        assert!(sync_issues(&dir, &mut an, &[]).unwrap().pushed <= 1);
        let again = sync_issues(&dir, &mut bo, &[]).unwrap();
        assert_eq!(bo.topics.iter().find(|t| t.guid == guid).unwrap().comments.len(), 2, "{again:?}");
        // an removes the other: it goes at bo's too, and does not come back
        let other = an.topics[1].guid.clone();
        an.topics.retain(|t| t.guid != other);
        sync_issues(&dir, &mut an, std::slice::from_ref(&other)).unwrap();
        assert_eq!(sync_issues(&dir, &mut bo, &[]).unwrap().removed, 1);
        assert_eq!((an.topics.len(), bo.topics.len()), (1, 1));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn decisions_on_clashes_go_both_ways() {
        let dir = folder("decisions");
        let (mut an_known, mut bo_known) = (HashMap::new(), HashMap::new());
        let key = |k: &str, d: &str| (k.to_string(), d.to_string());
        // an accepts the first
        let mut done = Synced::default();
        let take = sync_decisions(&dir, &[key("a|b", "accepted"), key("c|d", "open")], &mut an_known, "an", "2026-11-14T09:00:00Z", &mut done).unwrap();
        assert!(take.is_empty() && done.decisions_pushed == 1);
        // bo has both open: takes the first over, and rejects the second
        let mut done = Synced::default();
        let take = sync_decisions(&dir, &[key("a|b", "open"), key("c|d", "open")], &mut bo_known, "bo", "2026-11-14T09:10:00Z", &mut done).unwrap();
        assert_eq!(take, [key("a|b", "accepted")]);
        let mut done = Synced::default();
        sync_decisions(&dir, &[key("a|b", "accepted"), key("c|d", "rejected")], &mut bo_known, "bo", "2026-11-14T09:20:00Z", &mut done).unwrap();
        assert_eq!(done.decisions_pushed, 1);
        // an takes the second over
        let mut done = Synced::default();
        let take = sync_decisions(&dir, &[key("a|b", "accepted"), key("c|d", "open")], &mut an_known, "an", "2026-11-14T09:30:00Z", &mut done).unwrap();
        assert_eq!((take, done.decisions_pushed), (vec![key("c|d", "rejected")], 0));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
