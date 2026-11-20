// The library page: reads the paperloom-assets indexes in the browser, so it
// is always as new as the repository. No tracking, nothing is stored.
(() => {
  "use strict";
  const BASES = [
    "https://raw.githubusercontent.com/theamanullahdev/paperloom-assets/main/",
    "https://theamanullahdev.github.io/paperloom-assets/",
  ];
  const REPO = "https://github.com/theamanullahdev/paperloom-assets/tree/main/";
  const KIND_NAME = { look: "Widget look", stack: "Stack of widgets", theme: "App theme" };
  const WHERE = {
    look: "Open Paperloom → Widgets → Looks → Online library, then press Add on this look.",
    stack: "Open Paperloom → Widgets → Stacks → Online, then press Add on this stack.",
    theme: "Open Paperloom → Settings → Appearance → Download, then press Add on this theme.",
  };

  const $ = (id) => document.getElementById(id);
  const grid = $("grid"), state = $("state"), dialog = $("detail");
  let items = [], base = BASES[0], kind = "all", query = "";

  async function getJson(path) {
    let last = null;
    for (const b of BASES) {
      try {
        const res = await fetch(b + path, { cache: "no-cache" });
        if (res.ok) { base = b; return await res.json(); }
        last = new Error(res.status + "");
      } catch (err) { last = err; }
    }
    throw last;
  }

  const folderPath = (item) => (item.kind === "theme" ? "themes/" : item.kind === "stack" ? "widgets/stacks/" : "widgets/") + item.folder;
  const previewUrl = (item) => base + folderPath(item) + "/preview.png";
  const sizeOf = (files) => files.reduce((sum, f) => sum + (f.size || 0), 0);
  const prettySize = (n) => (n >= 1048576 ? (n / 1048576).toFixed(1) + " MB" : Math.max(1, Math.round(n / 1024)) + " KB");

  function el(tag, props = {}, ...children) {
    const node = document.createElement(tag);
    for (const [key, value] of Object.entries(props)) {
      if (key === "text") node.textContent = value;
      else if (key === "class") node.className = value;
      else node.setAttribute(key, value);
    }
    for (const child of children) if (child) node.append(child);
    return node;
  }

  function normalise(index, itemKind, list) {
    return (list || []).map((p) => ({
      kind: itemKind, id: p.id, name: p.name || p.id, author: p.author || "", about: p.description || "",
      folder: p.folder, files: p.files || [], needs: p.needs || [],
    }));
  }

  function visible() {
    const words = query.toLowerCase().split(/\s+/).filter(Boolean);
    return items.filter((it) => (kind === "all" || it.kind === kind) &&
      words.every((w) => (it.name + " " + it.author + " " + it.about + " " + it.id).toLowerCase().includes(w)));
  }

  function render() {
    const shown = visible();
    grid.replaceChildren(...shown.map((it) => {
      const img = el("img", { src: previewUrl(it), alt: "", loading: "lazy" });
      const card = el("button", { class: "lib-card", type: "button", "aria-label": it.name + ", " + KIND_NAME[it.kind] },
        img,
        el("span", { class: "meta" },
          el("span", { class: "chip", text: KIND_NAME[it.kind] }),
          el("h3", { text: it.name }),
          it.author && el("p", { text: "by " + it.author }),
          it.about && el("p", { class: "about", text: it.about })));
      card.addEventListener("click", () => open(it));
      return card;
    }));
    state.textContent = shown.length ? "" : items.length ? "Nothing matches that." : state.textContent;
    for (const tab of document.querySelectorAll(".tabs button")) {
      const k = tab.dataset.kind;
      tab.querySelector(".count").textContent = items.filter((it) => k === "all" || it.kind === k).length;
    }
  }

  function open(it) {
    $("d-preview").src = previewUrl(it);
    $("d-kind").textContent = KIND_NAME[it.kind];
    $("d-title").textContent = it.name;
    $("d-by").textContent = (it.author ? "by " + it.author + " · " : "") + prettySize(sizeOf(it.files));
    $("d-about").textContent = it.about;
    $("d-where").textContent = WHERE[it.kind] + (it.needs.length ? " It also installs: " + it.needs.join(", ") + "." : "");
    $("d-files").replaceChildren(...it.files.map((f) => el("li", {}, el("span", { text: f.path }), el("span", { text: prettySize(f.size) }))));
    $("d-source").href = REPO + folderPath(it);
    history.replaceState(null, "", "#" + it.kind + "/" + it.folder);
    if (!dialog.open) dialog.showModal();
  }

  function fromHash() {
    const [k, folder] = decodeURIComponent(location.hash.slice(1)).split("/");
    const found = items.find((it) => it.kind === k && it.folder === folder);
    if (found) open(found);
  }

  dialog.addEventListener("close", () => history.replaceState(null, "", location.pathname + location.search));
  dialog.addEventListener("click", (event) => { if (event.target === dialog) dialog.close(); });
  $("q").addEventListener("input", (event) => { query = event.target.value; render(); });
  for (const tab of document.querySelectorAll(".tabs button")) {
    tab.addEventListener("click", () => {
      kind = tab.dataset.kind;
      for (const other of document.querySelectorAll(".tabs button")) other.setAttribute("aria-selected", String(other === tab));
      render();
    });
  }

  (async () => {
    try {
      const [widgets, themes] = await Promise.all([getJson("widgets/index.json"), getJson("themes/index.json")]);
      items = [
        ...normalise(widgets, "look", widgets.packs),
        ...normalise(widgets, "stack", widgets.stacks),
        ...normalise(themes, "theme", themes.packs),
      ].sort((a, b) => a.name.localeCompare(b.name));
      state.textContent = "";
      render();
      fromHash();
    } catch (err) {
      state.textContent = "The library could not be loaded just now. Try again in a moment.";
    }
  })();
})();
