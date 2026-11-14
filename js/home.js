/* Scroll reveal for landing sections. The CSS only hides .rv when .has-js is on,
   so if this file fails to load the content is simply visible. */
(function () {
  "use strict";
  var items = document.querySelectorAll(".rv");
  if (!items.length) return;

  var reduce = window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  var show = function (el) { el.classList.add("in"); };

  if (reduce || !("IntersectionObserver" in window)) {
    items.forEach(show);
    return;
  }

  var io = new IntersectionObserver(function (entries) {
    entries.forEach(function (entry) {
      if (!entry.isIntersecting) return;
      show(entry.target);
      io.unobserve(entry.target);
    });
  }, { rootMargin: "0px 0px -10% 0px", threshold: 0.1 });

  items.forEach(function (el) { io.observe(el); });
  window.addEventListener("beforeprint", function () { items.forEach(show); });
})();
