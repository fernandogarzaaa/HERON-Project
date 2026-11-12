//! Built-in evaluation of memory retrieval. Deterministic and offline: it measures the retrieval
//! layer (keyword + offline embeddings + ranking), not a model's reasoning. Run with `heron eval`.

use crate::memory::Memory;
use crate::store::Store;
use anyhow::Result;

const FACTS: &[(&str, &str)] = &[
    ("profile", "The user's name is Dana Okafor and she is a freelance landscape architect"),
    ("profile", "The user is allergic to shellfish and avoids restaurants that mostly serve seafood"),
    ("profile", "The user lives in Rotterdam in a third-floor apartment without a lift"),
    ("preference", "The user prefers meetings after 10 in the morning and never on Fridays"),
    ("preference", "The user wants invoices sent as PDF with a 30 day payment term"),
    ("preference", "The user likes replies short, in British English, with metric units"),
    ("preference", "The user drinks oat milk flat whites and dislikes sweetened drinks"),
    ("person", "Tomasz is the user's brother; he lives in Gdansk and visits every August"),
    ("person", "Priya Nair is the main contact at the client Greenline and prefers phone calls"),
    ("person", "The user's accountant is Mr Bakker, reachable on Tuesdays and Thursdays"),
    ("person", "Lena is the user's partner and works night shifts at the hospital"),
    ("person", "The user's dentist is Dr Lindqvist on Coolsingel street"),
    ("project", "The Greenline rooftop garden project is due for final drawings on 14 March"),
    ("project", "The user is writing a book about urban meadows with a draft deadline in June"),
    ("project", "The municipal park tender requires a soil report and three reference projects"),
    ("project", "The user's website is being rebuilt by a contractor named Studio Halve"),
    ("workflow", "For new client quotes the user first visits the site, then sends a two page estimate within three days"),
    ("workflow", "Every Sunday evening the user plans the week and moves unfinished tasks forward"),
    ("workflow", "Receipts are photographed and filed in the folder Finance/Receipts by month"),
    ("fact", "The user's car is a grey electric Renault that needs charging about twice a week"),
    ("fact", "The user's gym membership renews automatically on the first of every month"),
    ("fact", "The studio wifi network is called Meadowlark and the printer is on the second shelf"),
    ("fact", "The user's passport expires in November next year"),
    ("fact", "The user's bicycle was stolen last spring and the insurance claim is still open"),
    ("fact", "The user's mother has her birthday on 2 May and likes peonies"),
    ("fact", "The user takes a Dutch language class on Wednesday evenings"),
    ("fact", "The user's favourite plant nursery is Kwekerij De Linde near Delft"),
    ("fact", "The user keeps project photos on an external drive labelled Atlas"),
    ("fact", "The user runs ten kilometres on Saturday mornings along the river"),
    ("fact", "The user's landlord is the housing association Woonstad"),
];

/// (question, index of the fact that answers it). Worded differently from the facts on purpose.
const QUESTIONS: &[(&str, usize)] = &[
    ("what is her job", 0),
    ("any food allergies I should know about before booking dinner", 1),
    ("which city does she live in", 2),
    ("when is a good time to schedule a meeting", 3),
    ("how should invoices be formatted and what are the payment terms", 4),
    ("what style should replies be written in", 5),
    ("what coffee does she order", 6),
    ("who is Tomasz", 7),
    ("who do I contact at Greenline", 8),
    ("when can the accountant be reached", 9),
    ("what does Lena do for work", 10),
    ("where is the dentist", 11),
    ("when are the rooftop garden drawings due", 12),
    ("what is the book about", 13),
    ("what does the park tender need", 14),
    ("who is rebuilding the website", 15),
    ("how does she prepare a quote for a new client", 16),
    ("when does she plan her week", 17),
    ("where do receipts go", 18),
    ("how often does the car need charging", 19),
    ("when does the gym membership renew", 20),
    ("what is the wifi called", 21),
    ("when does the passport run out", 22),
    ("what happened to the bike", 23),
    ("mother's birthday", 24),
    ("which evening is the language class", 25),
    ("favourite nursery for plants", 26),
    ("where are the project photos stored", 27),
    ("when does she go running", 28),
    ("who is the landlord", 29),
    // misspellings and fragments
    ("acountant availabilty", 9),
    ("greenlne contact persn", 8),
    ("shelfish alergy", 1),
    ("pasport expiry", 22),
];

/// (index of the fact that changed, its replacement, a question, a word only the new answer contains, a word only the old one contains)
const UPDATES: &[(usize, &str, &str, &str, &str)] = &[
    (2, "The user moved to Utrecht and now lives in a ground-floor flat", "where does she live now", "Utrecht", "Rotterdam"),
    (12, "The Greenline rooftop garden final drawings are now due on 28 March", "rooftop garden drawings deadline", "28 March", "14 March"),
    (9, "The user's accountant is now Ms de Vries, reachable on Mondays", "who is the accountant", "de Vries", "Bakker"),
    (19, "The user sold the car and now uses a shared car subscription", "does she have a car", "shared car", "Renault"),
];

#[derive(Debug, Clone, PartialEq)]
pub struct Report {
    pub questions: usize,
    pub top1: usize,
    pub top3: usize,
    pub updates: usize,
    /// After a fact changed: the new version is the top answer.
    pub current_first: usize,
    /// After a fact changed: the retired version still shows up in the results.
    pub stale_leaks: usize,
    pub misses: Vec<String>,
}

impl Report {
    pub fn summary(&self) -> String {
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        format!(
            "Memory retrieval (offline embeddings, {} facts, {} questions worded differently from the facts)\n  right fact ranked first:      {}/{} ({:.0}%)\n  right fact in the top three:  {}/{} ({:.0}%)\nAfter {} facts changed\n  current version ranked first: {}/{}\n  retired version still shown:  {}/{}{}",
            FACTS.len(),
            self.questions,
            self.top1,
            self.questions,
            pct(self.top1, self.questions),
            self.top3,
            self.questions,
            pct(self.top3, self.questions),
            self.updates,
            self.current_first,
            self.updates,
            self.stale_leaks,
            self.updates,
            if self.misses.is_empty() { String::new() } else { format!("\nNot in the top three:\n  {}", self.misses.join("\n  ")) }
        )
    }
}

pub async fn run_memory_eval() -> Result<Report> {
    let store = Store::in_memory()?;
    let memory = Memory::new(store.clone(), None);
    let mut ids = vec![];
    for (kind, content) in FACTS {
        ids.push(memory.save(content, kind, "", 0.6, "user").await?.id());
    }
    let mut report = Report { questions: QUESTIONS.len(), top1: 0, top3: 0, updates: UPDATES.len(), current_first: 0, stale_leaks: 0, misses: vec![] };
    for (question, expected) in QUESTIONS {
        let hits = memory.search(question, 3).await?;
        let rank = hits.iter().position(|h| h.id == ids[*expected]);
        match rank {
            Some(0) => {
                report.top1 += 1;
                report.top3 += 1;
            }
            Some(_) => report.top3 += 1,
            None => report.misses.push(format!("\"{question}\"")),
        }
    }
    for (index, replacement, question, new_word, old_word) in UPDATES {
        memory.save_replacing(replacement, FACTS[*index].0, "", 0.6, "user", Some(ids[*index])).await?;
        let hits = memory.search(question, 3).await?;
        if hits.first().map(|h| h.content.contains(new_word)).unwrap_or(false) {
            report.current_first += 1;
        }
        if hits.iter().any(|h| h.content.contains(old_word)) {
            report.stale_leaks += 1;
        }
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn memory_retrieval_quality_does_not_regress() {
        let r = run_memory_eval().await.unwrap();
        println!("{}", r.summary());
        assert_eq!(r.stale_leaks, 0, "a retired fact must never be recalled");
        assert_eq!(r.current_first, r.updates);
        assert!(r.top3 * 100 >= r.questions * 85, "top-3 recall fell below 85%:\n{}", r.summary());
        assert!(r.top1 * 100 >= r.questions * 65, "top-1 recall fell below 65%:\n{}", r.summary());
    }
}
