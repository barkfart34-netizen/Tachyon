//! Tachyon's Chronicles data pack in `tachyon/content/chronicles/data`:
//! its files, read the way Horizon hands a reload listener its folder
//! (`<namespace>:<path>` and the parsed JSON, sorted by id), must make a
//! pack without a single problem and be completable end-to-end.

use crate::hq_logic::Book;
use crate::hq_model::{Progress, TaskKind};
use crate::hq_progress::{Actor, Effect, QuestEvent, apply, wanted_items};
use crate::hq_validate::{DATA_FOLDER, load_data};
use std::path::{Path, PathBuf};

/// Every `.json` file under `dir`, as its path below it with `/`.
fn files(dir: &Path, below: &str, out: &mut Vec<(String, PathBuf)>) {
    for entry in std::fs::read_dir(dir).unwrap() {
        let path = entry.unwrap().path();
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let rel = if below.is_empty() { name } else { format!("{below}/{name}") };
        if path.is_dir() {
            files(&path, &rel, out);
        } else if let Some(stem) = rel.strip_suffix(".json") {
            out.push((stem.to_owned(), path));
        }
    }
}

#[test]
fn chronicles_data_pack_loads_and_completes_cleanly() {
    let data = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tachyon/content/chronicles/data");
    let mut read = Vec::new();
    for entry in std::fs::read_dir(&data).unwrap() {
        let namespace_dir = entry.unwrap().path();
        let namespace = namespace_dir.file_name().unwrap().to_string_lossy().into_owned();
        let folder = namespace_dir.join(DATA_FOLDER);
        if !folder.is_dir() {
            continue;
        }
        let mut found = Vec::new();
        files(&folder, "", &mut found);
        for (path, file) in found {
            let json: serde_json::Value = serde_json::from_str(&std::fs::read_to_string(file).unwrap()).unwrap();
            read.push((format!("{namespace}:{path}"), json));
        }
    }
    read.sort_by(|a, b| a.0.cmp(&b.0));
    assert_eq!(read.len(), 4, "{read:?}");

    let loaded = load_data(&read).expect("Chronicles data pack holds quests");
    let problems: Vec<String> = loaded.problems.iter().map(|p| p.to_string()).collect();
    assert!(problems.is_empty(), "{problems:#?}");
    assert_eq!(loaded.file.chapters.len(), 3);
    let total_quests: usize = loaded.file.chapters.iter().map(|c| c.quests.len()).sum();
    assert_eq!(total_quests, 20);

    // Every Chronicles quest (including kinetic_speed and structure tasks) completes cleanly.
    let book = Book::new(loaded.file);
    let mut progress = Progress::default();
    let mut effects = Vec::new();
    let mut now = 0;
    for &n in book.order() {
        for (t, task) in book.quest(n).tasks.iter().enumerate() {
            now += 1;
            let id = book.task_id(n, t);
            let event = match &task.kind {
                TaskKind::Checkmark => QuestEvent::Check { task: id },
                TaskKind::Item { consume: false, .. } => {
                    assert!(wanted_items(&book, &progress).iter().any(|w| w.task == id), "{id}");
                    QuestEvent::ItemCount { task: id, count: task.kind.goal() }
                }
                TaskKind::KineticSpeed { rpm } => QuestEvent::KineticSpeed { rpm: *rpm },
                TaskKind::Structure { structure } => QuestEvent::Structure { structure: structure.clone() },
                other => panic!("unexpected Chronicles task kind: {other:?}"),
            };
            effects.extend(apply(&book, &mut progress, &Actor::new("steve", now), &event).effects);
        }
        assert!(progress.is_completed(&book.nodes()[n].id), "{} did not complete", book.nodes()[n].id);
    }
    now += 1;
    effects.extend(apply(&book, &mut progress, &Actor::new("steve", now), &QuestEvent::ClaimAll).effects);
    assert!(!effects.iter().any(|e| matches!(e, Effect::Refused(_))), "{effects:?}");
    for chapter in ["machine_age", "metals", "lost_realms"] {
        assert!(effects.contains(&Effect::ChapterCompleted { chapter: chapter.into() }), "{chapter}");
    }
}
