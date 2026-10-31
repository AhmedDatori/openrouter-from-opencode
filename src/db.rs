use anyhow::{Context, Result};
use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering::Relaxed},
        mpsc,
    },
};
use tokio::sync::oneshot;

type Job = Box<dyn FnOnce(&mut Connection) + Send + 'static>;
#[derive(Clone)]
pub struct Db {
    sender: Arc<mpsc::Sender<Job>>,
    pending: Arc<AtomicUsize>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub id: String,
    pub name: String,
    pub prefix: String,
    pub created_at: String,
    pub disabled: bool,
    pub rpm: u32,
    pub allowed_models: Vec<String>,
    pub expires_at: Option<i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Log {
    pub id: String,
    pub created_at: String,
    pub key_id: Option<String>,
    pub path: String,
    pub method: String,
    pub kind: String,
    pub model: Option<String>,
    pub upstream_id: Option<String>,
    pub session_id: Option<String>,
    pub app: Option<String>,
    pub status: u16,
    pub outcome: String,
    pub duration_ms: u64,
    pub first_byte_ms: Option<u64>,
    pub first_token_ms: Option<u64>,
    pub bytes_in: usize,
    pub bytes_out: usize,
    pub usage: Option<Value>,
    pub request: Option<Value>,
    pub response: Option<Value>,
    pub error: Option<String>,
    pub attempts: Vec<Value>,
}
impl Log {
    pub fn new(id: String, path: String, method: String) -> Self {
        Self {
            id,
            created_at: chrono::Utc::now().to_rfc3339(),
            key_id: None,
            path,
            method,
            kind: "api".into(),
            model: None,
            upstream_id: None,
            session_id: None,
            app: None,
            status: 200,
            outcome: "completed".into(),
            duration_ms: 0,
            first_byte_ms: None,
            first_token_ms: None,
            bytes_in: 0,
            bytes_out: 0,
            usage: None,
            request: None,
            response: None,
            error: None,
            attempts: vec![],
        }
    }
    pub fn costs(&self) -> (Option<f64>, Option<f64>, Option<f64>) {
        let u = self.usage.as_ref();
        (
            u.and_then(|v| v.get("cost")).and_then(Value::as_f64),
            u.and_then(|v| v.pointer("/opencode/list_cost"))
                .and_then(Value::as_f64),
            u.and_then(|v| v.pointer("/opencode/allowance"))
                .and_then(Value::as_f64),
        )
    }
}
impl Db {
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(p) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(p)?;
        }
        let mut conn = Connection::open(path).context("Open SQLite database")?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))?;
        }
        conn.busy_timeout(std::time::Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON; PRAGMA cache_size=-2048;
          CREATE TABLE IF NOT EXISTS api_keys(id TEXT PRIMARY KEY, hash TEXT NOT NULL UNIQUE, name TEXT NOT NULL, prefix TEXT NOT NULL, created_at TEXT NOT NULL, disabled INTEGER NOT NULL DEFAULT 0, rpm INTEGER NOT NULL DEFAULT 60, allowed_models TEXT NOT NULL DEFAULT '[]', expires_at INTEGER);
          CREATE TABLE IF NOT EXISTS admin_sessions(hash TEXT PRIMARY KEY, csrf TEXT NOT NULL, expires_at INTEGER NOT NULL);
          CREATE TABLE IF NOT EXISTS logs(id TEXT PRIMARY KEY, created_at TEXT NOT NULL, key_id TEXT, kind TEXT NOT NULL, model TEXT, upstream_id TEXT, status INTEGER NOT NULL, outcome TEXT NOT NULL, duration_ms INTEGER NOT NULL, cash REAL, list_cost REAL, allowance REAL, prompt_tokens INTEGER, completion_tokens INTEGER, body TEXT NOT NULL);
          CREATE INDEX IF NOT EXISTS logs_created ON logs(created_at DESC);
          CREATE INDEX IF NOT EXISTS logs_key ON logs(key_id,created_at DESC);
          CREATE INDEX IF NOT EXISTS logs_upstream ON logs(upstream_id);
          CREATE TABLE IF NOT EXISTS usage_daily(day TEXT NOT NULL, key_id TEXT NOT NULL, model TEXT NOT NULL, requests INTEGER NOT NULL DEFAULT 0, errors INTEGER NOT NULL DEFAULT 0, cash REAL NOT NULL DEFAULT 0, list_cost REAL NOT NULL DEFAULT 0, allowance REAL NOT NULL DEFAULT 0, prompt_tokens INTEGER NOT NULL DEFAULT 0, completion_tokens INTEGER NOT NULL DEFAULT 0, usage_missing INTEGER NOT NULL DEFAULT 0, duration_ms INTEGER NOT NULL DEFAULT 0, PRIMARY KEY(day,key_id,model));
          CREATE TABLE IF NOT EXISTS admin_events(id INTEGER PRIMARY KEY, created_at TEXT DEFAULT CURRENT_TIMESTAMP, action TEXT NOT NULL, details TEXT NOT NULL);
          CREATE TABLE IF NOT EXISTS gateway_secrets(name TEXT PRIMARY KEY, value TEXT NOT NULL);
          PRAGMA user_version=1;")?;
        let (tx, rx) = mpsc::channel::<Job>();
        let pending = Arc::new(AtomicUsize::new(0));
        let done = pending.clone();
        std::thread::Builder::new()
            .name("sqlite".into())
            .spawn(move || {
                for job in rx {
                    job(&mut conn);
                    done.fetch_sub(1, Relaxed);
                }
            })?;
        Ok(Self {
            sender: Arc::new(tx),
            pending,
        })
    }
    /// Database jobs queued or running; a growing value means storage is the bottleneck.
    pub fn queue_depth(&self) -> usize {
        self.pending.load(Relaxed)
    }
    fn send(&self, job: Job) -> Result<(), mpsc::SendError<Job>> {
        self.pending.fetch_add(1, Relaxed);
        self.sender.send(job).inspect_err(|_| {
            self.pending.fetch_sub(1, Relaxed);
        })
    }
    pub async fn call<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let (tx, rx) = oneshot::channel();
        self.send(Box::new(move |conn| {
            let _ = tx.send(f(conn));
        }))
        .map_err(|_| anyhow::anyhow!("Database worker stopped"))?;
        rx.await.context("Database worker response")?
    }
    pub fn save_log(&self, log: Log) {
        let _ = self.send(Box::new(move |c| {
            let result = (|| -> Result<()> {
                let (cash, list, allowance) = log.costs();
                let usage = log.usage.as_ref();
                let prompt = usage.and_then(|u| u.get("prompt_tokens").or_else(|| u.get("input_tokens"))).and_then(Value::as_i64);
                let completion = usage.and_then(|u| u.get("completion_tokens").or_else(|| u.get("output_tokens"))).and_then(Value::as_i64);
                let body = serde_json::to_string(&log)?;
                let tx = c.transaction()?;
                let inserted = tx.execute("INSERT OR IGNORE INTO logs VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)", params![log.id, log.created_at, log.key_id, log.kind, log.model, log.upstream_id, log.status, log.outcome, log.duration_ms, cash, list, allowance, prompt, completion, body])?;
                if inserted > 0 && log.kind == "inference" {
                    tx.execute("INSERT INTO usage_daily VALUES(?1,?2,?3,1,?4,?5,?6,?7,?8,?9,?10,?11)
                      ON CONFLICT(day,key_id,model) DO UPDATE SET requests=requests+1,errors=errors+excluded.errors,cash=cash+excluded.cash,list_cost=list_cost+excluded.list_cost,allowance=allowance+excluded.allowance,prompt_tokens=prompt_tokens+excluded.prompt_tokens,completion_tokens=completion_tokens+excluded.completion_tokens,usage_missing=usage_missing+excluded.usage_missing,duration_ms=duration_ms+excluded.duration_ms",params![&log.created_at[..10],log.key_id.as_deref().unwrap_or("unknown"),log.model.as_deref().unwrap_or("unresolved"),i64::from(log.status>=400||log.outcome!="completed"),cash.unwrap_or(0.0),list.unwrap_or(0.0),allowance.unwrap_or(0.0),prompt.unwrap_or(0),completion.unwrap_or(0),i64::from(log.usage.is_none()),log.duration_ms])?;
                }
                tx.commit()?;
                Ok(())
            })();
            if let Err(e) = result { eprintln!("save log failed: {e}"); }
        }));
    }
    pub async fn flush(&self) -> Result<()> {
        self.call(|_| Ok(())).await
    }
    pub async fn upstream_credential(&self) -> Result<Option<String>> {
        self.call(|c| {
            c.query_row(
                "SELECT value FROM gateway_secrets WHERE name='opencode_go_key'",
                [],
                |r| r.get(0),
            )
            .optional()
            .map_err(Into::into)
        })
        .await
    }
    pub async fn set_upstream_credential(&self, key: String) -> Result<()> {
        self.call(move |c| {
            let tx = c.transaction()?;
            let configured = !key.is_empty();
            tx.execute("INSERT INTO gateway_secrets(name,value) VALUES('opencode_go_key',?1) ON CONFLICT(name) DO UPDATE SET value=excluded.value", [key])?;
            tx.execute("INSERT INTO admin_events(action,details) VALUES(?1,?2)", params![if configured { "upstream_key_saved" } else { "upstream_key_removed" }, json!({"configured":configured}).to_string()])?;
            tx.commit()?;
            Ok(())
        }).await
    }
    pub async fn find_key(&self, hash: String) -> Result<Option<Key>> {
        self.call(move|c|c.query_row("SELECT id,name,prefix,created_at,disabled,rpm,allowed_models,expires_at FROM api_keys WHERE hash=?1",[hash],key_row).optional().map_err(Into::into)).await
    }
    pub async fn keys(&self) -> Result<Vec<Key>> {
        self.call(|c|{let mut q=c.prepare("SELECT id,name,prefix,created_at,disabled,rpm,allowed_models,expires_at FROM api_keys ORDER BY created_at DESC")?;Ok(q.query_map([],key_row)?.collect::<rusqlite::Result<_>>()?)}).await
    }
    pub async fn create_key(&self, key: Key, hash: String) -> Result<Key> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO api_keys VALUES(?1,?2,?3,?4,?5,?6,?7,?8,?9)",
                params![
                    key.id,
                    hash,
                    key.name,
                    key.prefix,
                    key.created_at,
                    key.disabled,
                    key.rpm,
                    serde_json::to_string(&key.allowed_models)?,
                    key.expires_at
                ],
            )?;
            Ok(key)
        })
        .await
    }
    pub async fn update_key(&self, id: String, patch: Value) -> Result<Option<Key>> {
        self.call(move|c|{
            let old=c.query_row("SELECT id,name,prefix,created_at,disabled,rpm,allowed_models,expires_at FROM api_keys WHERE id=?1",[&id],key_row).optional()?;
            let Some(mut k)=old else{return Ok(None)};
            if let Some(v)=patch.get("name").and_then(Value::as_str){k.name=v.into()}
            if let Some(v)=patch.get("disabled").and_then(Value::as_bool){k.disabled=v}
            if let Some(v)=patch.get("rpm").and_then(Value::as_u64){k.rpm=v as u32}
            if let Some(v)=patch.get("allowed_models"){k.allowed_models=serde_json::from_value(v.clone())?}
            if let Some(v)=patch.get("expires_at"){k.expires_at=v.as_i64()}
            c.execute("UPDATE api_keys SET name=?2,disabled=?3,rpm=?4,allowed_models=?5,expires_at=?6 WHERE id=?1",params![id,k.name,k.disabled,k.rpm,serde_json::to_string(&k.allowed_models)?,k.expires_at])?;Ok(Some(k))
        }).await
    }
    pub async fn delete_key(&self, id: String) -> Result<bool> {
        self.call(move |c| Ok(c.execute("UPDATE api_keys SET disabled=1 WHERE id=?1", [id])? > 0))
            .await
    }
    pub async fn session(&self, hash: String) -> Result<Option<(String, i64)>> {
        self.call(move |c| {
            Ok(c.query_row(
                "SELECT csrf,expires_at FROM admin_sessions WHERE hash=?1",
                [hash],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?)
        })
        .await
    }
    pub async fn new_session(&self, hash: String, csrf: String) -> Result<()> {
        self.call(move |c| {
            c.execute(
                "INSERT INTO admin_sessions VALUES(?1,?2,?3)",
                params![hash, csrf, chrono::Utc::now().timestamp() + 43200],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn drop_session(&self, hash: String) -> Result<()> {
        self.call(move |c| {
            c.execute("DELETE FROM admin_sessions WHERE hash=?1", [hash])?;
            Ok(())
        })
        .await
    }
    pub async fn event(&self, action: &str, details: Value) -> Result<()> {
        let action = action.to_string();
        self.call(move |c| {
            c.execute(
                "INSERT INTO admin_events(action,details) VALUES(?1,?2)",
                params![action, details.to_string()],
            )?;
            Ok(())
        })
        .await
    }
    pub async fn logs(
        &self,
        page: u64,
        limit: u64,
        key: Option<String>,
        model: Option<String>,
        only_errors: bool,
    ) -> Result<Value> {
        self.call(move|c|{
            let filter="(?1 IS NULL OR key_id=?1) AND (?2 IS NULL OR model=?2) AND (?3=0 OR status>=400 OR outcome!='completed')";
            let total:i64=c.query_row(&format!("SELECT count(*) FROM logs WHERE {filter}"),params![key,model,only_errors],|r|r.get(0))?;
            let mut q=c.prepare(&format!("SELECT body FROM logs WHERE {filter} ORDER BY created_at DESC LIMIT ?4 OFFSET ?5"))?;
            let rows=q.query_map(params![key,model,only_errors,limit,(page-1)*limit],|r|r.get::<_,String>(0))?;
            let mut data=vec![];
            for row in rows {let mut v:Value=serde_json::from_str(&row?)?;v.as_object_mut().unwrap().remove("request");v.as_object_mut().unwrap().remove("response");data.push(v)}
            Ok(json!({"data":data,"total":total,"page":page,"limit":limit}))
        }).await
    }
    pub async fn log(&self, id: String, key_id: Option<String>) -> Result<Option<Value>> {
        self.call(move|c|{let s:Option<String>=c.query_row("SELECT body FROM logs WHERE (id=?1 OR upstream_id=?1) AND (?2 IS NULL OR key_id=?2) ORDER BY created_at DESC LIMIT 1",params![id,key_id],|r|r.get(0)).optional()?;s.map(|s|serde_json::from_str(&s).map_err(Into::into)).transpose()}).await
    }
    pub async fn stats(&self) -> Result<Value> {
        self.call(|c| {
            let totals=c.query_row("SELECT coalesce(sum(requests),0),coalesce(sum(errors),0),coalesce(sum(cash),0),coalesce(sum(list_cost),0),coalesce(sum(allowance),0),coalesce(sum(prompt_tokens),0),coalesce(sum(completion_tokens),0),coalesce(1.0*sum(duration_ms)/nullif(sum(requests),0),0),coalesce(sum(usage_missing),0) FROM usage_daily",[],|r|Ok(json!({"requests":r.get::<_,i64>(0)?,"errors":r.get::<_,i64>(1)?,"cash_usd":r.get::<_,f64>(2)?,"list_usd":r.get::<_,f64>(3)?,"allowance_usd":r.get::<_,f64>(4)?,"prompt_tokens":r.get::<_,i64>(5)?,"completion_tokens":r.get::<_,i64>(6)?,"avg_duration_ms":r.get::<_,f64>(7)?,"usage_missing":r.get::<_,i64>(8)?})))?;
            let mut q=c.prepare("SELECT day,sum(requests),sum(cash),sum(allowance),sum(prompt_tokens+completion_tokens) FROM usage_daily GROUP BY day ORDER BY day DESC LIMIT 30")?;
            let daily=q.query_map([],|r|Ok(json!({"date":r.get::<_,String>(0)?,"requests":r.get::<_,i64>(1)?,"cash_usd":r.get::<_,f64>(2)?,"allowance_usd":r.get::<_,f64>(3)?,"tokens":r.get::<_,i64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            let mut q=c.prepare("SELECT key_id,sum(requests),sum(cash),sum(allowance),sum(prompt_tokens+completion_tokens) FROM usage_daily GROUP BY key_id ORDER BY sum(requests) DESC")?;
            let keys=q.query_map([],|r|Ok(json!({"key_id":r.get::<_,String>(0)?,"requests":r.get::<_,i64>(1)?,"cash_usd":r.get::<_,f64>(2)?,"allowance_usd":r.get::<_,f64>(3)?,"tokens":r.get::<_,i64>(4)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(json!({"totals":totals,"daily":daily,"keys":keys}))
        }).await
    }
    pub async fn events(&self) -> Result<Value> {
        self.call(|c| {
            let mut q=c.prepare("SELECT id,created_at,action,details FROM admin_events ORDER BY id DESC LIMIT 100")?;
            let data=q.query_map([],|r|Ok(json!({"id":r.get::<_,i64>(0)?,"created_at":r.get::<_,String>(1)?,"action":r.get::<_,String>(2)?,"details":r.get::<_,String>(3)?})))?.collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(json!({"data":data}))
        }).await
    }
    pub async fn cleanup(&self, days: i64) -> Result<()> {
        self.call(move |c| {
            let before = (chrono::Utc::now() - chrono::Duration::days(days)).to_rfc3339();
            c.execute("DELETE FROM logs WHERE created_at<?1", [before])?;
            c.execute(
                "DELETE FROM admin_sessions WHERE expires_at<?1",
                [chrono::Utc::now().timestamp()],
            )?;
            c.execute("PRAGMA incremental_vacuum(100)", [])?;
            Ok(())
        })
        .await
    }
}
fn key_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Key> {
    let models: String = r.get(6)?;
    Ok(Key {
        id: r.get(0)?,
        name: r.get(1)?,
        prefix: r.get(2)?,
        created_at: r.get(3)?,
        disabled: r.get(4)?,
        rpm: r.get(5)?,
        allowed_models: serde_json::from_str(&models).unwrap_or_default(),
        expires_at: r.get(7)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[tokio::test]
    async fn accounting_survives_payload_cleanup_and_duplicate_submissions() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway.db");
        let db = Db::open(&path).unwrap();
        let mut log = Log::new(
            "gen-accounting".into(),
            "/chat/completions".into(),
            "POST".into(),
        );
        log.created_at = (chrono::Utc::now() - chrono::Duration::days(40)).to_rfc3339();
        log.kind = "inference".into();
        log.key_id = Some("key1".into());
        log.model = Some("test/model".into());
        log.usage = Some(
            json!({"prompt_tokens":10,"completion_tokens":5,"cost":0,"opencode":{"allowance":0.0004,"list_cost":0.0006}}),
        );
        db.save_log(log.clone());
        db.save_log(log);
        db.flush().await.unwrap();
        assert_eq!(db.stats().await.unwrap()["totals"]["requests"], 1);
        assert_eq!(db.logs(1, 10, None, None, false).await.unwrap()["total"], 1);
        db.cleanup(30).await.unwrap();
        assert_eq!(db.logs(1, 10, None, None, false).await.unwrap()["total"], 0);
        let totals = db.stats().await.unwrap();
        assert_eq!(totals["totals"]["requests"], 1);
        assert_eq!(totals["totals"]["allowance_usd"], 0.0004);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            assert_eq!(
                std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
                0o600
            );
        }
    }
}
