// Reads releases.json (the GitHub releases list the in-app updater also uses,
// published next to this page by .github/workflows/apt-repo.yml) and builds
// the download sections. Everything is inserted with textContent / DOM nodes.
(function () {
  "use strict";

  var LIVE_LIST = "https://theamanullahdev.github.io/paperloom/releases.json";
  var RELEASES_PAGE = "https://github.com/theamanullahdev/paperloom/releases";

  // Release asset naming contract: paperloom-<ver>-<os>-<arch>[-setup].<ext>
  var PLATFORMS = [
    { id: "windows", title: "Windows", note: "Windows 10 and 11", files: [
      { label: "Installer (.exe)", match: /-windows-x86_64-setup\.exe$/i, main: true },
      { label: "MSI, for managed installs", match: /-windows-x86_64\.msi$/i }
    ]},
    { id: "macos", title: "macOS", note: "macOS 15 or newer", files: [
      { label: "Apple Silicon (.dmg)", match: /-macos-aarch64\.dmg$/i, main: true },
      { label: "Intel (.dmg)", match: /-macos-x86_64\.dmg$/i }
    ]},
    { id: "deb", title: "Ubuntu, Mint, Debian, Kali", note: "Debian-based systems", files: [
      { label: "Package (.deb)", match: /-linux-x86_64\.deb$/i, main: true }
    ]},
    { id: "rpm", title: "Fedora, openSUSE", note: "RPM-based systems", files: [
      { label: "Package (.rpm)", match: /-linux-x86_64\.rpm$/i, main: true }
    ]},
    { id: "flatpak", title: "Any Linux — Flatpak", note: "Includes Arch, Manjaro, EndeavourOS", files: [
      { label: "Bundle (.flatpak)", match: /-linux-x86_64\.flatpak$/i, main: true }
    ], hint: "flatpak install --user ./paperloom-*-linux-x86_64.flatpak" }
  ];

  var releases = [];
  var $ = function (id) { return document.getElementById(id); };

  function el(tag, cls, text) {
    var n = document.createElement(tag);
    if (cls) n.className = cls;
    if (text != null) n.textContent = text;
    return n;
  }

  function fmtSize(bytes) {
    if (!bytes) return "";
    var mb = bytes / (1024 * 1024);
    return mb >= 100 ? Math.round(mb) + " MB" : mb.toFixed(1) + " MB";
  }
  function fmtDate(iso) {
    var d = new Date(iso);
    if (isNaN(d)) return "";
    return d.toLocaleDateString(undefined, { year: "numeric", month: "short", day: "numeric" });
  }
  function version(r) { return String(r.tag_name || "").replace(/^v/, ""); }
  function findAsset(r, re) {
    var list = r.assets || [];
    for (var i = 0; i < list.length; i++) if (re.test(list[i].name)) return list[i];
    return null;
  }

  function detectOS() {
    var ua = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
    var full = ua + " " + navigator.userAgent;
    if (/android|iphone|ipad|ipod/i.test(full)) return "mobile";
    if (/win/i.test(ua) || /Windows/i.test(full)) return "windows";
    if (/mac/i.test(ua) || /Macintosh/i.test(full)) return "macos";
    if (/linux|x11|cros/i.test(full)) return "linux";
    return "unknown";
  }

  function fileLink(asset, label) {
    var a = el("a", "file" + (asset ? "" : " missing"));
    a.appendChild(el("span", "name", label));
    if (asset) {
      a.href = asset.browser_download_url;
      a.appendChild(el("span", "meta", fmtSize(asset.size)));
    } else {
      a.appendChild(el("span", "meta", "not in this release"));
    }
    return a;
  }

  function renderPlatforms(rel) {
    var grid = $("platform-grid");
    grid.textContent = "";
    PLATFORMS.forEach(function (p) {
      var card = el("div", "card");
      card.id = "p-" + p.id;
      card.appendChild(el("h3", null, p.title));
      card.appendChild(el("p", "for", p.note));
      var files = el("div", "files");
      p.files.forEach(function (f) { files.appendChild(fileLink(findAsset(rel, f.match), f.label)); });
      card.appendChild(files);
      if (p.hint) {
        var pre = el("pre"); pre.appendChild(el("code", null, p.hint));
        pre.style.marginTop = "12px"; pre.style.marginBottom = "0";
        card.appendChild(pre);
      }
      grid.appendChild(card);
    });
    $("platforms-ver").textContent = "v" + version(rel);
  }

  function renderPrimary(rel) {
    var os = detectOS();
    var a = $("primary"), sub = $("primary-sub"), note = $("primary-note");
    var v = "v" + version(rel);
    var pick = null, text = "", extra = "";
    if (os === "windows") {
      pick = findAsset(rel, PLATFORMS[0].files[0].match); text = "Download for Windows";
    } else if (os === "macos") {
      pick = findAsset(rel, PLATFORMS[1].files[0].match); text = "Download for macOS";
      var intel = findAsset(rel, PLATFORMS[1].files[1].match);
      if (intel) {
        extra = "Apple Silicon build. On an Intel Mac, ";
        note.textContent = extra;
        var l = el("a", null, "get the Intel build"); l.href = intel.browser_download_url;
        note.appendChild(l); note.appendChild(document.createTextNode("."));
      }
    } else if (os === "linux") {
      pick = findAsset(rel, PLATFORMS[2].files[0].match); text = "Download for Linux (.deb)";
      note.textContent = "Fedora, Arch and others: see the other formats below.";
    }
    if (pick) {
      a.href = pick.browser_download_url;
      a.querySelector(".btn-title").textContent = text;
      sub.textContent = v + " · " + fmtSize(pick.size);
    } else {
      a.href = "#platforms";
      a.querySelector(".btn-title").textContent = "Download Paperloom";
      sub.textContent = v + " · choose your system";
      if (os === "mobile") note.textContent = "Paperloom runs on desktop computers.";
    }
  }

  function renderDetail(rel, isLatest) {
    var box = $("release-detail");
    box.textContent = "";
    var h = el("h3", null, "Paperloom " + version(rel));
    if (isLatest) h.appendChild(el("span", "badge latest", "Latest"));
    if (rel.prerelease) h.appendChild(el("span", "badge pre", "Pre-release"));
    box.appendChild(h);
    box.appendChild(el("p", "date", "Published " + fmtDate(rel.published_at || rel.created_at)));

    var files = el("div", "files");
    var shown = {};
    PLATFORMS.forEach(function (p) {
      p.files.forEach(function (f) {
        var asset = findAsset(rel, f.match);
        if (asset) { shown[asset.name] = true; files.appendChild(fileLink(asset, p.title.split(",")[0].replace(" — Flatpak", "") + " · " + f.label)); }
      });
    });
    (rel.assets || []).forEach(function (asset) {
      if (!shown[asset.name]) files.appendChild(fileLink(asset, asset.name));
    });
    if (!files.children.length) files.appendChild(el("p", "muted", "This release has no attached files."));
    box.appendChild(files);

    var sums = findAsset(rel, /^SHA256SUMS\.txt$/i);
    if (sums) {
      var p = el("p", "sums"); var a = el("a", null, "SHA256SUMS.txt"); a.href = sums.browser_download_url;
      p.appendChild(document.createTextNode("Verify downloads with ")); p.appendChild(a); p.appendChild(document.createTextNode("."));
      box.appendChild(p);
    }

    if (rel.body && rel.body.trim()) {
      var d = el("details", "notes"); d.appendChild(el("summary", null, "Release notes"));
      d.appendChild(el("pre", null, rel.body.trim()));
      box.appendChild(d);
    }
    $("gh-link").href = rel.html_url || RELEASES_PAGE;
  }

  function select(index) {
    var rel = releases[index];
    $("version").value = String(index);
    renderDetail(rel, index === 0);
    var hash = "#v" + version(rel);
    if (history.replaceState) history.replaceState(null, "", index === 0 ? location.pathname + location.search : hash);
  }

  function renderHistory() {
    var ul = $("history-list");
    ul.textContent = "";
    releases.forEach(function (r, i) {
      var li = el("li");
      var b = el("button", null, "v" + version(r)); b.type = "button";
      b.addEventListener("click", function () {
        select(i);
        $("release-detail").scrollIntoView({ behavior: "smooth", block: "start" });
      });
      li.appendChild(b);
      li.appendChild(el("span", "d", fmtDate(r.published_at || r.created_at)));
      ul.appendChild(li);
    });
  }

  function renderPicker() {
    var s = $("version");
    s.textContent = "";
    releases.forEach(function (r, i) {
      var o = el("option", null, "v" + version(r) + (i === 0 ? " (latest)" : "") + (r.prerelease ? " — pre-release" : ""));
      o.value = String(i);
      s.appendChild(o);
    });
    s.disabled = false;
    s.addEventListener("change", function () { select(parseInt(s.value, 10)); });
  }

  function fail() {
    $("platform-grid").textContent = "";
    var p = el("p", "muted", "The release list couldn't be loaded. ");
    var a = el("a", null, "Download from GitHub Releases"); a.href = RELEASES_PAGE;
    p.appendChild(a); p.appendChild(document.createTextNode("."));
    $("platform-grid").appendChild(p);
    $("version").textContent = ""; $("version").appendChild(el("option", null, "Unavailable"));
    $("release-detail").appendChild(p.cloneNode(true));
  }

  function load(url) {
    return fetch(url, { cache: "no-cache" }).then(function (r) {
      if (!r.ok) throw new Error(String(r.status));
      return r.json();
    });
  }

  function start(list) {
    releases = list
      .filter(function (r) { return !r.draft; })
      .sort(function (a, b) { return new Date(b.published_at || b.created_at) - new Date(a.published_at || a.created_at); });
    // "Latest" is the newest stable release when there is one.
    var stable = releases.filter(function (r) { return !r.prerelease; });
    if (stable.length && stable[0] !== releases[0]) {
      releases = stable.concat(releases.filter(function (r) { return r.prerelease; }));
    }
    if (!releases.length) { fail(); return; }

    renderPlatforms(releases[0]);
    renderPrimary(releases[0]);
    renderPicker();
    renderHistory();

    var want = (location.hash.match(/^#v(.+)$/) || [])[1];
    var idx = 0;
    if (want) releases.forEach(function (r, i) { if (version(r) === want) idx = i; });
    select(idx);
    if (want && idx) setTimeout(function () { $("releases").scrollIntoView(); }, 0);
  }

  // Copy buttons
  document.addEventListener("click", function (e) {
    var b = e.target.closest && e.target.closest(".copy");
    if (!b) return;
    var pre = b.previousElementSibling;
    if (!pre) return;
    var text = pre.textContent;
    var done = function () { var t = b.textContent; b.textContent = "Copied"; setTimeout(function () { b.textContent = t; }, 1600); };
    if (navigator.clipboard) navigator.clipboard.writeText(text).then(done, function () {});
  });

  load("releases.json").catch(function () { return load(LIVE_LIST); }).then(start, fail);
})();
