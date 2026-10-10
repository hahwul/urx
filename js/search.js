/* Client-side docs search over /search.json, which Hwaro already generates.
   No dependency: the scoring below is small enough not to justify one. */
(function () {
  "use strict";
  var overlay, input, results, index = null, active = -1, rows = [];

  /* Derive the JSON path from the stylesheet's own URL so a sub-path deploy
     keeps working without a build-time base_url. */
  function searchUrl() {
    var link = document.querySelector('link[rel="stylesheet"][href*="/css/"]');
    if (!link) return "/search.json";
    var path = new URL(link.href, document.baseURI).pathname;
    return path.substring(0, path.indexOf("/css/")) + "/search.json";
  }

  function score(item, terms) {
    var title = (item.title || "").toLowerCase();
    var content = (item.content || "").toLowerCase();
    var total = 0;
    for (var i = 0; i < terms.length; i++) {
      var t = terms[i];
      var ti = title.indexOf(t);
      var ci = content.indexOf(t);
      if (ti === -1 && ci === -1) return -1;          // every term must appear
      if (ti !== -1) {
        total += 1000 - Math.min(ti, 100);
        if (ti === 0 || /[^a-z0-9]/.test(title.charAt(ti - 1))) total += 200;
        if (title === t) total += 400;
      } else {
        total += 100 - Math.min(90, Math.floor(ci / 24));
      }
    }
    return total;
  }

  function esc(s) {
    return s.replace(/[&<>"]/g, function (c) {
      return { "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;" }[c];
    });
  }

  function mark(text, terms) {
    var out = esc(text);
    terms.forEach(function (t) {
      out = out.replace(new RegExp("(" + t.replace(/[.*+?^${}()|[\]\\]/g, "\\$&") + ")", "gi"), "<mark>$1</mark>");
    });
    return out;
  }

  function snippet(item, terms) {
    var content = item.content || "";
    var at = content.toLowerCase().indexOf(terms[0]);
    if (at === -1) return mark(content.slice(0, 120), terms) + (content.length > 120 ? "..." : "");
    var start = Math.max(0, at - 60);
    var end = Math.min(content.length, at + 100);
    return (start > 0 ? "..." : "") + mark(content.slice(start, end), terms) + (end < content.length ? "..." : "");
  }

  function render(query) {
    var terms = query.toLowerCase().split(/\s+/).filter(Boolean);
    results.innerHTML = "";
    rows = [];
    active = -1;
    if (!terms.length || !index) return;

    var hits = [];
    index.forEach(function (item) {
      var s = score(item, terms);
      if (s > 0) hits.push({ item: item, score: s });
    });
    hits.sort(function (a, b) { return b.score - a.score; });

    if (!hits.length) {
      results.innerHTML = '<li class="search-empty">No matches for &ldquo;' + esc(query) + '&rdquo;</li>';
      return;
    }
    hits.slice(0, 10).forEach(function (hit) {
      var li = document.createElement("li");
      li.innerHTML =
        '<a href="' + hit.item.url + '">' +
        '<span class="search-result-title">' + mark(hit.item.title || "", terms) + "</span>" +
        '<span class="search-result-snippet">' + snippet(hit.item, terms) + "</span></a>";
      results.appendChild(li);
      rows.push(li);
    });
    move(0);
  }

  function move(i) {
    if (!rows.length) return;
    if (active >= 0) rows[active].classList.remove("active");
    active = (i + rows.length) % rows.length;
    rows[active].classList.add("active");
    rows[active].scrollIntoView({ block: "nearest" });
  }

  function open() {
    overlay.classList.add("open");
    document.documentElement.classList.add("search-lock");
    input.value = "";
    results.innerHTML = "";
    input.focus();
    if (index) return;
    fetch(searchUrl())
      .then(function (r) { return r.json(); })
      .then(function (data) { index = Array.isArray(data) ? data : data.pages || []; })
      .catch(function () { index = []; });
  }

  function close() {
    overlay.classList.remove("open");
    document.documentElement.classList.remove("search-lock");
  }

  document.addEventListener("DOMContentLoaded", function () {
    overlay = document.getElementById("searchOverlay");
    input = document.getElementById("searchInput");
    results = document.getElementById("searchResults");
    if (!overlay || !input || !results) return;

    document.addEventListener("click", function (e) {
      if (e.target.closest("[data-search-open]")) { e.preventDefault(); open(); }
      else if (e.target === overlay) close();
    });

    input.addEventListener("input", function () { render(input.value.trim()); });

    document.addEventListener("keydown", function (e) {
      var isOpen = overlay.classList.contains("open");
      if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "k") {
        e.preventDefault();
        isOpen ? close() : open();
        return;
      }
      if (!isOpen) return;
      if (e.key === "Escape") { e.preventDefault(); close(); }
      else if (e.key === "ArrowDown") { e.preventDefault(); move(active + 1); }
      else if (e.key === "ArrowUp") { e.preventDefault(); move(active - 1); }
      else if (e.key === "Enter" && active >= 0) {
        e.preventDefault();
        var a = rows[active].querySelector("a");
        if (a) window.location.href = a.getAttribute("href");
      }
    });

    /* The nav shows the Apple key by default; correct it elsewhere. */
    if (!/Mac|iPhone|iPad/.test(navigator.platform)) {
      document.querySelectorAll(".search-btn kbd").forEach(function (k) { k.textContent = "Ctrl K"; });
    }
  });
})();
