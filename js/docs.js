/* Docs-page chrome: the mobile sidebar disclosure, and keyboard access to
   overflowing code blocks and tables. */
(function () {
  "use strict";
  var nav = document.querySelector(".docs-navigation");
  if (nav) {
    var mq = window.matchMedia("(max-width: 860px)");
    var sync = function () {
      nav.open = !mq.matches;
      nav.setAttribute("data-ready", "");
    };
    sync();
    mq.addEventListener("change", sync);
    document.addEventListener("keydown", function (e) {
      if (e.key === "Escape" && mq.matches && nav.open) nav.open = false;
    });
  }

  /* Only give focus to blocks that actually scroll, so keyboard users get the
     scroll affordance without dozens of extra tab stops. */
  var targets = document.querySelectorAll(".document-body pre, .document-body table");
  if (!targets.length || !window.ResizeObserver) return;
  var ro = new ResizeObserver(function (entries) {
    entries.forEach(function (entry) {
      var el = entry.target;
      if (el.scrollWidth > el.clientWidth + 1) el.setAttribute("tabindex", "0");
      else el.removeAttribute("tabindex");
    });
  });
  targets.forEach(function (el) { ro.observe(el); });
})();
