/* hfsdr docs — navigation, motion, diagram lifecycle */
(function () {
  "use strict";

  var progressEl = null;
  var contentObs = null;
  var enhanceScheduled = false;

  function initProgress() {
    if (progressEl) return;
    progressEl = document.createElement("div");
    progressEl.className = "hf-progress";
    progressEl.setAttribute("aria-hidden", "true");
    document.body.appendChild(progressEl);
    window.addEventListener(
      "scroll",
      function () {
        var doc = document.documentElement;
        var scrollTop = doc.scrollTop || document.body.scrollTop;
        var height = doc.scrollHeight - doc.clientHeight;
        var pct = height > 0 ? (scrollTop / height) * 100 : 0;
        progressEl.style.width = pct + "%";
      },
      { passive: true }
    );
  }

  function markSidebarActive() {
    var path = window.location.pathname.split("/").pop() || "index.html";
    document.querySelectorAll("#sidebar a").forEach(function (a) {
      var href = a.getAttribute("href");
      if (!href || href.indexOf("http") === 0) return;
      var active =
        href === path ||
        (path === "" && href === "index.html") ||
        href === path.replace(/^\.\//, "");
      a.classList.toggle("active", active);
    });
  }

  function revealOnScroll(root) {
    var nodes = (root || document).querySelectorAll(".hf-reveal:not(.hf-visible)");
    if (!("IntersectionObserver" in window)) {
      nodes.forEach(function (n) {
        n.classList.add("hf-visible");
      });
      return;
    }
    var obs = new IntersectionObserver(
      function (entries) {
        entries.forEach(function (e) {
          if (e.isIntersecting) {
            e.target.classList.add("hf-visible");
            obs.unobserve(e.target);
          }
        });
      },
      { threshold: 0.08, rootMargin: "0px 0px -20px 0px" }
    );
    nodes.forEach(function (n) {
      obs.observe(n);
    });
  }

  function wrapTables(root) {
    (root || document)
      .querySelectorAll(".content main table")
      .forEach(function (table) {
        if (table.parentElement.classList.contains("hf-table-wrap")) return;
        var wrap = document.createElement("div");
        wrap.className = "hf-table-wrap hf-reveal";
        wrap.style.overflowX = "auto";
        table.parentNode.insertBefore(wrap, table);
        wrap.appendChild(table);
      });
  }

  function enhanceContent() {
    var content = document.getElementById("content");
    if (!content) return;

    if (window.HfDiagrams) {
      window.HfDiagrams.mountAll(content);
    }

    revealOnScroll(content);
    wrapTables(content);
    markSidebarActive();

    content.querySelectorAll("h2, h3, blockquote, .hf-card").forEach(function (el) {
      if (!el.classList.contains("hf-reveal")) el.classList.add("hf-reveal");
    });
  }

  function scheduleEnhance() {
    if (enhanceScheduled) return;
    enhanceScheduled = true;
    requestAnimationFrame(function () {
      enhanceScheduled = false;
      enhanceContent();
    });
  }

  function observeContent() {
    var content = document.getElementById("content");
    if (!content || contentObs) return;
    contentObs = new MutationObserver(function () {
      scheduleEnhance();
    });
    contentObs.observe(content, { childList: true, subtree: true });
  }

  function init() {
    if (document.body.classList.contains("rustdoc")) {
      document.documentElement.dataset.theme =
        document.documentElement.dataset.theme || "ayu";
    }
    initProgress();
    enhanceContent();
    observeContent();
    document.body.classList.add("hf-docs");
  }

  if (document.readyState === "loading") {
    document.addEventListener("DOMContentLoaded", init);
  } else {
    init();
  }

  window.addEventListener("popstate", function () {
    scheduleEnhance();
  });

  var origPush = history.pushState;
  var origReplace = history.replaceState;
  history.pushState = function () {
    origPush.apply(this, arguments);
    scheduleEnhance();
  };
  history.replaceState = function () {
    origReplace.apply(this, arguments);
    scheduleEnhance();
  };
})();
