//! Mirrors the "Using OptativeSet" tutorial in README.md.

mod common;
use common::{Api, Greeting, spawn_greetings_server};
use optative::{OptativeSet, Reconcile};

#[test]
fn managed_set_tutorial_steps() {
    let (base_url, server) = spawn_greetings_server();
    let mut api = Api { base_url };
    let mut store: OptativeSet<Greeting> = OptativeSet::new();

    // Step 4 — initial desired set.
    store.reconcile(
        vec![
            Greeting {
                person: "ada".into(),
                message: "hello, ada".into(),
            },
            Greeting {
                person: "grace".into(),
                message: "welcome, grace".into(),
            },
        ],
        &mut api,
        &mut (),
    );
    {
        let s = server.lock().unwrap();
        assert_eq!(s.get("ada"), Some(&"hello, ada".to_string()));
        assert_eq!(s.get("grace"), Some(&"welcome, grace".to_string()));
    }

    // Step 5 — change of mind: ada's message changes, grace is dropped.
    store.reconcile(
        vec![Greeting {
            person: "ada".into(),
            message: "good morning, ada".into(),
        }],
        &mut api,
        &mut (),
    );
    {
        let s = server.lock().unwrap();
        assert_eq!(s.get("ada"), Some(&"good morning, ada".to_string()));
        assert!(!s.contains_key("grace"));
    }
}
