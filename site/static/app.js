// Vitrina: progressive enhancement only. Every page is complete without this
// file (spec §3.2). No libraries, no build step, budget 10 KB.
(function () {
  "use strict";
  var api = document.body.getAttribute("data-api") || "/api";
  var list = document.getElementById("list");
  var current = list ? list.getAttribute("data-default") : "";

  function order(key) {
    var attr = list.getAttribute("data-o-" + key);
    return attr === null ? null : attr.split(" ").filter(Boolean);
  }

  // Pre-rendered orders live in data attributes; switching a preset moves
  // the existing cards, it never computes a ranking.
  function apply(key) {
    var ids = order(key);
    if (!ids) return false;
    var cards = {};
    Array.prototype.forEach.call(list.children, function (li) {
      cards[li.getAttribute("data-id")] = li;
      li.hidden = true;
    });
    ids.forEach(function (id, i) {
      var li = cards[id];
      if (!li) return;
      li.hidden = false;
      var r = li.querySelector(".rank");
      if (r) r.textContent = String(i + 1);
      list.appendChild(li);
    });
    document.querySelectorAll(".tabs a[data-preset]").forEach(function (a) {
      if (a.getAttribute("data-preset") === key) a.setAttribute("aria-current", "page");
      else a.removeAttribute("aria-current");
    });
    document.querySelectorAll(".pnote[data-for]").forEach(function (p) {
      p.hidden = p.getAttribute("data-for") !== key;
    });
    current = key;
    return true;
  }

  if (list) {
    // Without a script only the default preset exists, so the tabs appear
    // only once they can work.
    document.querySelectorAll(".tabs[hidden]").forEach(function (n) { n.hidden = false; });
    var wanted = new URLSearchParams(location.search).get("preset");
    if (wanted && wanted !== current) apply(wanted);
    document.querySelectorAll(".tabs a[data-preset]").forEach(function (a) {
      a.addEventListener("click", function (e) {
        var key = a.getAttribute("data-preset");
        if (!apply(key)) return;
        e.preventDefault();
        var u = new URL(location.href);
        u.searchParams.set("preset", key);
        history.replaceState(null, "", u.pathname + u.search + u.hash);
      });
    });
  }

  // Count the click, then let the link do its job: the tracking link is
  // followed directly, never through our own redirect (И3).
  document.addEventListener("click", function (e) {
    var a = e.target.closest ? e.target.closest("a[data-buy]") : null;
    if (!a || !navigator.sendBeacon) return;
    var body = new URLSearchParams({
      product: a.getAttribute("data-buy"),
      preset: current || new URLSearchParams(location.search).get("preset") || "",
      page: location.pathname
    });
    try { navigator.sendBeacon(api + "/click", body); } catch (err) { /* counting is optional */ }
  });

  document.querySelectorAll("form.report").forEach(function (form) {
    form.addEventListener("submit", function (e) {
      if (!window.fetch) return;
      e.preventDefault();
      var body = new URLSearchParams(new FormData(form));
      body.set("page_url", location.pathname);
      function show(sel) {
        form.querySelectorAll(".thanks").forEach(function (p) { p.hidden = !p.matches(sel); });
      }
      fetch(form.action, { method: "POST", body: body, credentials: "omit" })
        .then(function (r) { show(r.ok ? ".ok" : ".fail"); if (r.ok) form.reset(); })
        .catch(function () { show(".fail"); });
    });
  });
})();
