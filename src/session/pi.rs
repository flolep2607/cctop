//! Pi coding-agent session discovery and JSONL extraction.

use super::extract::{self, for_each_jsonl};
use super::{Costs, FallbackRates, ModelBreakdown, Session, SessionData, Tokens};
use crate::config;
use crate::pricing::Provider;
use crate::util;
use serde_json::Value;
use std::collections::HashMap;
use std::path::Path;

fn message_ts(item: &Value, message: &Value) -> String {
    message
        .get("timestamp")
        .and_then(Value::as_i64)
        .map(util::ms_to_rfc3339)
        .filter(|s| !s.is_empty())
        .or_else(|| {
            item.get("timestamp")
                .and_then(Value::as_str)
                .map(str::to_string)
        })
        .unwrap_or_default()
}

/// Discover every persisted Pi session, including custom session roots.
pub fn list_sessions() -> Vec<Session> {
    let mut sessions = Vec::new();
    let paths: Vec<_> = config::pi_sessions_roots()
        .iter()
        .flat_map(|root| config::rglob(root, ".jsonl"))
        .collect();
    for path in paths {
        let mut session_id = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        let mut started = String::new();
        let mut last = String::new();
        let mut cwd = String::new();
        let mut title = None;
        let mut model = String::new();

        let _ = for_each_jsonl(&path, |item| {
            match item.get("type").and_then(Value::as_str) {
                Some("session") => {
                    if let Some(id) = item.get("id").and_then(Value::as_str) {
                        session_id = id.to_string();
                    }
                    started = item
                        .get("timestamp")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    cwd = item
                        .get("cwd")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    last = started.clone();
                }
                Some("session_info") => {
                    title = item.get("name").and_then(Value::as_str).map(str::to_string);
                }
                Some("message") => {
                    let Some(message) = item.get("message") else {
                        return;
                    };
                    let ts = message_ts(item, message);
                    if ts > last {
                        last = ts;
                    }
                    if message.get("role").and_then(Value::as_str) == Some("assistant")
                        && let Some(m) = message.get("model").and_then(Value::as_str)
                    {
                        model = m.to_string();
                    }
                }
                _ => {}
            }
        });

        if session_id.is_empty() || started.is_empty() {
            continue;
        }
        let mut session = Session::new(Provider::Pi, session_id);
        session.started_at = started;
        session.last_active = last;
        session.label_source = cwd;
        session.title = title;
        session.model = model;
        session.data_file = Some(path);
        sessions.push(session);
    }
    sessions.sort_by(|a, b| b.started_at.cmp(&a.started_at));
    sessions
}

fn usage_number(usage: &Value, key: &str) -> u64 {
    usage.get(key).and_then(Value::as_u64).unwrap_or(0)
}

fn cost_number(cost: &Value, key: &str) -> f64 {
    cost.get(key).and_then(Value::as_f64).unwrap_or(0.0)
}

/// Parse Pi's documented session JSONL format.
///
/// Usage costs are recorded by Pi and preserved exactly rather than re-estimated
/// from a model ID. The exception is a turn Pi priced at nothing while reporting
/// tokens, which is what a provider it has no rates for produces: those are
/// priced from LiteLLM, since reporting them as free hides real spend.
pub fn extract(path: &Path) -> SessionData {
    let mut data = SessionData::default();
    let mut breakdown: HashMap<String, (Tokens, Costs)> = HashMap::new();
    let mut rates = FallbackRates::default();

    let result = for_each_jsonl(path, |item| {
        if item.get("type").and_then(Value::as_str) == Some("session_info") {
            data.title = item.get("name").and_then(Value::as_str).map(str::to_string);
            return;
        }
        if item.get("type").and_then(Value::as_str) != Some("message") {
            return;
        }
        let Some(message) = item.get("message") else {
            return;
        };
        if message.get("role").and_then(Value::as_str) != Some("assistant") {
            return;
        }

        let model = message
            .get("model")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        data.last_model = model.clone();
        let usage = message.get("usage").unwrap_or(&Value::Null);
        let cost = usage.get("cost").unwrap_or(&Value::Null);
        let cache_write = usage_number(usage, "cacheWrite");
        let cache_write_1h = usage_number(usage, "cacheWrite1h").min(cache_write);
        let tokens = Tokens {
            input: usage_number(usage, "input"),
            output: usage_number(usage, "output"),
            cache_read: usage_number(usage, "cacheRead"),
            cache_write_5m: cache_write - cache_write_1h,
            cache_write_1h,
            reasoning_output: usage_number(usage, "reasoning"),
            total: usage_number(usage, "totalTokens"),
            ..Default::default()
        };
        let reported = Costs {
            input: cost_number(cost, "input"),
            output: cost_number(cost, "output"),
            cache_read: cost_number(cost, "cacheRead"),
            cache_write_5m: cost_number(cost, "cacheWrite"),
            total: cost_number(cost, "total"),
            ..Default::default()
        };
        let billable = tokens.input + tokens.output + tokens.cache_read + tokens.cache_write_5m;
        let costs = if reported.total > 0.0 || billable == 0 {
            reported
        } else {
            rates.costs(&model, &tokens).unwrap_or(reported)
        };

        data.tokens.input += tokens.input;
        data.tokens.output += tokens.output;
        data.tokens.cache_read += tokens.cache_read;
        data.tokens.cache_write_5m += tokens.cache_write_5m;
        data.tokens.cache_write_1h += tokens.cache_write_1h;
        data.tokens.reasoning_output += tokens.reasoning_output;
        data.tokens.total += tokens.total;
        data.costs.input += costs.input;
        data.costs.output += costs.output;
        data.costs.cache_read += costs.cache_read;
        data.costs.cache_write_5m += costs.cache_write_5m;
        data.costs.total += costs.total;

        let (mt, mc) = breakdown.entry(model.clone()).or_default();
        mt.input += tokens.input;
        mt.output += tokens.output;
        mt.cache_read += tokens.cache_read;
        mt.cache_write_5m += tokens.cache_write_5m;
        mt.cache_write_1h += tokens.cache_write_1h;
        mt.reasoning_output += tokens.reasoning_output;
        mt.total += tokens.total;
        mc.input += costs.input;
        mc.output += costs.output;
        mc.cache_read += costs.cache_read;
        mc.cache_write_5m += costs.cache_write_5m;
        mc.total += costs.total;

        let ts = message_ts(item, message);
        if let Some(dt) = util::parse_ts(&ts) {
            *data
                .costs_by_day
                .entry(util::local_date_key(&dt))
                .or_default()
                .entry(model.clone())
                .or_insert(0.0) += costs.total;
            *data
                .costs_by_hour
                .entry(util::local_hour_key(&dt))
                .or_default()
                .entry(model.clone())
                .or_insert(0.0) += costs.total;
        }

        if let Some(content) = message.get("content").and_then(Value::as_array) {
            for block in content {
                if block.get("type").and_then(Value::as_str) != Some("toolCall") {
                    continue;
                }
                let name = block
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown");
                let args = block.get("arguments").unwrap_or(&Value::Null);
                let (short, full) = extract::tool_detail(name, args);
                extract::push_tool_detail(
                    &mut data.metrics.tool_details,
                    name,
                    short,
                    full,
                    ts.clone(),
                    block.get("id").and_then(Value::as_str).map(str::to_string),
                    None,
                );
                *data.metrics.tools.entry(name.to_string()).or_insert(0) += 1;
                data.metrics.tool_count += 1;
            }
        }
    });

    if let Err(err) = result {
        data.error = Some(format!(
            "Could not read Pi session {}: {err}",
            path.display()
        ));
        return data;
    }
    let mut models: Vec<_> = breakdown.into_iter().collect();
    models.sort_by(|a, b| a.0.cmp(&b.0));
    data.models = models.iter().map(|(m, _)| m.clone()).collect();
    data.model_breakdown = models
        .into_iter()
        .map(|(model, (tokens, costs))| ModelBreakdown {
            model,
            total: costs.total,
            tokens,
            costs,
        })
        .collect();
    data
}

pub fn delete(session: &Session) -> std::io::Result<()> {
    let Some(path) = &session.data_file else {
        return Ok(());
    };
    std::fs::remove_file(path)
}

pub fn extract_last_tool(session: &Session) -> String {
    let Some(path) = &session.data_file else {
        return String::new();
    };
    let mut last = String::new();
    let _ = for_each_jsonl(path, |item| {
        let Some(content) = item
            .get("message")
            .and_then(|m| m.get("content"))
            .and_then(Value::as_array)
        else {
            return;
        };
        for block in content {
            if block.get("type").and_then(Value::as_str) == Some("toolCall")
                && let Some(name) = block.get("name").and_then(Value::as_str)
            {
                last = name.to_string();
            }
        }
    });
    last
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_recorded_usage_cost_and_tools() {
        let path = std::env::temp_dir().join(format!("cctop-pi-{}.jsonl", std::process::id()));
        let fixture = concat!(
            "{\"type\":\"session\",\"version\":3,\"id\":\"pi-1\",\"timestamp\":\"2026-08-05T00:00:00Z\",\"cwd\":\"/work\"}\n",
            "{\"type\":\"session_info\",\"name\":\"Pi work\"}\n",
            "{\"type\":\"message\",\"timestamp\":\"2026-08-05T00:01:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"gpt-test\",\"timestamp\":1785888060000,\"usage\":{\"input\":100,\"output\":20,\"cacheRead\":50,\"cacheWrite\":10,\"totalTokens\":180,\"cost\":{\"input\":0.1,\"output\":0.2,\"cacheRead\":0.01,\"cacheWrite\":0.02,\"total\":0.33}},\"content\":[{\"type\":\"toolCall\",\"id\":\"call-1\",\"name\":\"read\",\"arguments\":{\"path\":\"README.md\"}}]}}\n"
        );
        std::fs::write(&path, fixture).unwrap();

        let data = extract(&path);
        assert_eq!(data.title.as_deref(), Some("Pi work"));
        assert_eq!(data.last_model, "gpt-test");
        assert_eq!(data.tokens.total, 180);
        assert!((data.costs.total - 0.33).abs() < 1e-9);
        assert_eq!(data.metrics.tool_count, 1);
        assert_eq!(data.metrics.tools.get("read"), Some(&1));

        std::fs::remove_file(path).unwrap();
    }

    /// Pi prices what it has rates for and writes zeros for the rest, so a turn
    /// with tokens and no cost is a provider Pi doesn't know — not a free one.
    #[test]
    fn a_turn_pi_could_not_price_is_costed_from_litellm() {
        let _rates = crate::pricing::install_test_table(&[(
            "zai/glm-5.1",
            serde_json::json!({
                "input_cost_per_token": 1.4e-6,
                "output_cost_per_token": 4.4e-6,
                "cache_read_input_token_cost": 2.6e-7,
                "cache_creation_input_token_cost": 0.0,
            }),
        )]);
        let path = std::env::temp_dir().join(format!(
            "cctop-pi-fallback-{}-{}.jsonl",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            concat!(
                "{\"type\":\"session\",\"version\":3,\"id\":\"pi-2\",\"timestamp\":\"2026-08-05T00:00:00Z\",\"cwd\":\"/work\"}\n",
                "{\"type\":\"message\",\"timestamp\":\"2026-08-05T00:01:00Z\",\"message\":{\"role\":\"assistant\",\"model\":\"myproxy/zai/glm-5.1\",\"timestamp\":1785888060000,\"usage\":{\"input\":1000000,\"output\":1000000,\"cacheRead\":1000000,\"cacheWrite\":1000000,\"totalTokens\":4000000,\"cost\":{\"total\":0}}}}\n"
            ),
        )
        .unwrap();

        let data = extract(&path);
        // 1.40 input + 4.40 output + 0.26 cache read + 0.00 cache write per Mtok.
        assert!(
            (data.costs.total - 6.06).abs() < 1e-9,
            "priced at {}",
            data.costs.total
        );
        // The breakdown has to add up to the total, not just the total be right.
        assert!((data.costs.input - 1.4).abs() < 1e-9);
        assert!((data.costs.output - 4.4).abs() < 1e-9);
        assert!((data.costs.cache_read - 0.26).abs() < 1e-9);
        assert_eq!(data.costs.cache_write_5m, 0.0);

        std::fs::remove_file(path).unwrap();
    }
}
