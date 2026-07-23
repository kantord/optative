//! Shared scaffolding used by the tutorial-style integration tests.
//!
//! Defines:
//! - the `Greeting` resource and its `Lifecycle` impl,
//! - the `Api` REST client the lifecycle delegates to,
//! - a tiny in-process HTTP server (`spawn_greetings_server`) that mimics the
//!   real API so tests can run without a network,
//! - `Spec`/`Log`, a minimal `Lifecycle` impl that just logs which hook fired,
//!   for tests that care about enter/reconcile_self/exit call sequencing
//!   rather than a realistic backing resource.

#![allow(dead_code)] // each test only uses a subset of this module

use optative::Lifecycle;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::thread;

pub type Log = Arc<Mutex<Vec<(&'static str, String)>>>;

#[derive(Clone)]
pub struct Spec {
    pub id: String,
    pub value: i32,
}

impl Lifecycle for Spec {
    type Key = String;
    type State = i32;
    type Context = Log;
    type Output = ();
    type Error = std::convert::Infallible;

    fn key(&self) -> String {
        self.id.clone()
    }

    fn enter(self, log: &mut Log, _: &mut ()) -> Result<i32, Self::Error> {
        log.lock().unwrap().push(("enter", self.id));
        Ok(self.value)
    }

    fn reconcile_self(self, state: &mut i32, log: &mut Log, _: &mut ()) -> Result<(), Self::Error> {
        log.lock().unwrap().push(("reconcile_self", self.id));
        *state = self.value;
        Ok(())
    }

    fn exit(state: i32, log: &mut Log, _: &mut ()) -> Result<(), Self::Error> {
        log.lock().unwrap().push(("exit", state.to_string()));
        Ok(())
    }
}

#[derive(Serialize, Deserialize)]
pub struct Greeting {
    pub person: String,
    pub message: String,
}

impl Lifecycle for Greeting {
    type Key = String;
    type State = Greeting;
    type Context = Api;
    type Output = ();
    type Error = ureq::Error;

    fn key(&self) -> String {
        self.person.clone()
    }

    fn enter(self, api: &mut Api, _: &mut ()) -> Result<Greeting, Self::Error> {
        api.create(&self)?;
        Ok(self)
    }

    fn reconcile_self(
        self,
        state: &mut Greeting,
        api: &mut Api,
        _: &mut (),
    ) -> Result<(), Self::Error> {
        if state.message != self.message {
            api.update(&self)?;
            *state = self;
        }
        Ok(())
    }

    fn exit(state: Greeting, api: &mut Api, _: &mut ()) -> Result<(), Self::Error> {
        api.remove(&state)
    }
}

/// REST client the lifecycle delegates to. One method per HTTP verb.
pub struct Api {
    pub base_url: String,
}

impl Api {
    pub fn create(&self, g: &Greeting) -> Result<(), ureq::Error> {
        ureq::post(&format!("{}/greetings/{}", self.base_url, g.person)).send(&g.message)?;
        Ok(())
    }

    pub fn update(&self, g: &Greeting) -> Result<(), ureq::Error> {
        ureq::put(&format!("{}/greetings/{}", self.base_url, g.person)).send(&g.message)?;
        Ok(())
    }

    pub fn remove(&self, g: &Greeting) -> Result<(), ureq::Error> {
        ureq::delete(&format!("{}/greetings/{}", self.base_url, g.person)).call()?;
        Ok(())
    }
}

pub type ServerStore = Arc<Mutex<HashMap<String, String>>>;

/// Spawn an in-process HTTP server that stores one greeting per person.
/// Enforces strict REST semantics: POST conflicts on existing keys, PUT 404s
/// on missing keys. That way the tests would catch any optative invariant
/// violation (e.g. enter called for an already-entered item).
pub fn spawn_greetings_server() -> (String, ServerStore) {
    let store: ServerStore = Arc::new(Mutex::new(HashMap::new()));
    let store_for_thread = store.clone();
    let server = tiny_http::Server::http("127.0.0.1:0").unwrap();
    let addr = server.server_addr().to_ip().unwrap();
    let base_url = format!("http://{addr}");
    thread::spawn(move || {
        for mut request in server.incoming_requests() {
            let url = request.url().to_string();
            let Some(name) = url.strip_prefix("/greetings/").map(str::to_string) else {
                let _ = request.respond(tiny_http::Response::empty(404));
                continue;
            };
            match request.method() {
                tiny_http::Method::Post => {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).unwrap();
                    let mut s = store_for_thread.lock().unwrap();
                    use std::collections::hash_map::Entry;
                    let resp = match s.entry(name) {
                        Entry::Vacant(e) => {
                            e.insert(body);
                            201
                        }
                        Entry::Occupied(_) => 409,
                    };
                    let _ = request.respond(tiny_http::Response::empty(resp));
                }
                tiny_http::Method::Put => {
                    let mut body = String::new();
                    request.as_reader().read_to_string(&mut body).unwrap();
                    let mut s = store_for_thread.lock().unwrap();
                    use std::collections::hash_map::Entry;
                    let resp = match s.entry(name) {
                        Entry::Occupied(mut e) => {
                            e.insert(body);
                            204
                        }
                        Entry::Vacant(_) => 404,
                    };
                    let _ = request.respond(tiny_http::Response::empty(resp));
                }
                tiny_http::Method::Delete => {
                    let mut s = store_for_thread.lock().unwrap();
                    if s.remove(&name).is_some() {
                        let _ = request.respond(tiny_http::Response::empty(204));
                    } else {
                        let _ = request.respond(tiny_http::Response::empty(404));
                    }
                }
                _ => {
                    let _ = request.respond(tiny_http::Response::empty(405));
                }
            }
        }
    });
    (base_url, store)
}
