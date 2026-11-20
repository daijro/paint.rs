//! Outside requests from Paint.NET plug-ins: a plug-in that wants the network, files, other programs, a system
//! setting or native code asks first, and is denied until the user allows it. The host stops such a
//! call it holds no grant for and fails the operation with `pdn-permission:{…}` ([`Request::from_error`]); paint.rs
//! asks, then runs the operation again with the answer. The host gets every grant through [`send`]: an allowed
//! entry lets the call through, a denied one makes it fail as it would offline, so the plug-in carries on.
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

const PREFIX: &str = "pdn-permission:";

/// What a request is for: `category` (network, files, processes, system, native) and `target` (one host,
/// folder, program or setting, as the host words it; "" for the whole category). A grant matches it exactly.
#[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct Grant {
    pub category: String,
    pub target: String,
}

impl Grant {
    /// "connect to imgur.com": what it lets a plug-in do, as a verb phrase.
    pub fn wants(&self) -> String {
        let t = self.target.as_str();
        let (with, without) = match self.category.as_str() {
            "network" => ("connect to", "connect to the network"),
            "files" => ("use files in", "use your files"),
            "processes" => ("run", "run other programs"),
            "system" => ("change", "change a system setting"),
            "native" => ("run native code from", "run native code"),
            other if t.is_empty() => return format!("use {other}"),
            _ => ("use", ""),
        };
        if t.is_empty() {
            without.into()
        } else {
            format!("{with} {t}")
        }
    }
}

/// One outside request a plug-in (its installed file name) made.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Request {
    pub plugin: String,
    pub grant: Grant,
    /// More about it, for the prompt's second line (a URL, a path, a command line); may be empty.
    pub detail: String,
    /// Why it cannot be allowed here (a browser cannot change the wallpaper); empty when it can.
    pub unavailable: String,
    /// A network request the host leaves to paint.rs (it cannot wait for one inside a render): allowed, paint.rs
    /// makes it, hands the host the response under `key` ([`response_json`]), and runs the plug-in again.
    pub http: Option<HttpRequest>,
    /// The host's name for that request.
    pub key: String,
}

/// A plug-in's HTTP request, as the host hands it over.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HttpRequest {
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

/// The response to give the host for a request: `{status, headers, body}` with the body in base64 (status 0, no
/// body, when it could not be made).
pub fn response_json(status: u16, headers: &[(String, String)], body: &[u8]) -> String {
    use base64::Engine as _;
    let headers: Map<String, Value> = headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
    let body = base64::engine::general_purpose::STANDARD.encode(body);
    json!({ "status": status, "headers": headers, "body": body }).to_string()
}

impl Request {
    /// The request an operation stopped at, from its error's text (the JSON may have text after it).
    pub fn from_error(error: &str) -> Option<Self> {
        let at = error.find(PREFIX)? + PREFIX.len();
        let value = serde_json::Deserializer::from_str(&error[at..])
            .into_iter::<Value>()
            .next()?
            .ok()?;
        let s = |key: &str| value[key].as_str().unwrap_or_default().to_owned();
        let request = Self {
            plugin: s("plugin"),
            grant: Grant {
                category: s("category"),
                target: s("target"),
            },
            detail: s("detail"),
            unavailable: s("unavailable"),
            http: value.get("request").filter(|r| r.is_object()).map(|r| {
                use base64::Engine as _;
                let s = |key: &str| r[key].as_str().unwrap_or_default().to_owned();
                let headers = r["headers"].as_object().into_iter().flatten();
                HttpRequest {
                    method: s("method"),
                    url: s("url"),
                    headers: headers
                        .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_owned()))
                        .collect(),
                    body: base64::engine::general_purpose::STANDARD
                        .decode(s("body"))
                        .unwrap_or_default(),
                }
            }),
            key: s("key"),
        };
        (!request.plugin.is_empty() && !request.grant.category.is_empty()).then_some(request)
    }

    /// "Upload.dll wants to connect to imgur.com".
    pub fn message(&self) -> String {
        format!("{} wants to {}", self.plugin, self.grant.wants())
    }
}

/// The answers so far, per plug-in.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Permissions {
    /// Always allowed, remembered across starts ([`Permissions::remembered`]).
    always: BTreeMap<String, BTreeSet<Grant>>,
    /// Allowed for the run in progress (Allow once), until [`Permissions::end_once`] for its plug-in.
    once: BTreeMap<String, BTreeSet<Grant>>,
    /// Denied until the next start: the call fails as offline, and paint.rs asks no more.
    denied: BTreeMap<String, BTreeSet<Grant>>,
}

fn has(set: &BTreeMap<String, BTreeSet<Grant>>, plugin: &str, grant: &Grant) -> bool {
    set.get(plugin).is_some_and(|s| s.contains(grant))
}

fn take(set: &mut BTreeMap<String, BTreeSet<Grant>>, plugin: &str, grant: &Grant) {
    if let Some(s) = set.get_mut(plugin) {
        s.remove(grant);
        if s.is_empty() {
            set.remove(plugin);
        }
    }
}

impl Permissions {
    /// Whether `request` already has an answer (so it needs no prompt, only the host's copy brought up to date).
    pub fn answered(&self, request: &Request) -> bool {
        let (p, g) = (&request.plugin, &request.grant);
        has(&self.always, p, g) || has(&self.once, p, g) || has(&self.denied, p, g)
    }

    /// Whether the host lets `request` through (Always allow, or Allow once for this run).
    pub fn allows(&self, request: &Request) -> bool {
        let (p, g) = (&request.plugin, &request.grant);
        has(&self.always, p, g) || has(&self.once, p, g)
    }

    pub fn allow_once(&mut self, request: &Request) {
        take(&mut self.denied, &request.plugin, &request.grant);
        let set = self.once.entry(request.plugin.clone()).or_default();
        set.insert(request.grant.clone());
    }

    pub fn allow_always(&mut self, request: &Request) {
        take(&mut self.denied, &request.plugin, &request.grant);
        take(&mut self.once, &request.plugin, &request.grant);
        let set = self.always.entry(request.plugin.clone()).or_default();
        set.insert(request.grant.clone());
    }

    pub fn deny(&mut self, request: &Request) {
        take(&mut self.once, &request.plugin, &request.grant);
        let set = self.denied.entry(request.plugin.clone()).or_default();
        set.insert(request.grant.clone());
    }

    /// Forgets an Always allow: the plug-in asks again next time.
    pub fn revoke(&mut self, plugin: &str, grant: &Grant) {
        take(&mut self.always, plugin, grant);
    }

    /// The run `plugin`'s Allow once answers were for has ended.
    pub fn end_once(&mut self, plugin: &str) {
        self.once.remove(plugin);
    }

    /// `plugin`'s Always allow answers, for its row on the Plugins page.
    pub fn always_of(&self, plugin: &str) -> Vec<Grant> {
        self.always
            .get(plugin)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default()
    }

    /// The Always allow answers as JSON (`{"Upload.dll":[{"category":…,"target":…}]}`), to remember.
    pub fn remembered(&self) -> String {
        to_json(&self.always, &BTreeMap::new())
    }

    /// Takes back the answers [`Permissions::remembered`] wrote (at start, beside any given since); anything
    /// unreadable is dropped.
    pub fn load(&mut self, json: &str) {
        let Ok(Value::Object(map)) = serde_json::from_str::<Value>(json) else {
            return;
        };
        for (plugin, grants) in map {
            for g in grants.as_array().into_iter().flatten() {
                let s = |key: &str| g[key].as_str().unwrap_or_default().to_owned();
                if !s("category").is_empty() {
                    let set = self.always.entry(plugin.clone()).or_default();
                    set.insert(Grant {
                        category: s("category"),
                        target: s("target"),
                    });
                }
            }
        }
    }

    /// Everything the host may use: the allowed entries, then the denied ones marked `"deny": true`.
    pub fn host_json(&self) -> String {
        let mut allowed = self.always.clone();
        for (plugin, set) in &self.once {
            allowed
                .entry(plugin.clone())
                .or_default()
                .extend(set.iter().cloned());
        }
        to_json(&allowed, &self.denied)
    }
}

fn to_json(
    allowed: &BTreeMap<String, BTreeSet<Grant>>,
    denied: &BTreeMap<String, BTreeSet<Grant>>,
) -> String {
    let mut map = Map::new();
    for (set, deny) in [(allowed, false), (denied, true)] {
        for (plugin, grants) in set {
            let list = map.entry(plugin.clone()).or_insert_with(|| json!([]));
            for g in grants {
                let mut entry = json!({ "category": g.category, "target": g.target });
                if deny {
                    entry["deny"] = json!(true);
                }
                list.as_array_mut().unwrap().push(entry);
            }
        }
    }
    Value::Object(map).to_string()
}

static STATE: Mutex<Option<Permissions>> = Mutex::new(None);

/// Reads or changes the answers; the host gets the new set after every call.
pub fn with<T>(f: impl FnOnce(&mut Permissions) -> T) -> T {
    let (out, json) = {
        let mut state = STATE.lock().unwrap_or_else(|e| e.into_inner());
        let state = state.get_or_insert_with(Permissions::default);
        let out = f(state);
        (out, state.host_json())
    };
    super::dotnet::set_permissions(&json);
    out
}

/// Gives the host the answers again (it has just started).
pub fn send() {
    with(|_| ());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(plugin: &str, category: &str, target: &str) -> Request {
        Request {
            plugin: plugin.into(),
            grant: Grant {
                category: category.into(),
                target: target.into(),
            },
            ..Default::default()
        }
    }

    /// The host's message carries the request; text around it (an exception's type, a stack) is ignored.
    #[test]
    fn a_request_reads_from_the_error() {
        let error = "PermissionRequiredException: pdn-permission:{\"plugin\":\"Upload.dll\",\"category\":\"network\",\
                     \"target\":\"imgur.com\",\"detail\":\"POST https://api.imgur.com/3/image\"}\n   at Upload.Render()";
        let r = Request::from_error(error).unwrap();
        assert_eq!(r.plugin, "Upload.dll");
        assert_eq!(r.grant.wants(), "connect to imgur.com");
        assert_eq!(r.detail, "POST https://api.imgur.com/3/image");
        assert_eq!(r.message(), "Upload.dll wants to connect to imgur.com");
        assert!(r.http.is_none() && r.key.is_empty());
        let fetch = "pdn-permission:{\"plugin\":\"P.dll\",\"category\":\"network\",\"target\":\"example.invalid\",\
                     \"detail\":\"http://example.invalid/a\",\"key\":\"k1\",\"request\":{\"key\":\"k1\",\"method\":\"POST\",\
                     \"url\":\"http://example.invalid/a\",\"headers\":{\"Accept\":\"*/*\"},\"body\":\"aGk=\"}}";
        let f = Request::from_error(fetch).unwrap();
        assert_eq!(f.key, "k1");
        let http = f.http.unwrap();
        assert_eq!(
            (http.method.as_str(), http.body.as_slice()),
            ("POST", &b"hi"[..])
        );
        assert_eq!(http.headers, [("Accept".to_string(), "*/*".to_string())]);
        assert_eq!(
            serde_json::from_str::<Value>(&response_json(200, &[("a".into(), "b".into())], b"ok"))
                .unwrap(),
            json!({ "status": 200, "headers": { "a": "b" }, "body": "b2s=" })
        );
        assert!(Request::from_error("Arg_InvalidCastException").is_none());
        assert!(Request::from_error("pdn-permission:{\"plugin\":\"x.dll\"}").is_none());
    }

    /// One wording for every category, with or without a target.
    #[test]
    fn every_category_reads_as_a_verb_phrase() {
        let wants = |c: &str, t: &str| request("p", c, t).grant.wants();
        assert_eq!(wants("files", "C:\\Pictures"), "use files in C:\\Pictures");
        assert_eq!(wants("processes", "notepad.exe"), "run notepad.exe");
        assert_eq!(
            wants("system", "your desktop wallpaper"),
            "change your desktop wallpaper"
        );
        assert_eq!(wants("system", ""), "change a system setting");
        assert_eq!(
            wants("native", "user32.dll"),
            "run native code from user32.dll"
        );
        assert_eq!(wants("native", ""), "run native code");
        assert_eq!(wants("printer", ""), "use printer");
        assert_eq!(
            wants("printer", "the default printer"),
            "use the default printer"
        );
    }

    /// Allow once lasts the run, Always allow is remembered and revocable, Deny lasts until the next start; the
    /// host sees allowed entries and denied ones marked as such.
    #[test]
    fn answers_and_what_the_host_sees() {
        let net = request("Upload.dll", "network", "imgur.com");
        let wall = request("Wall.dll", "system", "your desktop wallpaper");
        let mut p = Permissions::default();
        assert!(!p.answered(&net));
        p.allow_once(&net);
        p.deny(&wall);
        assert!(p.answered(&net) && p.answered(&wall));
        assert!(p.allows(&net) && !p.allows(&wall));
        let host = |p: &Permissions| serde_json::from_str::<Value>(&p.host_json()).unwrap();
        assert_eq!(
            host(&p),
            json!({
                "Upload.dll": [{ "category": "network", "target": "imgur.com" }],
                "Wall.dll": [{ "category": "system", "target": "your desktop wallpaper", "deny": true }],
            })
        );
        p.end_once("Upload.dll");
        assert!(!p.answered(&net));
        assert_eq!(p.remembered(), "{}");

        p.allow_always(&net);
        p.end_once("Upload.dll");
        assert!(p.answered(&net));
        assert_eq!(p.always_of("Upload.dll"), std::slice::from_ref(&net.grant));
        let mut again = Permissions::default();
        again.load(&p.remembered());
        assert_eq!(
            again.always_of("Upload.dll"),
            std::slice::from_ref(&net.grant)
        );
        assert!(
            !again.answered(&wall),
            "a denial is not remembered past the start"
        );

        p.revoke("Upload.dll", &net.grant);
        assert!(!p.answered(&net) && p.always_of("Upload.dll").is_empty());
        // Allowing what was denied replaces the denial.
        p.allow_always(&wall);
        assert_eq!(
            host(&p),
            json!({ "Wall.dll": [{ "category": "system", "target": "your desktop wallpaper" }] })
        );
    }
}
