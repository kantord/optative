mod common;
use common::{Log, Spec};
use optative::{OptativeJsonSet, Reconcile};
use std::sync::{Arc, Mutex};

fn temp_file(name: &str) -> std::path::PathBuf {
    let mut path = std::env::temp_dir();
    path.push(format!(
        "optative-json-set-test-{name}-{}.jsonl",
        std::process::id()
    ));
    let _ = std::fs::remove_file(&path);
    path
}

#[test]
fn missing_file_starts_empty_and_creates_it_on_reconcile() {
    let path = temp_file("missing");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
    set.reconcile(
        vec![Spec {
            id: "a".to_string(),
            value: 1,
        }],
        &mut log,
        &mut (),
    );

    assert!(log.lock().unwrap().contains(&("enter", "a".to_string())));
    assert!(path.exists());
    let _ = std::fs::remove_file(&path);
}

#[test]
fn state_survives_a_reload_and_skips_enter_for_known_keys() {
    let path = temp_file("reload");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    {
        let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
        set.reconcile(
            vec![Spec {
                id: "dispatched".to_string(),
                value: 1,
            }],
            &mut log,
            &mut (),
        );
    }

    // Simulate a process restart: fresh OptativeJsonSet, same file.
    log = Arc::new(Mutex::new(Vec::new()));
    let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
    assert_eq!(set.get(&"dispatched".to_string()), Some(&1));

    set.reconcile(
        vec![Spec {
            id: "dispatched".to_string(),
            value: 2,
        }],
        &mut log,
        &mut (),
    );

    let events = log.lock().unwrap().clone();
    assert!(
        !events.contains(&("enter", "dispatched".to_string())),
        "enter must not re-fire for a key already known from the file: {events:?}"
    );
    assert!(events.contains(&("reconcile_self", "dispatched".to_string())));
    assert_eq!(set.get(&"dispatched".to_string()), Some(&2));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn unchanged_value_survives_a_reload_without_a_spurious_enter() {
    let path = temp_file("unchanged");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    {
        let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
        set.reconcile(
            vec![Spec {
                id: "steady".to_string(),
                value: 7,
            }],
            &mut log,
            &mut (),
        );
    }

    // Same restart, but the desired value is unchanged this time.
    log = Arc::new(Mutex::new(Vec::new()));
    let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
    set.reconcile(
        vec![Spec {
            id: "steady".to_string(),
            value: 7,
        }],
        &mut log,
        &mut (),
    );

    let events = log.lock().unwrap().clone();
    assert!(
        !events.contains(&("enter", "steady".to_string())),
        "enter must not re-fire for an unchanged key across a restart: {events:?}"
    );
    assert_eq!(set.get(&"steady".to_string()), Some(&7));

    let _ = std::fs::remove_file(&path);
}

#[test]
fn removed_key_calls_exit_and_is_dropped_from_the_file() {
    let path = temp_file("removed");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    {
        let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
        set.reconcile(
            vec![Spec {
                id: "gone".to_string(),
                value: 5,
            }],
            &mut log,
            &mut (),
        );
        set.reconcile(vec![], &mut log, &mut ());
    }

    let events = log.lock().unwrap().clone();
    assert!(events.contains(&("exit", "5".to_string())));

    let set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
    assert_eq!(set.get(&"gone".to_string()), None);

    let _ = std::fs::remove_file(&path);
}

#[test]
fn open_removes_a_stale_new_file_left_by_a_crashed_write() {
    let path = temp_file("crashed-write");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    {
        let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();
        set.reconcile(
            vec![Spec {
                id: "a".to_string(),
                value: 1,
            }],
            &mut log,
            &mut (),
        );
    }

    // Simulate a process that wrote its replacement file but crashed before
    // the rename that would have made it real.
    let mut stale = path.clone().into_os_string();
    stale.push(".new.999999");
    let stale = std::path::PathBuf::from(stale);
    std::fs::write(&stale, "garbage").unwrap();

    let set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();

    assert!(!stale.exists(), "stale .new file must be cleared on open");
    assert_eq!(set.get(&"a".to_string()), Some(&1));

    let _ = std::fs::remove_file(&path);
}

#[test]
#[cfg(unix)]
fn persist_failure_is_reported_not_panicked() {
    use std::os::unix::fs::PermissionsExt;

    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "optative-json-set-test-write-denied-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir(&dir).unwrap();
    let path = dir.join("state.jsonl");
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut set: OptativeJsonSet<Spec> = OptativeJsonSet::open(&path).unwrap();

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).unwrap();

    set.reconcile(
        vec![Spec {
            id: "a".to_string(),
            value: 1,
        }],
        &mut log,
        &mut (),
    );

    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o755)).unwrap();

    assert!(
        set.persist_error().is_some(),
        "write failure must not panic"
    );
    assert_eq!(set.get(&"a".to_string()), Some(&1), "state stays in memory");

    let _ = std::fs::remove_dir_all(&dir);
}
