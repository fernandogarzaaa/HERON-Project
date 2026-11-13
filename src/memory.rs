//! Long-term memory with hybrid retrieval: FTS5 keyword search + vector similarity,
//! fused with reciprocal-rank fusion and weighted by importance and recency.
//!
//! Embeddings work fully offline by default (feature-hashed word + character n-grams);
//! a remote embedding model can be configured for stronger semantic recall.

use crate::llm::{ChatRequest, Llm, Message};
use crate::store::{MemoryItem, Store};
use crate::util::{now_ts, redact_secrets};
use anyhow::Result;
use std::collections::HashMap;
use std::sync::Arc;

const LOCAL_DIM: usize = 384;
/// Above this cosine similarity a new memory is treated as an update of an existing one.
const DEDUPE_THRESHOLD: f32 = 0.88;
/// Facts from these sources were written by other people or inferred, not stated by the user.
pub const OUTSIDE_SOURCES: &[&str] = &["email", "calendar", "documents", "observed", "computer", "outside"];

#[derive(Clone)]
pub struct Memory {
    store: Store,
    remote: Option<Arc<dyn Llm>>,
    /// Where the local neural model is kept, when the user has switched it on.
    neural_dir: Option<std::path::PathBuf>,
}

fn fnv1a(s: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in s {
        h ^= *b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    h
}

const STOPWORDS: &[&str] = &[
    "the", "a", "an", "is", "are", "was", "were", "be", "to", "of", "and", "or", "in", "on", "at", "for", "with", "that", "this", "it", "as", "by", "from", "my",
    "me", "i", "you", "your", "do", "does", "did", "what", "which", "who", "how", "when", "where", "s", "user", "user's", "has", "have", "about",
];

pub fn tokenize(text: &str) -> Vec<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '\'')
        .map(|w| w.trim_matches('\'').to_string())
        .filter(|w| w.chars().count() >= 2 && !STOPWORDS.contains(&w.as_str()))
        .collect()
}

/// Offline embedding: signed feature hashing of words (weight 1.0) and char trigrams (0.5), L2-normalized.
pub fn local_embed(text: &str) -> Vec<f32> {
    let mut v = vec![0f32; LOCAL_DIM];
    let mut add = |feat: &str, w: f32| {
        let h = fnv1a(feat.as_bytes());
        let idx = (h % LOCAL_DIM as u64) as usize;
        let sign = if (h >> 63) & 1 == 1 { -1.0 } else { 1.0 };
        v[idx] += sign * w;
    };
    for word in tokenize(text) {
        // Crude stemming so "running"/"runs" land near "run".
        let stem = word.trim_end_matches("ing").trim_end_matches("es").trim_end_matches('s').to_string();
        add(&format!("w:{}", if stem.len() >= 3 { &stem } else { &word }), 1.0);
        let chars: Vec<char> = format!("^{word}$").chars().collect();
        for tri in chars.windows(3) {
            add(&format!("t:{}", tri.iter().collect::<String>()), 0.5);
        }
    }
    let norm = v.iter().map(|x| x * x).sum::<f32>().sqrt();
    if norm > 0.0 {
        v.iter_mut().for_each(|x| *x /= norm);
    }
    v
}

pub fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    if na == 0.0 || nb == 0.0 { 0.0 } else { dot / (na.sqrt() * nb.sqrt()) }
}

fn to_blob(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

fn from_blob(b: &[u8]) -> Vec<f32> {
    b.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

#[derive(Debug, Clone, PartialEq)]
pub enum SaveOutcome {
    Created(i64),
    Updated(i64),
}

impl SaveOutcome {
    pub fn id(&self) -> i64 {
        match self {
            SaveOutcome::Created(i) | SaveOutcome::Updated(i) => *i,
        }
    }
}

impl Memory {
    pub fn new(store: Store, remote: Option<Arc<dyn Llm>>) -> Self {
        Memory { store, remote, neural_dir: None }
    }

    /// Use the local neural model for embeddings (falls back to the built-in method if it cannot load).
    pub fn with_neural(mut self, cache_dir: std::path::PathBuf) -> Self {
        self.neural_dir = Some(cache_dir);
        self
    }

    /// The embedder this memory prefers right now.
    pub fn preferred_embedder(&self) -> &'static str {
        if self.neural_dir.is_some() && crate::embed::neural_built_in() {
            crate::embed::NEURAL_TAG
        } else if self.remote.is_some() {
            crate::embed::REMOTE_TAG
        } else {
            crate::embed::HASH_TAG
        }
    }

    /// Embed text, returning the vector and which embedder actually produced it. Order of preference:
    /// the local neural model, a remote embedding model, then the built-in offline method, which never fails.
    async fn embed(&self, text: &str) -> (Vec<f32>, &'static str) {
        if let Some(dir) = self.neural_dir.clone().filter(|_| crate::embed::neural_built_in()) {
            let input = vec![text.to_string()];
            // Model inference is CPU work: keep it off the async threads.
            if let Ok(Ok(mut v)) = tokio::task::spawn_blocking(move || crate::embed::neural_embed(&dir, &input)).await {
                if let Some(e) = v.pop().filter(|e| !e.is_empty()) {
                    return (e, crate::embed::NEURAL_TAG);
                }
            }
        } else if let Some(llm) = &self.remote {
            if let Ok(mut v) = llm.embed(&[text.to_string()]).await {
                if let Some(e) = v.pop().filter(|e| !e.is_empty()) {
                    return (e, crate::embed::REMOTE_TAG);
                }
            }
        }
        (local_embed(text), crate::embed::HASH_TAG)
    }

    /// Stored vectors made by `embedder`. One of a different size than today's (`dims`) was made by an
    /// earlier model under the same name, for example after the embedding model was changed in
    /// Settings. It cannot be compared with anything, so it is left out and queued for re-indexing.
    fn vectors(&self, embedder: &str, dims: usize) -> Result<Vec<(i64, Vec<f32>)>> {
        let mut out = vec![];
        for (id, blob) in self.store.memory_embeddings(embedder)? {
            let v = from_blob(&blob);
            if v.len() == dims {
                out.push((id, v));
            } else {
                let _ = self.store.mark_embedding_stale(id);
            }
        }
        Ok(out)
    }

    /// Bring older memories into the current embedder's space, a few at a time. Until a memory is
    /// re-indexed it is still found by keyword. Returns how many were updated.
    pub async fn reindex(&self, limit: usize) -> Result<usize> {
        let want = self.preferred_embedder();
        let mut done = 0;
        for (id, content) in self.store.memories_to_reindex(want, limit)? {
            let (emb, tag) = self.embed(&content).await;
            if tag != want {
                break; // the preferred embedder is unavailable right now; try again later
            }
            self.store.set_memory_embedding(id, &to_blob(&emb), tag)?;
            done += 1;
        }
        Ok(done)
    }

    /// Save a memory. Secrets are redacted. A restatement of a known fact refreshes it in place; a changed
    /// fact retires the old version (kept as history) and stores the new one. Facts taken from outside
    /// material are stored unconfirmed and can never replace something the user confirmed.
    pub async fn save(&self, content: &str, kind: &str, tags: &str, importance: f64, source: &str) -> Result<SaveOutcome> {
        self.save_replacing(content, kind, tags, importance, source, None).await
    }

    /// As `save`, with an explicit fact to retire (when the caller knows this corrects memory `replaces`).
    pub async fn save_replacing(&self, content: &str, kind: &str, tags: &str, importance: f64, source: &str, replaces: Option<i64>) -> Result<SaveOutcome> {
        let content = redact_secrets(content.trim());
        let importance = importance.clamp(0.0, 1.0);
        let confirmed = !OUTSIDE_SOURCES.contains(&source);
        let (emb, embedder) = self.embed(&content).await;
        // Meaning-based vectors put paraphrases close together, so "same fact" needs a stricter bar there.
        let threshold = if embedder == crate::embed::HASH_TAG { DEDUPE_THRESHOLD } else { 0.93 };
        let mut best: Option<(i64, f32)> = None;
        for (id, stored) in self.vectors(embedder, emb.len())? {
            let sim = cosine(&emb, &stored);
            if sim >= threshold && best.map(|b| sim > b.1).unwrap_or(true) {
                best = Some((id, sim));
            }
        }
        let target = replaces.or(best.map(|b| b.0));
        if let Some(old) = target.and_then(|id| self.store.get_memory(id).ok().flatten()) {
            let merged_tags = merge_tags(&old.tags, tags);
            let same = |s: &str| s.to_lowercase().chars().filter(|c| c.is_alphanumeric()).collect::<String>();
            if same(&old.content) == same(&content) {
                self.store.update_memory(old.id, &content, &merged_tags, old.importance.max(importance), Some(&to_blob(&emb)), old.confirmed || confirmed, embedder)?;
                return Ok(SaveOutcome::Updated(old.id));
            }
            if old.confirmed && !confirmed {
                // Outside material disagrees with something the user vouched for: keep both, retire nothing.
                let id = self.store.insert_memory(kind, &content, tags, importance, source, Some(&to_blob(&emb)), false, embedder)?;
                return Ok(SaveOutcome::Created(id));
            }
            let id = self.store.insert_memory(kind, &content, &merged_tags, old.importance.max(importance), source, Some(&to_blob(&emb)), confirmed, embedder)?;
            self.store.supersede_memory(old.id, id)?;
            return Ok(SaveOutcome::Updated(id));
        }
        let id = self.store.insert_memory(kind, &content, tags, importance, source, Some(&to_blob(&emb)), confirmed, embedder)?;
        Ok(SaveOutcome::Created(id))
    }

    /// Hybrid search. Returns the best `limit` memories with a relevance score.
    pub async fn search(&self, query: &str, limit: usize) -> Result<Vec<MemoryItem>> {
        let mut fused: HashMap<i64, f64> = HashMap::new();
        const K: f64 = 60.0;

        let tokens = tokenize(query);
        if !tokens.is_empty() {
            let expr = tokens.iter().map(|t| format!("\"{}\"", t.replace('"', ""))).collect::<Vec<_>>().join(" OR ");
            if let Ok(ids) = self.store.fts_memories(&expr, 40) {
                for (rank, id) in ids.iter().enumerate() {
                    *fused.entry(*id).or_default() += 1.0 / (K + rank as f64 + 1.0);
                }
            }
        }

        let (q, embedder) = self.embed(query).await;
        // Unrelated sentences score near zero with hashed vectors but around 0.1-0.2 with a neural model.
        let floor = if embedder == crate::embed::HASH_TAG { 0.12 } else { 0.25 };
        let mut sims: Vec<(i64, f32)> = self
            .vectors(embedder, q.len())?
            .into_iter()
            .map(|(id, stored)| (id, cosine(&q, &stored)))
            .filter(|(_, s)| *s > floor)
            .collect();
        sims.sort_by(|a, b| b.1.total_cmp(&a.1));
        for (rank, (id, _)) in sims.iter().take(40).enumerate() {
            *fused.entry(*id).or_default() += 1.0 / (K + rank as f64 + 1.0);
        }

        // A neural model understands meaning but not misspellings; the built-in character-based method is
        // the opposite. When the neural (or a remote) model is in use, add the built-in ranking as a third
        // voice so "acountant" still finds "accountant". Computed on the fly: it is cheap at personal scale.
        if embedder != crate::embed::HASH_TAG {
            let hq = local_embed(query);
            let mut typo: Vec<(i64, f32)> = self.store.list_memories(None, 3000)?.iter().map(|m| (m.id, cosine(&hq, &local_embed(&m.content)))).filter(|(_, s)| *s > 0.12).collect();
            typo.sort_by(|a, b| b.1.total_cmp(&a.1));
            for (rank, (id, _)) in typo.iter().take(40).enumerate() {
                *fused.entry(*id).or_default() += 1.0 / (K + rank as f64 + 1.0);
            }
        }

        let now = now_ts();
        let mut out = Vec::new();
        for (id, base) in fused {
            if let Some(mut m) = self.store.get_memory(id)? {
                // Whole days: a fact saved a second later is not "more recent" in any way that should
                // decide between two equally good matches (and would make rankings depend on timing).
                let age_days = ((now - m.updated_at).max(0) / 86_400) as f64;
                let recency = 1.0 + 0.25 * (-age_days / 30.0).exp();
                m.score = Some(base * (0.6 + 0.8 * m.importance) * recency);
                out.push(m);
            }
        }
        // Equal scores are ordered by id, so the same question always gives the same ranking.
        out.sort_by(|a, b| b.score.unwrap_or(0.0).total_cmp(&a.score.unwrap_or(0.0)).then(a.id.cmp(&b.id)));
        out.truncate(limit);
        self.store.touch_memories(&out.iter().map(|m| m.id).collect::<Vec<_>>())?;
        Ok(out)
    }

    /// Memories that should always be in context: who the user is and how they like things done.
    pub fn core(&self, limit: usize) -> Result<Vec<MemoryItem>> {
        // Unconfirmed facts never become part of who the user "is". They are left out before counting,
        // so a pile of them cannot crowd the user's own facts out of the result.
        let mut v = self.store.list_confirmed_memories(&["profile", "preference", "workflow"], limit)?;
        v.truncate(limit);
        Ok(v)
    }

    /// Ask the (fast) model to extract durable facts from the latest exchange and store them.
    /// Returns what was learned. Failures are non-fatal by design.
    pub async fn learn_from_exchange(&self, llm: &dyn Llm, model: &str, user: &str, assistant: &str) -> Result<Vec<String>> {
        if user.trim().len() < 12 {
            return Ok(vec![]);
        }
        let known = self.search(user, 6).await.unwrap_or_default();
        let known_text = known.iter().map(|m| format!("- (#{}) {}", m.id, m.content)).collect::<Vec<_>>().join("\n");
        let prompt = format!(
            "You maintain long-term memory for a personal assistant. From the exchange below, extract ONLY durable facts worth \
             remembering across future conversations: who the user is, stable preferences, ongoing projects, important people, \
             recurring commitments, corrections to how the assistant should behave, and workflows (the steps or format the user \
             wants for a kind of task they do repeatedly).\n\
             Do NOT extract: one-off requests, transient task details, anything already known, secrets/credentials, \
             facts stated only inside quoted/external content, or the fact that a reminder, task or scheduled job was \
             just set up (those are kept in their own lists).\n\n\
             Already known:\n{known_text}\n\n\
             <user>\n{}\n</user>\n<assistant>\n{}\n</assistant>\n\n\
             Reply with a JSON array only (possibly empty). Each item: \
             {{\"content\": \"self-contained sentence in third person\", \"kind\": \"profile|preference|workflow|project|person|fact\", \"importance\": 0.0-1.0}}. \
             If an item corrects or updates one of the already-known facts (the user changed their mind, moved, switched jobs...), \
             add \"replaces\": <that fact's number>.",
            crate::util::truncate_middle(user, 4000),
            crate::util::truncate_middle(assistant, 2000)
        );
        let resp = llm.chat(ChatRequest { messages: vec![Message::user(prompt)], tools: vec![], model: Some(model.to_string()), max_tokens: Some(600) }, &|_| {}).await?;
        let text = resp.message.text();
        let Some(start) = text.find('[') else { return Ok(vec![]) };
        let Some(end) = text.rfind(']') else { return Ok(vec![]) };
        let items: Vec<serde_json::Value> = serde_json::from_str(&text[start..=end]).unwrap_or_default();
        let mut learned = vec![];
        for it in items.iter().take(5) {
            let Some(content) = it["content"].as_str().filter(|c| c.len() > 8) else { continue };
            let kind = it["kind"].as_str().unwrap_or("fact");
            let kind = if ["profile", "preference", "workflow", "project", "person", "fact"].contains(&kind) { kind } else { "fact" };
            let imp = it["importance"].as_f64().unwrap_or(0.5);
            let replaces = it["replaces"].as_i64().filter(|id| known.iter().any(|m| m.id == *id));
            match self.save_replacing(content, kind, "", imp, "auto", replaces).await {
                Ok(SaveOutcome::Created(_)) => learned.push(content.to_string()),
                Ok(SaveOutcome::Updated(_)) if replaces.is_some() => learned.push(format!("{content} (updated)")),
                _ => {}
            }
        }
        Ok(learned)
    }
}

fn merge_tags(a: &str, b: &str) -> String {
    let mut tags: Vec<&str> = a.split(',').chain(b.split(',')).map(|t| t.trim()).filter(|t| !t.is_empty()).collect();
    tags.sort();
    tags.dedup();
    tags.join(",")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mem() -> Memory {
        Memory::new(Store::in_memory().unwrap(), None)
    }

    #[test]
    fn local_embeddings_capture_similarity() {
        let a = local_embed("The user's favorite programming language is Rust");
        let b = local_embed("favourite programming languages: rust");
        let c = local_embed("Dentist appointment next Tuesday afternoon");
        assert!(cosine(&a, &b) > cosine(&a, &c) + 0.2, "{} vs {}", cosine(&a, &b), cosine(&a, &c));
        assert!((cosine(&a, &a) - 1.0).abs() < 1e-4);
        assert_eq!(cosine(&a, &[1.0]), 0.0);
    }

    #[tokio::test]
    async fn search_ranks_relevant_memory_first() {
        let m = mem();
        m.save("The user is allergic to peanuts", "profile", "health", 0.9, "").await.unwrap();
        m.save("The user's sister Maria lives in Lisbon", "person", "family", 0.6, "").await.unwrap();
        m.save("Project Falcon ships its beta in March", "project", "work", 0.7, "").await.unwrap();
        let r = m.search("what food allergies should I know about? peanut", 3).await.unwrap();
        assert!(r[0].content.contains("peanuts"));
        let r = m.search("where does Maria live", 3).await.unwrap();
        assert!(r[0].content.contains("Lisbon"));
        // typo tolerance via character n-grams
        let r = m.search("projct falcn", 3).await.unwrap();
        assert!(r[0].content.contains("Falcon"));
    }

    #[tokio::test]
    async fn near_duplicates_update_instead_of_duplicating() {
        let m = mem();
        let a = m.save("The user prefers dark mode in all apps", "preference", "ui", 0.5, "").await.unwrap();
        let b = m.save("The user prefers dark mode in all apps.", "preference", "theme", 0.8, "").await.unwrap();
        assert!(matches!(a, SaveOutcome::Created(_)));
        assert_eq!(b, SaveOutcome::Updated(a.id()));
        let all = m.store.list_memories(None, 10).unwrap();
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].tags, "theme,ui");
        assert!((all[0].importance - 0.8).abs() < 1e-9);
    }

    #[tokio::test]
    async fn changed_facts_keep_history_and_only_the_current_one_is_recalled() {
        let m = mem();
        let old = m.save("The user lives in Lisbon and works from home", "profile", "", 0.8, "user").await.unwrap();
        // Offline embeddings recognise rewordings, not changed values, so a correction names what it replaces
        // (the model does this when it learns from a conversation, or through the `remember` tool).
        let new = m.save_replacing("The user lives in Porto and works from home", "profile", "", 0.8, "user", Some(old.id())).await.unwrap();
        assert!(matches!(new, SaveOutcome::Updated(id) if id != old.id()));
        let hits = m.search("where does the user live", 5).await.unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].content.contains("Porto"));
        assert_eq!(m.store.memory_history(new.id()).unwrap(), vec!["The user lives in Lisbon and works from home"]);
        assert_eq!(m.store.memory_count(), 1);
        // an explicit correction retires a fact even when the wording is very different
        let diet = m.save("The user is vegetarian", "profile", "", 0.8, "user").await.unwrap();
        let fixed = m.save_replacing("The user eats fish again since March", "profile", "", 0.8, "auto", Some(diet.id())).await.unwrap();
        assert!(m.search("vegetarian diet fish", 5).await.unwrap().iter().all(|h| h.id != diet.id()));
        assert_eq!(m.store.memory_history(fixed.id()).unwrap(), vec!["The user is vegetarian"]);
    }

    #[tokio::test]
    async fn outside_facts_stay_unconfirmed_and_cannot_overwrite_the_user() {
        let m = mem();
        let mine = m.save("The user's bank is Northwind Bank", "fact", "", 0.8, "user").await.unwrap();
        // an email claims otherwise (as a phishing or poisoning attempt would)
        let theirs = m.save("The user's bank is Evil Bank", "fact", "", 0.6, "email").await.unwrap();
        assert!(matches!(theirs, SaveOutcome::Created(_)));
        let all = m.store.list_memories(None, 10).unwrap();
        assert_eq!(all.len(), 2, "the user's own fact is not retired");
        assert!(all.iter().find(|x| x.id == mine.id()).unwrap().confirmed);
        let outside = all.iter().find(|x| x.id == theirs.id()).unwrap();
        assert!(!outside.confirmed);
        assert_eq!(outside.source, "email");
        // unconfirmed facts are never part of the always-present profile
        m.save("The user is a senior accountant at Acme", "profile", "", 0.9, "email").await.unwrap();
        assert!(m.core(10).unwrap().is_empty());
        // once the user confirms it, it counts
        let id = m.store.list_memories(Some("profile"), 5).unwrap()[0].id;
        assert!(m.store.confirm_memory(id).unwrap());
        assert_eq!(m.core(10).unwrap().len(), 1);
    }

    #[tokio::test]
    async fn vectors_from_different_embedders_are_never_mixed() {
        let m = mem();
        let id = m.save("The user keeps bees on the allotment", "fact", "", 0.6, "user").await.unwrap().id();
        assert_eq!(m.preferred_embedder(), crate::embed::HASH_TAG);
        // Pretend this memory was embedded by another model whose vectors happen to have the same length.
        let foreign: Vec<u8> = local_embed("completely different text about tax returns").iter().flat_map(|f| f.to_le_bytes()).collect();
        m.store.set_memory_embedding(id, &foreign, "some-other-model").unwrap();
        // It is ignored for similarity (no false neighbours)...
        assert!(m.store.memory_embeddings(crate::embed::HASH_TAG).unwrap().is_empty());
        assert!(m.search("tax returns", 3).await.unwrap().is_empty());
        // ...still found by keyword...
        assert_eq!(m.search("bees allotment", 3).await.unwrap()[0].id, id);
        // ...and re-indexing brings it back into the current space without touching its text.
        assert_eq!(m.reindex(10).await.unwrap(), 1);
        assert_eq!(m.reindex(10).await.unwrap(), 0);
        assert_eq!(m.search("beez alotment", 3).await.unwrap()[0].id, id, "typo-tolerant vector search works again");
    }

    #[tokio::test]
    async fn secrets_never_reach_memory() {
        let m = mem();
        m.save("The deploy token is ghp_abcdefghijklmnopqrstuvwxyz0123456789", "fact", "", 0.5, "").await.unwrap();
        let all = m.store.list_memories(None, 10).unwrap();
        assert!(all[0].content.contains("[REDACTED]"));
        assert!(!all[0].content.contains("ghp_"));
    }

    #[tokio::test]
    async fn auto_learning_parses_model_output() {
        use crate::llm::ScriptedLlm;
        let m = mem();
        let llm = ScriptedLlm::new(vec![Message::assistant(
            "Here you go:\n[{\"content\":\"The user works as a nurse on night shifts\",\"kind\":\"profile\",\"importance\":0.8},{\"content\":\"x\",\"kind\":\"weird\"}]",
        )]);
        let learned = m.learn_from_exchange(&llm, "fast", "I'm a nurse and I work night shifts, so mornings are bad for me", "Noted!").await.unwrap();
        assert_eq!(learned.len(), 1);
        assert_eq!(m.core(5).unwrap()[0].kind, "profile");
    }
}
