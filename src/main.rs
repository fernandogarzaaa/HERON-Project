//! Heron command-line interface: interactive chat, one-shot runs, the local web UI,
//! the scheduler daemon, and management commands.

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use clap::{Parser, Subcommand};
use heron::agent::{Agent, Event, EventSink, Session};
use heron::app::App;
use heron::config::Config;
use heron::permissions::{Approval, ApprovalRequest, Approver, DenyAll, Mode};
use heron::util::fmt_ts;
use std::io::{IsTerminal, Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "heron", version, about = "Heron — a local-first personal AI agent", long_about = None)]
struct Cli {
    /// Folder to work in (default: current directory)
    #[arg(short = 'C', long, global = true)]
    workspace: Option<PathBuf>,
    /// Permission mode: readonly | ask | auto | yolo
    #[arg(long, global = true)]
    mode: Option<String>,
    /// Model to use for this run
    #[arg(long, global = true)]
    model: Option<String>,
    #[command(subcommand)]
    command: Option<Cmd>,
}

#[derive(Subcommand)]
enum Cmd {
    /// Interactive chat in the terminal (default)
    Chat {
        /// Continue the most recent conversation
        #[arg(short = 'c', long = "continue")]
        cont: bool,
        /// Resume a conversation by id (or unique prefix)
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Run a single request and print the answer (reads stdin if piped)
    Run {
        prompt: Vec<String>,
        #[arg(short, long)]
        session: Option<String>,
    },
    /// Start the app in your browser, with the scheduler running
    Serve {
        #[arg(long)]
        port: Option<u16>,
        /// Do not open the browser automatically
        #[arg(long)]
        no_open: bool,
    },
    /// Run only the scheduler (reminders and recurring jobs)
    Daemon,
    /// Create a starter config file
    Init,
    /// Check configuration and connectivity
    Doctor,
    /// Measure memory retrieval quality with the built-in test set
    Eval {
        /// Use the local neural model (needs a build with the `neural` feature; downloads it on first use)
        #[arg(long)]
        neural: bool,
    },
    /// List recent conversations
    Sessions,
    /// Manage long-term memory
    Memory {
        #[command(subcommand)]
        action: Option<MemoryCmd>,
    },
    /// List tasks
    Tasks,
    /// List reminders and scheduled jobs
    Schedules,
    /// Show results delivered by scheduled jobs
    Inbox,
    /// Show the audit log of tool actions
    Audit,
    /// Show how many tokens have been used, what is left and where the month is heading
    Usage,
    /// Serve Heron's memory and tasks to other apps over MCP (stdio)
    McpServe,
}

#[derive(Subcommand)]
enum MemoryCmd {
    List,
    Search { query: Vec<String> },
    Add { content: Vec<String> },
    Forget { id: i64 },
}

struct Style {
    on: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.on { format!("\x1b[{code}m{s}\x1b[0m") } else { s.to_string() }
    }
    fn dim(&self, s: &str) -> String {
        self.paint("2", s)
    }
    fn bold(&self, s: &str) -> String {
        self.paint("1", s)
    }
    fn red(&self, s: &str) -> String {
        self.paint("31", s)
    }
    fn yellow(&self, s: &str) -> String {
        self.paint("33", s)
    }
    fn blue(&self, s: &str) -> String {
        self.paint("34", s)
    }
}

fn style() -> Style {
    Style { on: std::io::stdout().is_terminal() && std::env::var_os("NO_COLOR").is_none() }
}

/// Asks on the terminal before risky actions.
struct TerminalApprover;

#[async_trait]
impl Approver for TerminalApprover {
    async fn approve(&self, req: &ApprovalRequest) -> Approval {
        if !std::io::stdin().is_terminal() {
            // Say the real reason, so the model passes on advice that will actually work.
            return Approval::Deny(Some(if req.tainted {
                "this conversation now contains content from outside (a web page, an email or a connected app), so changes need the user's own approval, and nobody is at the keyboard to give it. This is the same in every mode. Tell the user what you would have done and that they can approve it by asking again in `heron chat` or the Heron app".into()
            } else if req.risk.high() {
                "this is a risky action that always needs the user's own approval, and nobody is at the keyboard to give it. Tell the user they can approve it by asking again in `heron chat` or the Heron app".into()
            } else {
                "not running interactively, so nobody can approve this. Re-run with --mode auto to allow file edits and commands".into()
            }));
        }
        let st = style();
        let req = req.clone();
        tokio::task::spawn_blocking(move || {
            let head = if req.risk.high() { st.red("Heron wants to do something risky") } else { st.yellow("Heron wants permission") };
            eprintln!("\n{head} — {} [{}]", st.bold(&req.tool), req.risk.label());
            if req.tainted {
                eprintln!("  {}", st.yellow("Content from outside (a web page, email or connected app) is in this conversation and may have influenced this request."));
            }
            for line in req.detail.lines().take(30) {
                eprintln!("  {line}");
            }
            let can_always = !req.risk.high() && !req.tainted;
            loop {
                eprint!("{}", if can_always { "Allow? [y]es / [a]lways / [n]o: " } else { "Allow? [y]es / [n]o: " });
                let _ = std::io::stderr().flush();
                let mut line = String::new();
                if std::io::stdin().read_line(&mut line).unwrap_or(0) == 0 {
                    return Approval::Deny(None);
                }
                match line.trim().to_lowercase().as_str() {
                    "y" | "yes" => return Approval::Once,
                    "a" | "always" if can_always => return Approval::Always,
                    "n" | "no" | "" => return Approval::Deny(None),
                    other if other.len() > 3 => return Approval::Deny(Some(format!("the user declined and said: {other}"))),
                    _ => continue,
                }
            }
        })
        .await
        .unwrap_or(Approval::Deny(None))
    }
}

fn terminal_sink() -> EventSink {
    let st = style();
    let mid_line = Arc::new(AtomicBool::new(false));
    Arc::new(move |e| {
        let mut out = std::io::stdout().lock();
        let newline_if_needed = |out: &mut std::io::StdoutLock| {
            if mid_line.swap(false, Ordering::SeqCst) {
                let _ = writeln!(out);
            }
        };
        match e {
            Event::Text(t) => {
                let _ = write!(out, "{t}");
                mid_line.store(!t.ends_with('\n'), Ordering::SeqCst);
            }
            Event::ToolStart { name, summary, .. } => {
                newline_if_needed(&mut out);
                let _ = writeln!(out, "{}", st.dim(&format!("  ▸ {name} {summary}")));
            }
            Event::ToolEnd { ok, preview, name, .. } => {
                newline_if_needed(&mut out);
                if !ok {
                    let _ = writeln!(out, "{}", st.red(&format!("  ✗ {name}: {preview}")));
                }
            }
            Event::Notice(n) => {
                newline_if_needed(&mut out);
                let _ = writeln!(out, "{}", st.dim(&format!("  {n}")));
            }
            Event::Usage(_) => {}
            Event::Model { name, why } => {
                let _ = writeln!(out, "{}", st.dim(&format!("  model: {name} ({why})")));
            }
        }
        let _ = out.flush();
    })
}

async fn turn(app: &App, agent: &Agent, session: &mut Session, input: &str) -> Result<String> {
    let sink = terminal_sink();
    let result = tokio::select! {
        r = agent.run_turn(session, input, &sink) => r,
        _ = tokio::signal::ctrl_c() => {
            println!("\n{}", style().dim("  stopped"));
            return Ok(String::new());
        }
    };
    println!();
    app.persist_rules();
    let text = result?;
    let learned = agent.learn(session, input, &text).await;
    for l in learned {
        println!("{}", style().dim(&format!("  remembered: {l}")));
    }
    Ok(text)
}

fn print_inbox(app: &App) -> Result<()> {
    let unread = app.store.inbox(true, 20)?;
    if !unread.is_empty() {
        let st = style();
        println!("{}", st.bold(&format!("{} new result(s) from scheduled jobs:", unread.len())));
        for m in unread.iter().rev() {
            println!("{} {}\n{}\n", st.blue(&format!("[{}]", fmt_ts(m.ts))), st.bold(&m.title), m.body);
        }
        app.store.mark_inbox_read()?;
    }
    Ok(())
}

const HELP: &str = "Commands:
  /new               start a new conversation
  /sessions          list recent conversations        /resume <id>   switch to one
  /memory [query]    show or search memory            /forget <id>   delete a memory
  /tasks             show open tasks                  /schedules     show reminders
  /mode <m>          readonly | ask | auto | yolo     /undo          undo last file change
  /tokens            token usage this run             /compact       summarize older context now
  /exit              quit            (Ctrl-C stops a running reply, Ctrl-D exits)";

async fn repl(app: Arc<App>, mut session: Session) -> Result<()> {
    let st = style();
    let cfg = app.cfg();
    let agent = app.agent(Arc::new(TerminalApprover));
    println!("{} — {} · mode: {} · {}", st.bold(&cfg.agent.name), cfg.llm.model, app.policy.mode().name(), app.workspace.display());
    for s in &app.mcp_status {
        println!("{}", st.dim(&format!("  mcp {s}")));
    }
    if heron::llm::is_demo(&cfg) {
        println!("{}", st.yellow("Demo mode: canned replies, real actions. Set a model in ~/.heron/config.toml for real answers."));
    } else if cfg.api_key().is_none() && !cfg.llm.base_url.contains("localhost") && !cfg.llm.base_url.contains("127.0.0.1") {
        println!("{}", st.yellow(&format!("No API key found. Set {} (or run `heron doctor`).", cfg.llm.api_key_env)));
    }
    print_inbox(&app)?;
    println!("{}", st.dim("Type a request, or /help."));
    let mut rl = rustyline::DefaultEditor::new()?;
    let history = cfg.home.join("history.txt");
    let _ = rl.load_history(&history);
    loop {
        let line = match tokio::task::block_in_place(|| rl.readline("› ")) {
            Ok(l) => l,
            Err(rustyline::error::ReadlineError::Interrupted) => continue,
            Err(_) => break,
        };
        let input = line.trim();
        if input.is_empty() {
            continue;
        }
        let _ = rl.add_history_entry(input);
        if let Some(cmd) = input.strip_prefix('/') {
            let (name, arg) = cmd.split_once(' ').map(|(a, b)| (a, b.trim())).unwrap_or((cmd, ""));
            match name {
                "exit" | "quit" | "q" => break,
                "help" | "?" => println!("{HELP}"),
                "new" => {
                    session = Session::create(&app.store, "")?;
                    println!("{}", st.dim("  new conversation"));
                }
                "sessions" => list_sessions(&app)?,
                "resume" => match Session::load(&app.store, arg) {
                    Ok(s) => {
                        println!("{}", st.dim(&format!("  resumed {} ({} messages)", s.id, s.messages.len())));
                        session = s;
                    }
                    Err(e) => println!("{}", st.red(&e.to_string())),
                },
                "memory" => memory_cmd(&app, if arg.is_empty() { MemoryCmd::List } else { MemoryCmd::Search { query: vec![arg.to_string()] } }).await?,
                "forget" => match arg.trim_start_matches('#').parse() {
                    Ok(id) => memory_cmd(&app, MemoryCmd::Forget { id }).await?,
                    Err(_) => println!("usage: /forget <id>"),
                },
                "tasks" => list_tasks(&app)?,
                "schedules" => list_schedules(&app)?,
                "mode" => match Mode::parse(arg) {
                    Ok(m) => {
                        app.policy.set_mode(m);
                        println!("{}", st.dim(&format!("  mode: {}", m.name())));
                    }
                    Err(e) => println!("{}", st.red(&e.to_string())),
                },
                "undo" => match heron::tools::fs::undo_last(&agent.tool_ctx(&session.id)) {
                    Ok(Some(what)) => println!("{}", st.dim(&format!("  undone: {what}"))),
                    Ok(None) => println!("{}", st.dim("  nothing to undo")),
                    // A refusal (the file has changed since) is an answer, not a reason to end the conversation.
                    Err(e) => println!("{}", st.red(&format!("  not undone: {e:#}"))),
                },
                "tokens" => {
                    let u = app.usage.lock().unwrap().clone();
                    println!("{}", st.dim(&format!("  {} in / {} out · context ≈ {} tokens", u.prompt_tokens, u.completion_tokens, session.tokens())));
                }
                "compact" => {
                    let mut tight = agent.clone();
                    let mut c = (*tight.cfg).clone();
                    c.agent.context_tokens = 0;
                    tight.cfg = Arc::new(c);
                    tight.compact_if_needed(&mut session, &terminal_sink()).await;
                }
                other => println!("unknown command /{other} — try /help"),
            }
            continue;
        }
        if let Err(e) = turn(&app, &agent, &mut session, input).await {
            println!("{}", st.red(&format!("error: {e:#}")));
        }
    }
    let _ = rl.save_history(&history);
    Ok(())
}

fn list_sessions(app: &App) -> Result<()> {
    for s in app.store.list_sessions(20)?.iter().filter(|s| s.messages > 0) {
        println!("{}  {}  {:>3} msgs  {}", s.id, fmt_ts(s.updated_at), s.messages, s.title);
    }
    Ok(())
}

fn list_tasks(app: &App) -> Result<()> {
    let tasks = app.store.list_tasks(None)?;
    if tasks.is_empty() {
        println!("no tasks");
    }
    for t in tasks {
        println!("#{:<3} [{}] P{} {}{}", t.id, t.status, t.priority, t.title, t.due.map(|d| format!("  (due {d})")).unwrap_or_default());
    }
    Ok(())
}

fn list_schedules(app: &App) -> Result<()> {
    let all = app.store.list_schedules()?;
    if all.is_empty() {
        println!("nothing scheduled");
    }
    for s in all {
        let kind = if !s.enabled { "finished".to_string() } else if s.cron.is_empty() { "once".to_string() } else { s.cron.clone() };
        println!("#{:<3} {:<12} next {}  {} — {}", s.id, kind, s.next_run.map(fmt_ts).unwrap_or_else(|| "-".into()), s.name, heron::util::preview(&s.prompt, 60));
    }
    Ok(())
}

async fn memory_cmd(app: &App, cmd: MemoryCmd) -> Result<()> {
    match cmd {
        MemoryCmd::List => {
            let all = app.store.list_memories(None, 200)?;
            if all.is_empty() {
                println!("no memories yet");
            }
            for m in all {
                println!("#{:<4} [{}] {}", m.id, m.kind, m.content);
            }
        }
        MemoryCmd::Search { query } => {
            for m in app.memory.search(&query.join(" "), 10).await? {
                println!("#{:<4} [{}] {}", m.id, m.kind, m.content);
            }
        }
        MemoryCmd::Add { content } => {
            let out = app.memory.save(&content.join(" "), "fact", "", 0.7, "user").await?;
            println!("saved as #{}", out.id());
        }
        MemoryCmd::Forget { id } => println!("{}", if app.store.delete_memory(id)? { "forgotten" } else { "no such memory" }),
    }
    Ok(())
}

async fn doctor(cfg: &Config) -> Result<()> {
    let st = style();
    let ok = |b: bool| if b { st.paint("32", "ok  ") } else { st.red("FAIL") };
    println!("Heron {}", env!("CARGO_PKG_VERSION"));
    println!("{} home        {}", ok(cfg.ensure_home().is_ok()), cfg.home.display());
    let config_path = cfg.home.join("config.toml");
    println!("{} config      {}", ok(true), if config_path.exists() { config_path.display().to_string() } else { "defaults (run `heron init` to create a config file)".into() });
    println!("{} database    {}", ok(heron::store::Store::open(&cfg.db_path()).is_ok()), cfg.db_path().display());
    println!("     endpoint    {}", cfg.llm.base_url);
    println!("     model       {}", cfg.llm.model);
    let local = cfg.llm.base_url.contains("localhost") || cfg.llm.base_url.contains("127.0.0.1");
    let has_key = cfg.api_key().is_some();
    let demo = heron::llm::is_demo(cfg);
    println!("{} api key     {}", ok(has_key || local || demo), if demo { "not needed in demo mode".to_string() } else if has_key { "found".to_string() } else if local { "not needed for a local endpoint".into() } else { format!("missing — set {} or save one in the app's Settings", cfg.llm.api_key_env) });
    match heron::browser::find(cfg) {
        Some(path) => println!("{} browser     {} (Heron can open pages, click and fill in forms)", ok(true), path.display()),
        None if cfg.agent.browser.eq_ignore_ascii_case("off") => println!("     browser     switched off in the config file"),
        None => println!("     browser     none found — install Chrome, Edge, Chromium or Brave to let Heron use websites"),
    }
    use heron::llm::{ChatRequest, Message};
    let mut quick = cfg.clone();
    quick.llm.max_retries = 0;
    quick.llm.request_timeout_secs = 30;
    let llm = heron::llm::build(&quick)?;
    match llm.chat(ChatRequest { messages: vec![Message::user("Reply with the single word: ok")], tools: vec![], model: None, max_tokens: Some(16) }, &|_| {}).await {
        Ok(r) => println!("{} model call  replied \"{}\"", ok(true), heron::util::preview(r.message.text(), 40)),
        Err(e) => println!("{} model call  {}", ok(false), heron::util::preview(&format!("{e:#}"), 300)),
    }
    Ok(())
}

#[tokio::main]
async fn main() {
    let outcome = real_main().await;
    heron::browser::shutdown().await; // never leave a browser running behind us
    if let Err(e) = outcome {
        eprintln!("{}", style().red(&format!("error: {e:#}")));
        std::process::exit(1);
    }
}

async fn real_main() -> Result<()> {
    let cli = Cli::parse();
    let mut cfg = Config::load()?;
    if let Some(m) = &cli.mode {
        Mode::parse(m)?;
        cfg.permissions.mode = m.clone();
    }
    if let Some(m) = &cli.model {
        cfg.llm.model = m.clone();
    }
    let command = cli.command.unwrap_or(Cmd::Chat { cont: false, session: None });

    match &command {
        Cmd::Init => {
            let path = cfg.write_default()?;
            println!("Config: {}\nSet your key with e.g. `export {}=...`, then run `heron`.", path.display(), cfg.llm.api_key_env);
            return Ok(());
        }
        Cmd::Doctor => return doctor(&cfg).await,
        Cmd::Eval { neural } => {
            if *neural && !heron::embed::neural_built_in() {
                bail!("this build does not include the neural memory model; build with `cargo build --features neural`");
            }
            let dir = if *neural { Some(cfg.home.join("models")) } else { None };
            println!("{}", heron::eval::run_memory_eval_with(dir).await?.summary());
            return Ok(());
        }
        _ => {}
    }

    let uses_app_folder = matches!(command, Cmd::Serve { .. } | Cmd::Daemon | Cmd::McpServe);
    let workspace = match cli.workspace {
        Some(w) => w,
        None if uses_app_folder && !cfg.agent.workspace.is_empty() => cfg.app_workspace(),
        None => std::env::current_dir().context("cannot determine current directory")?,
    };
    if matches!(command, Cmd::McpServe) {
        cfg.mcp.servers.clear(); // acting as a server: do not spawn our own MCP clients
    }
    let app = Arc::new(App::build(cfg, workspace).await?);

    match command {
        Cmd::Chat { cont, session } => {
            let session = match (session, cont) {
                (Some(id), _) => Session::load(&app.store, &id)?,
                (None, true) => match app.store.last_conversation() {
                    Some(id) => Session::load(&app.store, &id)?,
                    None => Session::create(&app.store, "")?,
                },
                _ => Session::create(&app.store, "")?,
            };
            repl(app, session).await
        }
        Cmd::Run { prompt, session } => {
            let mut text = prompt.join(" ");
            if !std::io::stdin().is_terminal() {
                let mut piped = String::new();
                std::io::stdin().read_to_string(&mut piped)?;
                if !piped.trim().is_empty() {
                    text = if text.is_empty() { piped } else { format!("{text}\n\n<stdin>\n{piped}\n</stdin>") };
                }
            }
            if text.trim().is_empty() {
                bail!("nothing to do: pass a prompt, e.g. `heron run \"summarize README.md\"`");
            }
            let mut session = match session {
                Some(id) => Session::load(&app.store, &id)?,
                None => Session::create(&app.store, "")?,
            };
            let agent = app.agent(Arc::new(TerminalApprover));
            turn(&app, &agent, &mut session, &text).await.map(|_| ())
        }
        Cmd::Serve { port, no_open } => {
            let cfg = app.cfg();
            let (url, handle) = heron::server::start(app.clone(), &cfg.server.bind, port.unwrap_or(cfg.server.port)).await?;
            println!("Heron is running at:\n\n  {url}\n\nKeep this window open. Press Ctrl-C to quit.");
            if !no_open {
                let _ = open::that(&url);
            }
            tokio::select! {
                r = handle => r?,
                _ = tokio::signal::ctrl_c() => Ok(()),
            }
        }
        Cmd::Daemon => {
            app.own_background_work(); // before anything runs; the loop says so if another Heron already does
            println!("Heron scheduler running (jobs use '{}' mode). Ctrl-C to stop.", app.cfg().permissions.daemon_mode);
            tokio::spawn(app.clone().observer_loop());
            if heron::telegram::spawn_if_configured(&app) {
                println!("Phone connection (Telegram) is on.");
            }
            tokio::select! {
                _ = app.clone().scheduler_loop() => Ok(()),
                _ = tokio::signal::ctrl_c() => Ok(()),
            }
        }
        Cmd::Sessions => list_sessions(&app),
        Cmd::Memory { action } => memory_cmd(&app, action.unwrap_or(MemoryCmd::List)).await,
        Cmd::Tasks => list_tasks(&app),
        Cmd::Schedules => list_schedules(&app),
        Cmd::Inbox => {
            let all = app.store.inbox(false, 20)?;
            if all.is_empty() {
                println!("inbox is empty");
            }
            for m in all.iter().rev() {
                println!("[{}] {}\n{}\n", fmt_ts(m.ts), m.title, m.body);
            }
            app.store.mark_inbox_read()?;
            Ok(())
        }
        Cmd::Usage => {
            let r = heron::usage::report(&app.store, &app.cfg(), app.background_budget().0);
            let n = |v: u64| { let s = v.to_string(); let mut out = String::new(); for (i, c) in s.chars().enumerate() { if i > 0 && (s.len() - i) % 3 == 0 { out.push(','); } out.push(c); } out };
            let window = |name: &str, w: &heron::usage::Window| match w.left {
                Some(left) => println!("{name:<12} {} used of {} ({} left)", n(w.used), n(w.limit), n(left)),
                None => println!("{name:<12} {} used (no limit set)", n(w.used)),
            };
            window("Today", &r.budget.today);
            window("This month", &r.budget.month);
            println!("{:<12} about {} a day; this month is heading for about {}", "Pace", n(r.budget.daily_average), n(r.budget.projected_month));
            if let Some(day) = &r.budget.limit_reached_on {
                println!("{:<12} at this pace the monthly limit is reached on {day}", "");
            }
            println!("{:<12} {}", "Effort", r.level_label);
            println!("{:<12} {}", "Routing", if r.routing { format!("simple requests go to {}, the rest to {}", r.fast_model, r.main_model) } else { format!("everything goes to {}", r.main_model) });
            if !r.models.is_empty() {
                println!("\nThis month by model:");
                for (model, tin, tout, calls) in &r.models {
                    println!("  {model:<44} {:>12} in  {:>10} out  {:>5} calls", n(*tin), n(*tout), calls);
                }
            }
            println!("\nSet limits in the app (Usage), or daily_token_limit / monthly_token_limit under [agent] in the config file.");
            Ok(())
        }
        Cmd::Audit => {
            let (checked, altered) = app.store.verify_audit()?;
            match altered {
                None => println!("Activity log intact: {checked} entries verified.\n"),
                Some(id) => println!("WARNING: the activity log was altered at or before entry {id} ({checked} entries verified before it).\n"),
            }
            for a in app.store.audit_log(50)?.iter().rev() {
                println!("{}  {:<16} {:<16} {}", fmt_ts(a.ts), a.decision, a.tool, a.args);
            }
            Ok(())
        }
        Cmd::McpServe => {
            let agent = app.agent(Arc::new(DenyAll));
            let ctx = agent.tool_ctx("mcp");
            heron::mcp::serve(tokio::io::BufReader::new(tokio::io::stdin()), tokio::io::stdout(), &app.tools, &ctx, &app.policy).await
        }
        Cmd::Init | Cmd::Doctor | Cmd::Eval { .. } => unreachable!(),
    }
}
