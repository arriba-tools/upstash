//! The Upstash Vector index that `vector remember` stores notes in, `vector recall` searches and
//! `vector forget` deletes from.

use serde_json::{Value, json};
use std::collections::BTreeSet;
use std::time::Duration;
use ureq::RequestBuilder;
use ureq::typestate::WithBody;

use crate::cli::{ForgetArgs, Mode, RecallArgs, RememberArgs, VectorCommand};
use crate::common::{self, Io, Stop, now_ms, say};

/// The environment variables that hold the index's REST URL and token, named as the index's
/// Connect section in the Upstash Console names them.
pub const URL_VAR: &str = "UPSTASH_VECTOR_REST_URL";
pub const TOKEN_VAR: &str = "UPSTASH_VECTOR_REST_TOKEN";

/// Sends one request to the index and returns its result.
pub trait Index {
    fn post(&mut self, path: &str, body: &Value) -> Result<Value, Stop>;
    fn delete(&mut self, path: &str, body: &Value) -> Result<Value, Stop>;
}

/// An Upstash Vector index with a hosted embedding model, reached over its REST API.
pub struct Upstash {
    base: String,
    token: String,
    agent: ureq::Agent,
}

impl Upstash {
    pub fn open(url: &str, token: &str) -> Result<Self, Stop> {
        let base = base_url(url)?;
        let agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(30)))
            // An error status still carries the index's reason in its body.
            .http_status_as_error(false)
            .build()
            .into();
        let token = token.to_string();
        Ok(Self { base, token, agent })
    }

    /// Send `body` on `request` with the token, and read the index's answer.
    fn send(
        &self,
        path: &str,
        request: RequestBuilder<WithBody>,
        body: &Value,
    ) -> Result<Value, Stop> {
        let mut response = request
            .header("Authorization", format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| failure(path, e))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| failure(path, e))?;
        answer(path, status, &text)
    }
}

impl Index for Upstash {
    fn post(&mut self, path: &str, body: &Value) -> Result<Value, Stop> {
        let request = self.agent.post(format!("{}/{path}", self.base));
        self.send(path, request, body)
    }

    // The delete endpoint takes its IDs in the body of a DELETE.
    fn delete(&mut self, path: &str, body: &Value) -> Result<Value, Stop> {
        let request = self.agent.delete(format!("{}/{path}", self.base));
        self.send(path, request.force_send_body(), body)
    }
}

/// A failed request as unreachable when the index could not be reached, and as failed otherwise.
fn failure(path: &str, e: ureq::Error) -> Stop {
    use ureq::Error as E;
    let message = format!("Upstash Vector failed {path}: {e}");
    match e {
        E::Io(_) | E::Timeout(_) | E::HostNotFound | E::ConnectionFailed => {
            Stop::Unreachable(message)
        }
        _ => Stop::Failed(message),
    }
}

/// The result of an answer from the index, or the reason it gives for refusing.
fn answer(path: &str, status: u16, text: &str) -> Result<Value, Stop> {
    let refused = |why: &str| Stop::Failed(format!("Upstash Vector refused {path}: {why}"));
    let value: Value = serde_json::from_str(text)
        .map_err(|_| refused(&format!("HTTP {status} {}", text.trim())))?;
    match value.get("error").and_then(Value::as_str) {
        Some(why) => Err(refused(why)),
        None if status >= 400 => Err(refused(&format!("HTTP {status}"))),
        None => Ok(value.get("result").cloned().unwrap_or(Value::Null)),
    }
}

/// The https origin of `url`, an index's REST URL.
fn base_url(url: &str) -> Result<String, Stop> {
    url.strip_prefix("https://")
        .and_then(|rest| rest.split(['/', '?']).next())
        .filter(|host| !host.is_empty())
        .map(|host| format!("https://{host}"))
        .ok_or_else(|| Stop::Failed(format!("{URL_VAR} is not an https:// URL")))
}

/// The index for this run, from the environment variables UPSTASH_VECTOR_REST_URL and
/// UPSTASH_VECTOR_REST_TOKEN.
pub fn connect() -> Result<Upstash, Stop> {
    Upstash::open(&common::env(URL_VAR)?, &common::env(TOKEN_VAR)?)
}

/// Run a command that reads or writes the index.
pub fn run(index: &mut dyn Index, command: &VectorCommand, io: &mut Io) -> Result<i32, Stop> {
    match command {
        VectorCommand::Remember(a) => remember(index, a, io),
        VectorCommand::Recall(a) => recall(index, a, io),
        VectorCommand::Forget(a) => forget(index, a, io),
    }
}

/// Store a note in the namespace, and print its ID.
pub fn remember(index: &mut dyn Index, a: &RememberArgs, io: &mut Io) -> Result<i32, Stop> {
    let note = common::text(a.file.as_deref(), a.text.as_deref())?;
    if note.trim().is_empty() {
        return Err(Stop::Failed("The note is empty".into()));
    }
    let namespace = common::encode(&a.namespace);
    let at = now_ms();
    let id = match &a.replace {
        // An upsert under a stored ID overwrites that note. A missing ID would add a note instead.
        Some(old) => {
            let found = index.post(&format!("fetch/{namespace}"), &json!({ "ids": [old] }))?;
            if found.get(0).is_none_or(Value::is_null) {
                return Err(Stop::Failed(format!("No note {old} in {}", a.namespace)));
            }
            old.clone()
        }
        // The process ID keeps apart the notes one name stores in the same millisecond.
        None => format!("{at}-{}-{}", a.name, std::process::id()),
    };
    let body = json!({ "id": id, "data": note, "metadata": { "from": a.name, "at": at } });
    index.post(&format!("upsert-data/{namespace}"), &body)?;
    say!(io.out, "{id}");
    Ok(0)
}

/// Print the namespace's notes that best match the query, each as a header, a blank line, then its
/// text, or with --json as one array.
pub fn recall(index: &mut dyn Index, a: &RecallArgs, io: &mut Io) -> Result<i32, Stop> {
    let mode = match a.mode {
        Mode::Hybrid => "HYBRID",
        Mode::Dense => "DENSE",
        Mode::Sparse => "SPARSE",
    };
    let body = json!({
        "data": a.query,
        "topK": a.count,
        "includeMetadata": true,
        "includeData": true,
        // BM25 weighs each word by its rarity, so a figure outweighs words such as "at" and "D".
        "weightingStrategy": "IDF",
        "queryMode": mode,
    });
    let path = format!("query-data/{}", common::encode(&a.namespace));
    let result = index.post(&path, &body)?;
    let hits = result.as_array().map(Vec::as_slice).unwrap_or_default();
    let notes: Vec<Value> = hits
        .iter()
        .map(|hit| {
            let text = |pointer: &str| hit.pointer(pointer).and_then(Value::as_str);
            json!({
                "id": text("/id").unwrap_or_default(),
                "from": text("/metadata/from").unwrap_or_default(),
                "at": hit.pointer("/metadata/at").and_then(Value::as_u64),
                "score": hit.get("score").and_then(Value::as_f64).unwrap_or_default(),
                "text": text("/data").unwrap_or_default().trim_end(),
            })
        })
        .collect();
    if a.json {
        say!(io.out, "{}", Value::Array(notes));
        return Ok(0);
    }
    if notes.is_empty() {
        say!(io.err, "No note in {} matches", a.namespace);
    }
    for note in &notes {
        let text = |key: &str| note[key].as_str().unwrap_or_default();
        let score = note["score"].as_f64().unwrap_or_default();
        say!(
            io.out,
            "--- {}\nfrom: {}\nscore: {score:.4}\n\n{}\n",
            text("id"),
            text("from"),
            text("text")
        );
    }
    Ok(0)
}

/// Delete the notes with the given IDs from the namespace, failing when any of them names no note.
pub fn forget(index: &mut dyn Index, a: &ForgetArgs, io: &mut Io) -> Result<i32, Stop> {
    let ids: BTreeSet<&str> = a.ids.iter().map(String::as_str).collect();
    let path = format!("delete/{}", common::encode(&a.namespace));
    let result = index.delete(&path, &json!({ "ids": ids }))?;
    let deleted = result
        .get("deleted")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let asked = ids.len() as u64;
    if deleted < asked {
        return Err(Stop::Failed(format!(
            "Deleted {deleted} of {asked} notes, since the rest name no note in {}",
            a.namespace
        )));
    }
    say!(
        io.out,
        "Deleted {deleted} {}",
        if deleted == 1 { "note" } else { "notes" }
    );
    Ok(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tests::invoke_index;

    /// An index that records each request and answers a fetch with `fetched`, every other one with
    /// `reply`.
    struct Recorder {
        requests: Vec<(String, Value)>,
        methods: Vec<&'static str>,
        reply: Result<Value, Stop>,
        fetched: Value,
    }

    impl Recorder {
        fn answering(reply: Result<Value, Stop>) -> Self {
            Self {
                requests: Vec::new(),
                methods: Vec::new(),
                reply,
                fetched: json!([null]),
            }
        }
    }

    impl Index for Recorder {
        fn post(&mut self, path: &str, body: &Value) -> Result<Value, Stop> {
            self.requests.push((path.to_string(), body.clone()));
            self.methods.push("POST");
            if path.starts_with("fetch/") {
                return Ok(self.fetched.clone());
            }
            self.reply.clone()
        }

        fn delete(&mut self, path: &str, body: &Value) -> Result<Value, Stop> {
            self.requests.push((path.to_string(), body.clone()));
            self.methods.push("DELETE");
            self.reply.clone()
        }
    }

    #[test]
    fn forget_deletes_the_notes_and_says_how_many() {
        let mut index = Recorder::answering(Ok(json!({ "deleted": 2 })));
        let args = ["vector", "forget", "a/b", "2-mac-9", "1-win2-7", "2-mac-9"];
        let (code, out, _) = invoke_index(&mut index, &args);
        assert_eq!((code, out.trim()), (0, "Deleted 2 notes"));
        assert_eq!(index.methods, ["DELETE"]);
        // A repeated ID is sent once, so the count compares with the distinct IDs.
        assert_eq!(
            index.requests[0],
            (
                "delete/a%2Fb".to_string(),
                json!({ "ids": ["1-win2-7", "2-mac-9"] })
            )
        );
    }

    #[test]
    fn forget_fails_when_an_id_names_no_note() {
        let mut index = Recorder::answering(Ok(json!({ "deleted": 1 })));
        let args = ["vector", "forget", "c", "1-win2-7", "1-nobody-1"];
        let (code, out, err) = invoke_index(&mut index, &args);
        assert_eq!((code, out.as_str()), (1, ""));
        assert_eq!(
            err.trim(),
            "Deleted 1 of 2 notes, since the rest name no note in c"
        );
    }

    #[test]
    fn forget_needs_an_id() {
        let mut index = Recorder::answering(Ok(json!({ "deleted": 0 })));
        assert_eq!(invoke_index(&mut index, &["vector", "forget", "c"]).0, 2);
        assert!(index.requests.is_empty());
    }

    #[test]
    fn remember_stores_the_note_in_the_namespace_and_prints_its_id() {
        let mut index = Recorder::answering(Ok(json!("Success")));
        let args = [
            "vector",
            "remember",
            "win2",
            "a/b",
            "--text",
            "cross at D 236.6",
        ];
        let (code, out, _) = invoke_index(&mut index, &args);
        let (path, body) = &index.requests[0];
        assert_eq!(code, 0);
        // A namespace is one path segment, however it is spelled.
        assert_eq!(path, "upsert-data/a%2Fb");
        assert_eq!(body["data"], "cross at D 236.6");
        assert_eq!(body["metadata"]["from"], "win2");
        assert_eq!(out.trim(), body["id"]);
        assert!(out.contains("-win2-"), "{out}");
    }

    #[test]
    fn remember_needs_a_note() {
        let mut index = Recorder::answering(Ok(json!("Success")));
        assert_eq!(
            invoke_index(&mut index, &["vector", "remember", "a", "c"]).0,
            2
        );
        let args = ["vector", "remember", "a", "c", "--text", " "];
        let (code, _, err) = invoke_index(&mut index, &args);
        assert_eq!((code, err.trim()), (1, "The note is empty"));
        assert!(index.requests.is_empty());
    }

    #[test]
    fn remember_replace_overwrites_the_stored_note_under_its_id() {
        let mut index = Recorder::answering(Ok(json!("Success")));
        index.fetched = json!([{ "id": "1-win2-7" }]);
        let args = [
            "vector",
            "remember",
            "mac",
            "c",
            "--replace",
            "1-win2-7",
            "--text",
            "cross at D 236.7",
        ];
        let (code, out, _) = invoke_index(&mut index, &args);
        assert_eq!(code, 0);
        assert_eq!(
            index.requests[0],
            ("fetch/c".to_string(), json!({ "ids": ["1-win2-7"] }))
        );
        let (path, body) = &index.requests[1];
        assert_eq!(path, "upsert-data/c");
        assert_eq!(
            (&body["id"], &body["data"]),
            (&json!("1-win2-7"), &json!("cross at D 236.7"))
        );
        assert_eq!(body["metadata"]["from"], "mac");
        assert_eq!(out.trim(), "1-win2-7");
    }

    #[test]
    fn remember_replace_refuses_an_id_with_no_note() {
        let mut index = Recorder::answering(Ok(json!("Success")));
        let args = [
            "vector",
            "remember",
            "mac",
            "c",
            "--replace",
            "1-win2-8",
            "--text",
            "x",
        ];
        let (code, _, err) = invoke_index(&mut index, &args);
        assert_eq!((code, err.trim()), (1, "No note 1-win2-8 in c"));
        assert_eq!(index.requests.len(), 1);
    }

    #[test]
    fn recall_prints_the_best_matches_first() {
        let hits = json!([
            { "id": "2-win2", "score": 0.0328, "metadata": { "from": "win2" },
              "data": "cross at D 236.6\n" },
            { "id": "1-mac", "score": 0.0161, "metadata": { "from": "mac" }, "data": "d6 at D 241.13" },
        ]);
        let mut index = Recorder::answering(Ok(hits));
        let args = ["vector", "recall", "c", "D 236.6", "--count", "2"];
        let (code, out, _) = invoke_index(&mut index, &args);
        let (path, body) = &index.requests[0];
        assert_eq!(code, 0);
        assert_eq!(path, "query-data/c");
        assert_eq!(
            (body["topK"].as_u64(), body["includeData"].as_bool()),
            (Some(2), Some(true))
        );
        assert_eq!(body["weightingStrategy"], "IDF");
        assert_eq!(
            out,
            "--- 2-win2\nfrom: win2\nscore: 0.0328\n\ncross at D 236.6\n\n\
             --- 1-mac\nfrom: mac\nscore: 0.0161\n\nd6 at D 241.13\n\n"
        );
    }

    #[test]
    fn recall_mode_picks_the_part_of_the_index_to_search() {
        for (mode, sent) in [
            (None, "HYBRID"),
            (Some("dense"), "DENSE"),
            (Some("sparse"), "SPARSE"),
        ] {
            let mut index = Recorder::answering(Ok(json!([])));
            let mut args = vec!["vector", "recall", "c", "q"];
            args.extend(mode.map(|m| ["--mode", m]).into_iter().flatten());
            assert_eq!(invoke_index(&mut index, &args).0, 0, "{args:?}");
            assert_eq!(index.requests[0].1["queryMode"], sent, "{args:?}");
        }
    }

    #[test]
    fn recall_refuses_an_unknown_mode() {
        let mut index = Recorder::answering(Ok(json!([])));
        let args = ["vector", "recall", "c", "q", "--mode", "rrf"];
        assert_eq!(invoke_index(&mut index, &args).0, 2);
        assert!(index.requests.is_empty());
    }

    #[test]
    fn recall_json_prints_the_notes_as_one_array() {
        let hits = json!([
            { "id": "2-win2", "score": 0.8812, "metadata": { "from": "win2", "at": 2 },
              "data": "cross at D 236.6\n" },
            { "id": "1-mac", "score": 0.7905, "data": "d6 at D 241.13" },
        ]);
        let mut index = Recorder::answering(Ok(hits));
        let args = [
            "vector", "recall", "c", "D 236.6", "--mode", "dense", "--json",
        ];
        let (code, out, err) = invoke_index(&mut index, &args);
        assert_eq!((code, err.as_str()), (0, ""));
        // One line, so a script reads the whole answer at once.
        assert_eq!(out.lines().count(), 1);
        let notes: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(
            notes,
            json!([
                { "id": "2-win2", "from": "win2", "at": 2, "score": 0.8812,
                  "text": "cross at D 236.6" },
                // A note without metadata keeps every field.
                { "id": "1-mac", "from": "", "at": null, "score": 0.7905, "text": "d6 at D 241.13" },
            ])
        );
    }

    #[test]
    fn recall_json_prints_an_empty_array_when_nothing_matches() {
        let mut index = Recorder::answering(Ok(json!([])));
        let (code, out, err) = invoke_index(&mut index, &["vector", "recall", "c", "q", "--json"]);
        assert_eq!((code, out.trim(), err.as_str()), (0, "[]", ""));
    }

    #[test]
    fn recall_takes_a_query_that_starts_with_a_dash() {
        let mut index = Recorder::answering(Ok(json!([])));
        let (code, _, _) = invoke_index(&mut index, &["vector", "recall", "c", "--a_grow 1.25"]);
        assert_eq!(code, 0);
        assert_eq!(index.requests[0].1["data"], "--a_grow 1.25");
    }

    #[test]
    fn recall_says_when_nothing_matches() {
        let mut index = Recorder::answering(Ok(json!([])));
        let (code, out, err) = invoke_index(&mut index, &["vector", "recall", "c", "anything"]);
        assert_eq!((code, out.as_str()), (0, ""));
        assert_eq!(err.trim(), "No note in c matches");
    }

    #[test]
    fn a_refusal_stops_the_run_with_the_index_reason() {
        let refused = Stop::Failed("Upstash Vector refused query-data/c: no model".into());
        let mut index = Recorder::answering(Err(refused));
        let (code, _, err) = invoke_index(&mut index, &["vector", "recall", "c", "q"]);
        assert_eq!(
            (code, err.trim()),
            (1, "Upstash Vector refused query-data/c: no model")
        );
    }

    #[test]
    fn answer_reads_the_result_or_the_reason_for_refusing() {
        assert_eq!(
            answer("p", 200, r#"{"result":"Success"}"#),
            Ok(json!("Success"))
        );
        let refused = |status, text| match answer("p", status, text) {
            Err(Stop::Failed(m)) => m,
            other => panic!("{other:?}"),
        };
        assert_eq!(
            refused(
                400,
                r#"{"error":"Embedding data is not allowed","status":400}"#
            ),
            "Upstash Vector refused p: Embedding data is not allowed"
        );
        assert_eq!(refused(401, "{}"), "Upstash Vector refused p: HTTP 401");
        assert_eq!(
            refused(502, "Bad Gateway\n"),
            "Upstash Vector refused p: HTTP 502 Bad Gateway"
        );
    }

    #[test]
    fn base_url_is_the_https_origin() {
        for url in [
            "https://x-vector.upstash.io",
            "https://x-vector.upstash.io/",
            "https://x-vector.upstash.io/query?y=1",
        ] {
            assert_eq!(
                base_url(url).unwrap(),
                "https://x-vector.upstash.io",
                "{url}"
            );
        }
        for bad in ["", "http://x-vector.upstash.io", "https://", "https:///p"] {
            assert!(matches!(base_url(bad), Err(Stop::Failed(_))), "{bad}");
        }
    }
}
