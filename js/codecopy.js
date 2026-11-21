/* Copy button on every code block.
   The <pre> is wrapped in .code-block first: an absolutely-positioned child of a
   scrolling <pre> slides out of view on long lines. */
(function () {
  "use strict";
  var CLIP = '<svg class="clip" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V5a2 2 0 0 1 2-2h10"/></svg>';
  var CHECK = '<svg class="check" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="m5 13 4 4L19 7"/></svg>';

  function copy(text) {
    if (navigator.clipboard && window.isSecureContext) {
      // Can still reject: unfocused document, or permission denied.
      return navigator.clipboard.writeText(text).catch(function () { return legacyCopy(text); });
    }
    return legacyCopy(text);
  }

  // http:// previews have no clipboard API, and the modern one is not guaranteed.
  function legacyCopy(text) {
    return new Promise(function (resolve, reject) {
      var ta = document.createElement("textarea");
      ta.value = text;
      ta.style.cssText = "position:fixed;top:-9999px";
      document.body.appendChild(ta);
      ta.select();
      try { document.execCommand("copy") ? resolve() : reject(); }
      catch (e) { reject(e); }
      finally { document.body.removeChild(ta); }
    });
  }

  document.querySelectorAll(".document-body pre > code").forEach(function (code) {
    var pre = code.parentNode;
    if (pre.parentNode.classList.contains("code-block")) return;

    var wrap = document.createElement("div");
    wrap.className = "code-block";
    pre.parentNode.insertBefore(wrap, pre);
    wrap.appendChild(pre);

    var btn = document.createElement("button");
    btn.className = "code-copy";
    btn.type = "button";
    btn.setAttribute("aria-label", "Copy code");
    btn.innerHTML = CLIP + CHECK;
    wrap.appendChild(btn);

    var timer;
    btn.addEventListener("click", function () {
      copy(code.innerText).then(function () {
        btn.classList.add("copied");
        btn.setAttribute("aria-label", "Copied");
        clearTimeout(timer);
        timer = setTimeout(function () {
          btn.classList.remove("copied");
          btn.setAttribute("aria-label", "Copy code");
        }, 1600);
      }).catch(function () {});
    });
  });
})();
