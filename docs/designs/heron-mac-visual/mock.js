// Heron Mac app mockups: shared chrome.
// URL parameters: theme=light|dark, palette=shore|dusk|graphite,
// accent=system|slate|lagoon|reed|rose, shot=1 (hide the viewer strip), reduce=1 (reduced motion).
(function () {
  const q = new URLSearchParams(location.search);
  const root = document.documentElement;
  // Screenshots (shot=1) ignore saved viewer choices so every capture is deterministic.
  const read = (k) => { if (q.get("shot")) return q.get(k); try { return q.get(k) || localStorage.getItem("heron-mock-" + k); } catch (e) { return q.get(k); } };
  const theme = read("theme"), palette = read("palette"), accent = read("accent");
  if (theme === "light" || theme === "dark") root.dataset.theme = theme;
  if (palette && palette !== "shore") root.dataset.palette = palette;
  if (accent && accent !== "slate") root.dataset.accent = accent;
  if (q.get("shot")) root.classList.add("shot");
  if (q.get("reduce")) root.classList.add("reduce");

  // SF Symbol stand-ins. The Swift app uses the named SF Symbol, never these paths.
  const P = {
    "sun.max": '<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M2 12h2M20 12h2M4.9 4.9l1.4 1.4M17.7 17.7l1.4 1.4M4.9 19.1l1.4-1.4M17.7 6.3l1.4-1.4"/>',
    "bubble.left": '<path d="M4 5h16v11H9l-5 4z"/>',
    "tray": '<path d="M3 13l3-8h12l3 8v6H3z"/><path d="M3 13h5l1 2h6l1-2h5"/>',
    "brain": '<path d="M9 4a3 3 0 0 0-3 3 3 3 0 0 0-2 5 3 3 0 0 0 2 5 3 3 0 0 0 6 1V5a2 2 0 0 0-3-1zM15 4a3 3 0 0 1 3 3 3 3 0 0 1 2 5 3 3 0 0 1-2 5 3 3 0 0 1-6 1"/>',
    "checklist": '<path d="M4 6l1.5 1.5L8 5M4 12l1.5 1.5L8 11M4 18l1.5 1.5L8 17M11 6h9M11 12h9M11 18h9"/>',
    "clock": '<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>',
    "list.bullet.rectangle": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9h.01M7 13h.01M7 17h.01M10 9h7M10 13h7M10 17h7"/>',
    "gearshape": '<circle cx="12" cy="12" r="3"/><path d="M12 2l1.6 2.6 3-.6.9 2.9 2.9.9-.6 3L22 12l-2.6 1.6.6 3-2.9.9-.9 2.9-3-.6L12 22l-1.6-2.6-3 .6-.9-2.9-2.9-.9.6-3L2 12l2.6-1.6-.6-3 2.9-.9.9-2.9 3 .6z"/>',
    "hand.raised": '<path d="M8 13V6a1.5 1.5 0 0 1 3 0v5M11 11V4.5a1.5 1.5 0 0 1 3 0V11M14 11V6a1.5 1.5 0 0 1 3 0v8a7 7 0 0 1-7 7c-3 0-4.5-2-6-5l-1-2a1.5 1.5 0 0 1 2.5-1.5L8 15"/>',
    "pause.circle": '<circle cx="12" cy="12" r="9"/><path d="M10 9v6M14 9v6"/>',
    "square.and.pencil": '<path d="M12 4H5a1 1 0 0 0-1 1v14a1 1 0 0 0 1 1h14a1 1 0 0 0 1-1v-7"/><path d="M17 3l4 4-9 9H8v-4z"/>',
    "magnifyingglass": '<circle cx="11" cy="11" r="6"/><path d="M20 20l-4.5-4.5"/>',
    "globe": '<circle cx="12" cy="12" r="9"/><path d="M3 12h18M12 3c3 3.5 3 14.5 0 18M12 3c-3 3.5-3 14.5 0 18"/>',
    "lock.shield": '<path d="M12 3l8 3v6c0 5-3.5 8-8 9-4.5-1-8-4-8-9V6z"/><rect x="9" y="11" width="6" height="5" rx="1"/><path d="M10 11V9.5a2 2 0 0 1 4 0V11"/>',
    "exclamationmark.triangle": '<path d="M12 3l10 18H2z"/><path d="M12 10v5M12 18h.01"/>',
    "checkmark.circle": '<circle cx="12" cy="12" r="9"/><path d="M8 12l3 3 5-6"/>',
    "xmark.circle": '<circle cx="12" cy="12" r="9"/><path d="M9 9l6 6M15 9l-6 6"/>',
    "folder": '<path d="M3 6a1 1 0 0 1 1-1h5l2 2h9a1 1 0 0 1 1 1v10a1 1 0 0 1-1 1H4a1 1 0 0 1-1-1z"/>',
    "terminal": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M7 9l3 3-3 3M12 15h5"/>',
    "envelope": '<rect x="3" y="5" width="18" height="14" rx="2"/><path d="M3 7l9 6 9-6"/>',
    "calendar": '<rect x="3" y="5" width="18" height="16" rx="2"/><path d="M3 10h18M8 3v4M16 3v4"/>',
    "doc.text": '<path d="M6 3h8l4 4v14H6z"/><path d="M14 3v4h4M9 12h6M9 16h6"/>',
    "arrow.up": '<path d="M12 19V5M6 11l6-6 6 6"/>',
    "stop.fill": '<rect x="7" y="7" width="10" height="10" rx="1.5" fill="currentColor"/>',
    "arrow.clockwise": '<path d="M20 12a8 8 0 1 1-2.3-5.7M20 4v5h-5"/>',
    "paintpalette": '<path d="M12 3a9 9 0 1 0 0 18c1.5 0 2-1 2-2s-1-1.5-1-2.5 1-1.5 2-1.5h3a3 3 0 0 0 3-3c0-5-4-9-9-9z"/><circle cx="7.5" cy="11" r="1"/><circle cx="10" cy="7" r="1"/><circle cx="15" cy="7.5" r="1"/>',
    "cpu": '<rect x="6" y="6" width="12" height="12" rx="2"/><path d="M10 10h4v4h-4zM9 3v3M15 3v3M9 18v3M15 18v3M3 9h3M3 15h3M18 9h3M18 15h3"/>',
    "link": '<path d="M10 14a4 4 0 0 0 5.7 0l3-3a4 4 0 0 0-5.7-5.7l-1 1M14 10a4 4 0 0 0-5.7 0l-3 3a4 4 0 0 0 5.7 5.7l1-1"/>',
    "chart.bar": '<path d="M5 20V12M10 20V6M15 20v-9M20 20V4"/>',
    "person.crop.circle": '<circle cx="12" cy="12" r="9"/><circle cx="12" cy="10" r="3"/><path d="M6.5 18a6 6 0 0 1 11 0"/>',
    "sparkles": '<path d="M12 3l1.8 4.7L18.5 9.5 13.8 11.3 12 16l-1.8-4.7L5.5 9.5l4.7-1.8zM18 15l.8 2 2 .8-2 .8-.8 2-.8-2-2-.8 2-.8z"/>',
    "arrow.uturn.backward": '<path d="M9 14L4 9l5-5"/><path d="M4 9h10a6 6 0 0 1 0 12h-3"/>',
    "xmark": '<path d="M6 6l12 12M18 6L6 18"/>',
    "play.fill": '<path d="M8 5l11 7-11 7z" fill="currentColor"/>',
    "bell": '<path d="M6 16V11a6 6 0 0 1 12 0v5l2 2H4zM10 20a2 2 0 0 0 4 0"/>',
    "iphone": '<rect x="7" y="2" width="10" height="20" rx="2"/><path d="M11 18h2"/>',
    "plus": '<path d="M12 5v14M5 12h14"/>',
    "ellipsis.circle": '<circle cx="12" cy="12" r="9"/><path d="M8 12h.01M12 12h.01M16 12h.01"/>',
    "line.3.horizontal.decrease": '<path d="M4 7h16M7 12h10M10 17h4"/>',
    "sidebar.left": '<rect x="3" y="4" width="18" height="16" rx="2"/><path d="M9 4v16"/>',
    "quote.bubble": '<path d="M4 5h16v11H9l-5 4z"/><path d="M9 9v2M13 9v2"/>',
    "power": '<path d="M12 3v8M7 6a7 7 0 1 0 10 0"/>',
    "arrow.up.right.square": '<rect x="3" y="3" width="18" height="18" rx="2"/><path d="M10 14l7-7M11 7h6v6"/>'
  };
  const icon = (n, label) => `<svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" ${label ? `role="img" aria-label="${label}"` : 'aria-hidden="true"'} data-symbol="${n}">${P[n] || P["sparkles"]}</svg>`;
  window.icon = icon;

  // Avatar styles: ripple (default), heron, monogram, image.
  const BADGE = { thinking: "", speaking: "", idle: "", awaitingApproval: "hand.raised", error: "exclamationmark.triangle" };
  const LABEL = { idle: "Idle", thinking: "Thinking", awaitingApproval: "Waiting for your approval", speaking: "Replying", error: "Something went wrong" };
  window.avatar = function (style, state, size, name) {
    style = style || "ripple"; state = state || "idle"; size = size || 36; name = name || "Heron";
    let art;
    if (style === "ripple") {
      art = `<svg viewBox="0 0 40 40"><circle class="ring r1" cx="20" cy="20" r="18"/><circle class="ring r2" cx="20" cy="20" r="13" opacity=".7"/><circle class="ring r3" cx="20" cy="20" r="${state === "thinking" ? 18 : 8}" opacity=".45"/><circle class="core" cx="20" cy="20" r="5"/></svg>`;
    } else if (style === "heron") {
      art = `<svg viewBox="0 0 40 40"><circle class="disc" cx="20" cy="20" r="19"/><circle class="ring r1" cx="20" cy="20" r="19" opacity=".35"/><path class="glyph" d="M13 33c2-6 6-8 6-13s-4-6-3-10c1-3 5-3 6 0l7 2-7 1"/><circle class="core" cx="19.2" cy="11" r="1.2"/></svg>`;
    } else if (style === "monogram") {
      art = `<svg viewBox="0 0 40 40"><circle class="disc" cx="20" cy="20" r="19"/><circle class="ring r1" cx="20" cy="20" r="19" opacity=".35"/><text class="mono-l" x="20" y="27" text-anchor="middle" font-size="20">${name[0]}</text></svg>`;
    } else {
      art = `<svg viewBox="0 0 40 40"><defs><clipPath id="c${size}${state}"><circle cx="20" cy="20" r="17"/></clipPath></defs><g clip-path="url(#c${size}${state})"><rect width="40" height="40" fill="#9DB4C6"/><path d="M0 28c8-6 14-2 20-6s14-2 20 2v16H0z" fill="#5E7C93"/><circle cx="28" cy="12" r="4" fill="#F2E3B8"/></g><circle class="ring r1" cx="20" cy="20" r="19"/></svg>`;
    }
    const b = BADGE[state] ? `<span class="badge">${icon(BADGE[state])}</span>` : "";
    return `<span class="av" data-style="${style}" data-state="${state}" style="--size:${size}px" role="img" aria-label="${name}: ${LABEL[state]}">${art}${b}</span>`;
  };

  // Sidebar.
  window.sidebar = function (active, opts) {
    opts = opts || {};
    const items = [["today", "Today", "sun.max"], ["chat", "Chat", "bubble.left"], ["inbox", "Inbox", "tray", opts.inbox ?? 3, true], ["memory", "Memory", "brain", opts.memory ?? 2], ["tasks", "Tasks", "checklist", opts.tasks ?? 4], ["schedules", "Schedules", "clock"], ["activity", "Activity", "list.bullet.rectangle"]];
    const nav = items.map(([id, label, ic, n, wait]) => `<a href="${id === "activity" ? "#" : id + ".html"}" ${id === "activity" ? `title="Not mocked: Activity reuses the Schedules list pattern"` : ""} ${id === active ? 'aria-current="page"' : ""}>${icon(ic)}<span>${label}</span>${n ? `<span class="count${wait ? " waiting" : ""}">${n}</span>` : "<span></span>"}</a>`).join("");
    const recent = (opts.recent || ["Prep · Tue 18 Nov", "Invoice for Northwind", "Flights to Lisbon", "Summarise the Q4 notes"]).map((r, i) => `<a href="chat.html">${r}</a>`).join("");
    return `<aside class="sidebar" aria-label="Sidebar">
      <div class="agent">${avatar(opts.style, opts.state || "idle", 36, opts.name)}<div><b>${opts.name || "Heron"}</b><span>${opts.status || "Ready"}</span></div></div>
      <nav class="nav">${nav}</nav>
      <div class="side-h">Conversations</div><div class="recent">${recent}</div>
      <div class="side-foot"><span><span class="dot${opts.coreOff ? " off" : ""}"></span>${opts.foot1 || "Claude Code 2.1.4"}</span><span>${opts.foot2 || "Sandbox on · Ask first"}</span></div>
    </aside>`;
  };

  window.toolbar = function (title, extra) {
    return `<header class="toolbar"><button class="tb" aria-label="Hide sidebar">${icon("sidebar.left")}</button><div class="title"><b>${title}</b><span>Research preview</span></div><div class="spacer"></div>${extra || ""}
      <button class="tb boxed" aria-label="Permission mode: Ask first">${icon("hand.raised")}Ask first</button>
      <button class="tb" aria-label="Pause Heron">${icon("pause.circle")}</button>
      <button class="tb" aria-label="New conversation">${icon("square.and.pencil")}</button></header>`;
  };

  // Viewer strip.
  const pages = [["index", "Overview"], ["first-run", "First run"], ["today", "Today"], ["chat", "Chat + approval"], ["inbox", "Inbox"], ["memory", "Memory"], ["tasks", "Tasks"], ["schedules", "Schedules"], ["settings", "Settings"], ["menubar", "Menu bar"], ["avatar", "Avatar + themes"], ["states", "States"]];
  const here = (location.pathname.split("/").pop() || "index.html").replace(".html", "");
  window.addEventListener("DOMContentLoaded", () => {
    document.querySelectorAll("[data-icon]").forEach((el) => { el.insertAdjacentHTML("afterbegin", icon(el.dataset.icon)); });
    if (document.querySelector(".viewer")) return;
    const sel = (k, opts, cur) => `<label>${k} <select id="v-${k}" data-k="${k}">${opts.map((o) => `<option ${o === cur ? "selected" : ""}>${o}</option>`).join("")}</select></label>`;
    const strip = document.createElement("div");
    strip.className = "viewer";
    strip.innerHTML = `<b>Heron for Mac</b>${pages.map(([p, l]) => `<a href="${p}.html" ${p === here ? 'aria-current="page"' : ""}>${l}</a>`).join("")}<span class="grow"></span>
      ${sel("theme", ["system", "light", "dark"], theme || "system")}${sel("palette", ["shore", "dusk", "graphite"], palette || "shore")}${sel("accent", ["slate", "system", "lagoon", "reed", "rose"], accent || "slate")}`;
    document.body.prepend(strip);
    strip.querySelectorAll("select").forEach((s) => s.addEventListener("change", () => {
      const k = s.dataset.k, v = s.value;
      try { localStorage.setItem("heron-mock-" + k, v); } catch (e) {}
      const map = { theme: "theme", palette: "palette", accent: "accent" };
      if (k === "theme") { if (v === "system") delete root.dataset.theme; else root.dataset.theme = v; }
      else if ((k === "palette" && v === "shore") || (k === "accent" && v === "slate")) delete root.dataset[map[k]];
      else root.dataset[map[k]] = v;
    }));
  });
})();
