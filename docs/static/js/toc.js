/* The TOC markup comes from Hwaro's native {{ toc }}, so it is already in the
   HTML and works with JS off. This only adds scrollspy, heading anchors, and the
   move to an inline disclosure on narrow viewports. */
(function () {
  "use strict";
  var rail = document.getElementById("tocRail");
  if (!rail) return;

  var links = Array.prototype.slice.call(rail.querySelectorAll("a[href^='#']"));
  if (links.length < 2) { rail.remove(); return; }

  /* --- hover anchors on headings --- */
  document.querySelectorAll(".document-body h2[id], .document-body h3[id]").forEach(function (h) {
    var a = document.createElement("a");
    a.className = "heading-anchor";
    a.href = "#" + h.id;
    a.setAttribute("aria-label", "Link to this section");
    a.textContent = "#";
    h.appendChild(a);
  });

  /* --- scrollspy --- */
  var byId = {};
  links.forEach(function (a) { byId[decodeURIComponent(a.getAttribute("href").slice(1))] = a; });
  var headings = Object.keys(byId)
    .map(function (id) { return document.getElementById(id); })
    .filter(Boolean);

  if (headings.length && "IntersectionObserver" in window) {
    var offset = 100;   // header height plus a little breathing room

    var mark = function () {
      // The active entry is the last heading scrolled past, not merely one that
      // happens to intersect: between two headings nothing intersects, and the
      // rail would otherwise go blank.
      var current = headings[0];
      for (var i = 0; i < headings.length; i++) {
        if (headings[i].getBoundingClientRect().top <= offset) current = headings[i];
        else break;
      }
      links.forEach(function (a) { a.classList.remove("active"); });
      if (current && byId[current.id]) byId[current.id].classList.add("active");
    };

    // The observer is only a cheap "something moved" signal; mark() does the work.
    var spy = new IntersectionObserver(mark, {
      rootMargin: "-" + offset + "px 0px 0px 0px",
      threshold: [0, 1]
    });
    headings.forEach(function (h) { spy.observe(h); });
    mark();
  }

  /* --- responsive relocation ---
     Below 1199px the rail moves into the article, under the page heading. */
  var mq = window.matchMedia("(max-width: 1199px)");
  var heading = document.querySelector(".document-heading");
  var main = document.querySelector(".docs-main");
  var container = rail.parentNode;

  function place() {
    if (mq.matches) {
      if (rail.parentNode !== main) {
        rail.classList.add("inline");
        heading.insertAdjacentElement("afterend", rail);
      }
    } else if (rail.parentNode !== container) {
      rail.classList.remove("inline");
      container.appendChild(rail);
    }
  }
  if (heading && main) {
    place();
    mq.addEventListener("change", place);
  }
})();
