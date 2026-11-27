//! Host-owned Bifrost configuration and one cancellable chat completion.
use crate::model::Field;
use serde::{
    Deserialize, Deserializer,
    de::{Error, MapAccess, Visitor},
};
use serde_json::{Value, json};
use std::{collections::BTreeMap, path::PathBuf, sync::mpsc, time::Duration};

#[derive(Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Settings {
    pub base_url: String,
    pub model: String,
    pub key_env: String,
    #[serde(skip)]
    pub session_key: String,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:8200/v1".into(),
            model: String::new(),
            session_key: String::new(),
            key_env: "SOLIA_AI_API_KEY".into(),
        }
    }
}
impl Settings {
    pub fn load() -> Result<Self, String> {
        let explicit = std::env::var_os("SOLIA_AI_CONFIG").map(PathBuf::from);
        let path = explicit.clone().or_else(|| {
            std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config/solia/ai.json"))
        });
        let mut settings = Self::default();
        if let Some(path) = path {
            match std::fs::read(&path) {
                Ok(bytes) => {
                    settings = serde_json::from_slice(&bytes)
                        .map_err(|e| format!("Invalid AI configuration: {e}"))?
                }
                Err(e) if explicit.is_none() && e.kind() == std::io::ErrorKind::NotFound => (),
                Err(e) => return Err(format!("AI configuration: {e}")),
            }
        }
        if let Ok(s) = std::env::var("SOLIA_AI_BASE_URL") {
            settings.base_url = s;
        }
        if let Ok(s) = std::env::var("SOLIA_AI_MODEL") {
            settings.model = s;
        }
        Ok(settings)
    }
    fn endpoint(&self) -> Result<reqwest::Url, String> {
        let url = reqwest::Url::parse(&format!(
            "{}/chat/completions",
            self.base_url.trim_end_matches('/')
        ))
        .map_err(|_| "Invalid provider base URL")?;
        let local = matches!(url.host_str(), Some("localhost" | "127.0.0.1" | "[::1]"));
        if !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
            || !(url.scheme() == "https" || (local && url.scheme() == "http"))
        {
            return Err("Use HTTPS or loopback HTTP for the provider base URL".into());
        }
        if self.model.trim().is_empty() {
            return Err("Choose a Bifrost model before sending".into());
        }
        Ok(url)
    }
}

fn schema(fields: &[Field]) -> Value {
    let properties: BTreeMap<_, _> = fields
        .iter()
        .map(|f| {
            (
                &f.name,
                match f.range {
                    Some((min, max)) => json!({"type":"integer","minimum":min,"maximum":max}),
                    None => json!({"type":"boolean"}),
                },
            )
        })
        .collect();
    json!({"type":"object","properties":properties,"additionalProperties":false,"required":fields.iter().map(|f|&f.name).collect::<Vec<_>>()})
}

// Deserialize the response object directly so duplicate names cannot be overwritten.
struct Outputs(BTreeMap<String, Value>);
impl<'de> Deserialize<'de> for Outputs {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct Object;
        impl<'de> Visitor<'de> for Object {
            type Value = Outputs;
            fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
                write!(f, "a task result object")
            }
            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Outputs, A::Error> {
                let mut values = BTreeMap::new();
                while let Some((name, value)) = map.next_entry()? {
                    if values.insert(name, value).is_some() {
                        return Err(A::Error::custom("duplicate output field"));
                    }
                }
                Ok(Outputs(values))
            }
        }
        d.deserialize_map(Object)
    }
}
pub(crate) fn validate(content: &str, fields: &[Field]) -> Result<Vec<(usize, i32)>, String> {
    let Outputs(mut values) =
        serde_json::from_str(content).map_err(|e| format!("Invalid model JSON: {e}"))?;
    if values.len() != fields.len() {
        return Err("Unexpected model output fields".into());
    }
    fields
        .iter()
        .map(|f| {
            let value = values
                .remove(&f.name)
                .ok_or_else(|| format!("Missing field {}", f.name))?;
            let n = match f.range {
                Some((min, max)) => value
                    .as_i64()
                    .and_then(|n| i32::try_from(n).ok())
                    .filter(|n| (min..=max).contains(n)),
                None => value.as_bool().map(i32::from),
            }
            .ok_or_else(|| format!("Invalid model value for {}", f.name))?;
            Ok((f.slot, n))
        })
        .collect()
}

pub(crate) type Writes = Vec<(usize, i32)>;
pub(crate) struct Job {
    pub result: mpsc::Receiver<Result<Writes, String>>,
    _cancel: tokio::sync::oneshot::Sender<()>,
}
pub(crate) fn start(
    settings: &Settings,
    prompt: String,
    fields: Vec<Field>,
    ctx: eframe::egui::Context,
) -> Result<Job, String> {
    let endpoint = settings.endpoint()?;
    let key = if settings.session_key.is_empty() {
        std::env::var(&settings.key_env).ok()
    } else {
        Some(settings.session_key.clone())
    };
    let schema = schema(&fields);
    let body = json!({"model":settings.model,"stream":false,"messages":[
        {"role":"system","content":format!("Return exactly one JSON object conforming to this schema: {schema}")},
        {"role":"user","content":prompt}],
        "response_format":{"type":"json_schema","json_schema":{"name":"solia_result","strict":true,"schema":schema}}});
    let (tx, result) = mpsc::channel();
    let (cancel, closed) = tokio::sync::oneshot::channel();
    std::thread::Builder::new()
        .name("solia-inference".into())
        .spawn(move || {
            let runtime = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build();
            let result = match runtime {
                Err(e) => Err(e.to_string()),
                Ok(rt) => rt.block_on(async {
                    tokio::select! {
                        biased;
                        _ = closed => Err("Cancelled".into()),
                        result = completion(endpoint,key,body,&fields) => result,
                    }
                }),
            };
            let _ = tx.send(result);
            ctx.request_repaint();
        })
        .map_err(|e| e.to_string())?;
    Ok(Job {
        result,
        _cancel: cancel,
    })
}

async fn completion(
    endpoint: reqwest::Url,
    key: Option<String>,
    body: Value,
    fields: &[Field],
) -> Result<Writes, String> {
    let client = reqwest::Client::builder()
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client.post(endpoint).json(&body);
    if let Some(key) = key {
        req = req.header("x-bf-vk", key);
    }
    let mut response = req
        .send()
        .await
        .map_err(|e| format!("Bifrost request failed: {e}"))?;
    if !response.status().is_success() {
        return Err(format!("Bifrost returned HTTP {}", response.status()));
    }
    let mut data = vec![];
    while let Some(chunk) = response.chunk().await.map_err(|e| e.to_string())? {
        if chunk.len() > 16384 - data.len() {
            return Err("Bifrost response exceeds 16 KiB".into());
        }
        data.extend_from_slice(&chunk);
    }
    let envelope: Value = serde_json::from_slice(&data).map_err(|e| e.to_string())?;
    let content = envelope
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .ok_or("Missing Bifrost completion")?;
    validate(content, fields)
}
