/* Theme toggle. The no-flash script in header.html has already applied any stored
   choice; this only handles the click. No stored value means no data-theme
   attribute, which lets color-scheme + light-dark() follow the OS. */
(function () {
  "use strict";
  var KEY = "urx-theme";

  function current() {
    var set = document.documentElement.getAttribute("data-theme");
    if (set === "light" || set === "dark") return set;
    return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
  }

  document.addEventListener("click", function (e) {
    var btn = e.target.closest("[data-theme-toggle]");
    if (!btn) return;
    var next = current() === "dark" ? "light" : "dark";
    document.documentElement.setAttribute("data-theme", next);
    try { localStorage.setItem(KEY, next); } catch (err) {}
  });
})();
