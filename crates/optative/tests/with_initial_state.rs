mod common;
use common::{Log, Spec};
use optative::{OptativeSet, Reconcile};
use std::sync::{Arc, Mutex};

#[test]
fn seeded_items_never_fire_enter() {
    let mut log: Log = Arc::new(Mutex::new(Vec::new()));

    let mut set: OptativeSet<Spec> = OptativeSet::with_initial_state(vec![
        ("seeded_kept".to_string(), 1),
        ("seeded_removed".to_string(), 2),
    ]);

    set.reconcile(
        vec![
            Spec {
                id: "seeded_kept".to_string(),
                value: 10,
            },
            Spec {
                id: "fresh".to_string(),
                value: 20,
            },
        ],
        &mut log,
        &mut (),
    );

    let events = log.lock().unwrap().clone();
    let has = |name: &str, key: &str| events.iter().any(|(n, k)| *n == name && k == key);

    assert!(has("reconcile_self", "seeded_kept"), "{events:?}");
    assert!(has("exit", "2"), "{events:?}");
    assert!(has("enter", "fresh"), "{events:?}");

    for (name, key) in &events {
        assert!(
            !(*name == "enter" && (key == "seeded_kept" || key == "seeded_removed")),
            "enter fired for seeded key: ({name}, {key}) in {events:?}"
        );
    }

    assert_eq!(set.get(&"seeded_kept".to_string()), Some(&10));
    assert_eq!(set.get(&"fresh".to_string()), Some(&20));
    assert_eq!(set.get(&"seeded_removed".to_string()), None);
}
