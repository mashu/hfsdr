/* Interactive SVG / canvas diagrams for hfsdr docs */
(function (global) {
  "use strict";

  var C = {
    accent: "#38bdf8",
    accentDim: "#1e648c",
    ok: "#6ee7b7",
    warn: "#fbbf24",
    panel: "#161b26",
    raised: "#1c212c",
    grid: "#323c4e",
    muted: "#8c96aa",
    text: "#e2e8f4",
    error: "#f87171",
  };

  function reducedMotion() {
    return window.matchMedia("(prefers-reduced-motion: reduce)").matches;
  }

  function shell(title, hint, bodyEl, footerEl) {
    var wrap = document.createElement("div");
    wrap.className = "hf-diagram";
    wrap.innerHTML =
      '<div class="hf-diagram-header">' +
      '<p class="hf-diagram-title">' +
      title +
      "</p>" +
      (hint ? '<span class="hf-diagram-hint">' + hint + "</span>" : "") +
      "</div>";
    var body = document.createElement("div");
    body.className = "hf-diagram-body";
    body.appendChild(bodyEl);
    wrap.appendChild(body);
    var foot = document.createElement("div");
    foot.className = "hf-diagram-footer";
    if (footerEl) foot.appendChild(footerEl);
    else foot.textContent = hint || "";
    wrap.appendChild(foot);
    return { wrap: wrap, footer: foot, body: body };
  }

  function svgEl(w, h) {
    var ns = "http://www.w3.org/2000/svg";
    var s = document.createElementNS(ns, "svg");
    s.setAttribute("viewBox", "0 0 " + w + " " + h);
    s.setAttribute("width", "100%");
    s.setAttribute("height", String(h));
    s.setAttribute("preserveAspectRatio", "xMidYMid meet");
    s.setAttribute("role", "img");
    return s;
  }

  function setupCanvas(canvas, w, h) {
    canvas.width = w;
    canvas.height = h;
    canvas.style.width = "100%";
    canvas.style.height = h + "px";
    canvas.style.maxWidth = w + "px";
    canvas.style.display = "block";
  }

  function defsGradient(svg, id) {
    var ns = "http://www.w3.org/2000/svg";
    var uid = id + "-" + Math.random().toString(36).slice(2, 9);
    var defs = document.createElementNS(ns, "defs");
    var grad = document.createElementNS(ns, "linearGradient");
    grad.setAttribute("id", uid);
    grad.setAttribute("x1", "0%");
    grad.setAttribute("y1", "0%");
    grad.setAttribute("x2", "100%");
    grad.setAttribute("y2", "0%");
    ["0%", "50%", "100%"].forEach(function (off, i) {
      var stop = document.createElementNS(ns, "stop");
      stop.setAttribute("offset", off);
      stop.setAttribute(
        "stop-color",
        i === 1 ? C.accent : i === 0 ? C.accentDim : C.ok
      );
      grad.appendChild(stop);
    });
    defs.appendChild(grad);
    svg.appendChild(defs);
    return uid;
  }

  function box(svg, x, y, w, h, label, sub, color, id) {
    var ns = "http://www.w3.org/2000/svg";
    var g = document.createElementNS(ns, "g");
    g.setAttribute("class", "hf-node");
    if (id) g.setAttribute("data-id", id);
    var r = document.createElementNS(ns, "rect");
    r.setAttribute("x", x);
    r.setAttribute("y", y);
    r.setAttribute("width", w);
    r.setAttribute("height", h);
    r.setAttribute("rx", "10");
    r.setAttribute("fill", C.raised);
    r.setAttribute("stroke", color || C.grid);
    r.setAttribute("stroke-width", "1.5");
    g.appendChild(r);
    var t = document.createElementNS(ns, "text");
    t.setAttribute("x", x + w / 2);
    t.setAttribute("y", y + (sub ? h / 2 - 4 : h / 2));
    t.setAttribute("text-anchor", "middle");
    t.setAttribute("dominant-baseline", "middle");
    t.setAttribute("fill", C.text);
    t.setAttribute("font-size", "13");
    t.setAttribute("font-weight", "600");
    t.textContent = label;
    g.appendChild(t);
    if (sub) {
      var t2 = document.createElementNS(ns, "text");
      t2.setAttribute("x", x + w / 2);
      t2.setAttribute("y", y + h / 2 + 14);
      t2.setAttribute("text-anchor", "middle");
      t2.setAttribute("fill", C.muted);
      t2.setAttribute("font-size", "10");
      t2.textContent = sub;
      g.appendChild(t2);
    }
    svg.appendChild(g);
    return g;
  }

  function line(svg, x1, y1, x2, y2, active, gradId) {
    var ns = "http://www.w3.org/2000/svg";
    var p = document.createElementNS(ns, "path");
    p.setAttribute("d", "M" + x1 + " " + y1 + " L" + x2 + " " + y2);
    p.setAttribute("class", active ? "hf-pipe-active" : "hf-pipe");
    if (active && gradId) {
      p.setAttribute("stroke", "url(#" + gradId + ")");
    }
    svg.appendChild(p);
  }

  function animateDot(svg, pathD, duration) {
    if (reducedMotion()) return;
    var ns = "http://www.w3.org/2000/svg";
    var path = document.createElementNS(ns, "path");
    path.setAttribute("d", pathD);
    path.setAttribute("fill", "none");
    path.setAttribute("stroke", "none");
    path.setAttribute("id", "hf-path-" + Math.random().toString(36).slice(2));
    svg.appendChild(path);
    var dot = document.createElementNS(ns, "circle");
    dot.setAttribute("r", "4");
    dot.setAttribute("class", "hf-flow-dot");
    var anim = document.createElementNS(ns, "animateMotion");
    anim.setAttribute("dur", duration || "2.5s");
    anim.setAttribute("repeatCount", "indefinite");
    anim.setAttribute("path", pathD);
    dot.appendChild(anim);
    svg.appendChild(dot);
  }

  function wireClickNodes(svg, sh, labels, defaultFooter) {
    var active = null;

    function reset() {
      active = null;
      svg.querySelectorAll(".hf-node").forEach(function (x) {
        x.classList.remove("hf-active");
      });
      sh.footer.textContent = defaultFooter;
    }

    function selectNode(n) {
      var id = n.getAttribute("data-id");
      active = n;
      svg.querySelectorAll(".hf-node").forEach(function (x) {
        x.classList.remove("hf-active");
      });
      n.classList.add("hf-active");
      var label = n.querySelector("text").textContent;
      sh.footer.innerHTML =
        "<strong>" + label + ":</strong> " + labels[id];
    }

    svg.querySelectorAll(".hf-node").forEach(function (n) {
      n.addEventListener("click", function (ev) {
        ev.stopPropagation();
        if (active === n) {
          reset();
        } else {
          selectNode(n);
        }
      });
    });

    sh.body.addEventListener("click", function (ev) {
      if (!ev.target.closest(".hf-node")) {
        reset();
      }
    });

    return reset;
  }

  var registry = {
    "parallel-paths": function () {
      var s = svgEl(640, 280);
      var gid = defsGradient(s, "hf-flow-gradient");
      box(s, 250, 20, 140, 44, "IQ stream", "one ring buffer", C.accent, "iq");
      line(s, 320, 64, 320, 95, true, gid);
      line(s, 320, 95, 110, 130, true, gid);
      line(s, 320, 95, 320, 130, true, gid);
      line(s, 320, 95, 530, 130, true, gid);
      animateDot(s, "M320,64 L320,95 L110,130", "2.2s");
      animateDot(s, "M320,64 L320,95 L320,180", "2s");
      animateDot(s, "M320,64 L320,95 L530,130", "2.4s");
      box(s, 40, 130, 140, 56, "Listen", "CwChannel → audio", C.ok, "listen");
      box(s, 250, 130, 140, 56, "Panadapter", "FFT → waterfall", C.accent, "pan");
      box(s, 460, 130, 140, 56, "Skimmer", "many decoders", C.warn, "skim");
      var info = {
        iq: "All paths share the same IQ samples — looking never steals from listening.",
        listen: "One station, sharpest filters. Audio to your headphones.",
        pan: "See the whole band at once. Zoom and pan are visual-only.",
        skim: "Parallel CW decoders copy many callers into the spot table.",
      };
      var sh = shell(
        "Parallel signal paths",
        "Click a path",
        s,
        null
      );
      sh.footer.textContent = info.iq;
      wireClickNodes(s, sh, info, info.iq);
      return sh.wrap;
    },

    "thread-map": function () {
      var s = svgEl(700, 340);
      var gid = defsGradient(s, "hf-flow-gradient");
      box(s, 250, 16, 200, 48, "Main thread", "egui · try_poll", C.muted, "main");
      line(s, 350, 64, 350, 88, true, gid);
      box(s, 220, 88, 260, 52, "Engine", "drain · demod · FFT", C.accent, "engine");
      line(s, 280, 140, 90, 175, true, gid);
      line(s, 350, 140, 350, 175, true, gid);
      line(s, 420, 140, 500, 175, true, gid);
      line(s, 480, 140, 610, 175, true, gid);
      box(s, 20, 175, 140, 48, "Source", "producer", C.ok, "src");
      box(s, 280, 175, 140, 48, "Audio", "cpal callback", C.ok, "audio");
      box(s, 430, 175, 140, 48, "Skimmer", "decoders", C.warn, "skimmer");
      box(s, 580, 175, 110, 48, "Ingress", "decim", C.accentDim, "ingress");
      box(s, 120, 250, 120, 40, "iq-record", "", C.muted, "rec");
      box(s, 480, 250, 120, 40, "iq-playback", "", C.muted, "play");
      line(s, 350, 140, 180, 250, false);
      line(s, 350, 140, 540, 250, false);
      var meta = {
        main: "Draws UI only. Sends EngineCommand, never blocks on DSP.",
        engine: "Owns IQ ring consumer, connection, and all real-time paths.",
        src: "Kiwi reader, Airspy USB, RTL async, or QMX audio — pushes to rtrb.",
        audio: "cpal output callback reads resampled mono from a ring.",
        skimmer: "Gets copies of IQ blocks; parallel rayon decoders.",
        ingress: "Wideband FIR decimation overlapped with demod (hfsdr-ingress).",
        rec: "Gzip writer thread — engine push never blocks on disk.",
        play: "Decompresses IQ file into a ring for offline replay.",
      };
      var defaultFooter = "Click a box for details · click again or outside to reset.";
      var sh = shell("Thread map", "Click a node", s, null);
      sh.footer.textContent = defaultFooter;
      wireClickNodes(s, sh, meta, defaultFooter);
      return sh.wrap;
    },

    "signal-journey": function () {
      var s = svgEl(700, 200);
      var gid = defsGradient(s, "hf-flow-gradient");
      var stages = [
        { x: 20, label: "IQ block", sub: "4096 samples", c: C.accent },
        { x: 155, label: "Listen", sub: "CwChannel", c: C.ok },
        { x: 290, label: "Spectrum", sub: "FFT row", c: C.accent },
        { x: 425, label: "Skimmer", sub: "peaks", c: C.warn },
        { x: 560, label: "Output", sub: "GUI + audio", c: C.ok },
      ];
      stages.forEach(function (st, i) {
        box(s, st.x, 70, 115, 52, st.label, st.sub, st.c, "s" + i);
        if (i < stages.length - 1) {
          line(s, st.x + 115, 96, stages[i + 1].x, 96, true, gid);
          animateDot(
            s,
            "M" +
              (st.x + 115) +
              ",96 L" +
              stages[i + 1].x +
              ",96",
            1.8 + i * 0.2 + "s"
          );
        }
      });
      var sh = shell(
        "One engine pump",
        "Shared input · separate filters",
        s,
        null
      );
      sh.footer.textContent =
        "Widening the panadapter zoom does not widen your CW filter.";
      return sh.wrap;
    },

    "gui-layout": function () {
      var s = svgEl(640, 300);
      var regions = [
        { x: 8, y: 8, w: 624, h: 28, id: "status", label: "Status bar", c: C.panel },
        { x: 8, y: 44, w: 148, h: 200, id: "left", label: "Left panel", c: C.raised },
        { x: 164, y: 44, w: 320, h: 200, id: "plot", label: "Spectrum + waterfall", c: C.accentDim },
        { x: 492, y: 44, w: 140, h: 200, id: "right", label: "CW / skimmer", c: C.raised },
        { x: 8, y: 252, w: 624, h: 40, id: "bottom", label: "History · log · pipeline", c: C.panel },
      ];
      var hints = {
        status: "Link state, SNR, drops, panel toggles.",
        left: "Connection, recent hosts, spot table.",
        plot: "Click to tune. Cyan edges = listen passband.",
        right: "Filter shape, BFO, skimmer settings.",
        bottom: "Optional panels — pipeline shows live thread diagram.",
      };
      regions.forEach(function (r) {
        var ns = "http://www.w3.org/2000/svg";
        var g = document.createElementNS(ns, "g");
        g.setAttribute("class", "hf-node");
        g.setAttribute("data-id", r.id);
        var rect = document.createElementNS(ns, "rect");
        rect.setAttribute("x", r.x);
        rect.setAttribute("y", r.y);
        rect.setAttribute("width", r.w);
        rect.setAttribute("height", r.h);
        rect.setAttribute("rx", "8");
        rect.setAttribute("fill", r.c);
        rect.setAttribute("stroke", C.grid);
        rect.setAttribute("stroke-width", "1.5");
        rect.setAttribute("opacity", r.id === "plot" ? "0.85" : "0.65");
        g.appendChild(rect);
        var t = document.createElementNS(ns, "text");
        t.setAttribute("x", r.x + r.w / 2);
        t.setAttribute("y", r.y + r.h / 2);
        t.setAttribute("text-anchor", "middle");
        t.setAttribute("dominant-baseline", "middle");
        t.setAttribute("fill", C.text);
        t.setAttribute("font-size", r.id === "plot" ? "13" : "11");
        t.setAttribute("font-weight", "600");
        t.textContent = r.label;
        g.appendChild(t);
        if (r.id === "plot") {
          var band = document.createElementNS(ns, "rect");
          band.setAttribute("x", r.x + 120);
          band.setAttribute("y", r.y + 20);
          band.setAttribute("width", 80);
          band.setAttribute("height", r.h - 40);
          band.setAttribute("fill", "none");
          band.setAttribute("stroke", C.accent);
          band.setAttribute("stroke-width", "2");
          band.setAttribute("opacity", "0.9");
          g.appendChild(band);
        }
        s.appendChild(g);
      });
      var sh = shell("Receiver window", "Click a region", s, null);
      sh.footer.textContent = hints.plot;
      wireClickNodes(s, sh, hints, hints.plot);
      return sh.wrap;
    },

    "iq-chain": function () {
      var s = svgEl(400, 260);
      var gid = defsGradient(s, "hf-flow-gradient");
      box(s, 130, 16, 140, 40, "RF band", "at Fc", C.muted, "rf");
      line(s, 200, 56, 200, 78, true, gid);
      box(s, 110, 78, 180, 44, "Mixer + ADC", "", C.accentDim, "mix");
      line(s, 200, 122, 200, 144, true, gid);
      box(s, 100, 144, 200, 44, "(I, Q) samples", "complex stream", C.accent, "iq");
      line(s, 200, 188, 200, 210, true, gid);
      box(s, 90, 210, 220, 44, "rtrb ring", "engine thread", C.ok, "eng");
      animateDot(s, "M200,56 L200,78 L200,122 L200,144 L200,188 L200,210", "3s");
      var sh = shell("From antenna to engine", "", s, null);
      sh.footer.textContent =
        "Producer never blocks on UI — overload increments drop counter.";
      return sh.wrap;
    },

    "iq-phasor": function () {
      var canvas = document.createElement("canvas");
      setupCanvas(canvas, 480, 220);
      canvas.className = "hf-diagram-canvas-iq-phasor";
      var ctx = canvas.getContext("2d");
      var phase = 0;
      var keyed = true;
      var raf;

      canvas.addEventListener("click", function () {
        keyed = !keyed;
      });

      function draw() {
        var w = canvas.width;
        var h = canvas.height;
        ctx.fillStyle = C.raised;
        ctx.fillRect(0, 0, w, h);
        ctx.strokeStyle = C.grid;
        ctx.lineWidth = 1;
        ctx.beginPath();
        ctx.moveTo(40, h / 2);
        ctx.lineTo(w - 20, h / 2);
        ctx.moveTo(w / 2, 20);
        ctx.lineTo(w / 2, h - 20);
        ctx.stroke();
        ctx.fillStyle = C.muted;
        ctx.font = "11px DM Sans, sans-serif";
        ctx.fillText("I", w - 16, h / 2 - 6);
        ctx.fillText("Q", w / 2 + 6, 24);
        var amp = keyed ? 1 : 0.15;
        var cx = w / 2;
        var cy = h / 2;
        var rad = 70 * amp;
        var px = cx + Math.cos(phase) * rad;
        var py = cy - Math.sin(phase) * rad;
        ctx.strokeStyle = C.accentDim;
        ctx.lineWidth = 2;
        ctx.beginPath();
        ctx.moveTo(cx, cy);
        ctx.lineTo(px, py);
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(cx, cy, rad, 0, Math.PI * 2);
        ctx.strokeStyle = C.grid;
        ctx.lineWidth = 1;
        ctx.stroke();
        ctx.beginPath();
        ctx.arc(px, py, 7, 0, Math.PI * 2);
        ctx.fillStyle = keyed ? C.ok : C.muted;
        ctx.fill();
        ctx.fillStyle = C.text;
        ctx.font = "12px DM Sans, sans-serif";
        ctx.fillText(
          keyed ? "CW keyed — phasor rotating" : "CW idle — click to key",
          16,
          h - 14
        );
        if (!reducedMotion()) phase += keyed ? 0.04 : 0.01;
        raf = requestAnimationFrame(draw);
      }

      var sh = shell("IQ phasor", "Click to toggle key", canvas, null);
      sh.footer.textContent =
        "+500 Hz offset appears as a steady rotation in the I/Q plane.";
      draw();
      sh.wrap._hfCleanup = function () {
        cancelAnimationFrame(raf);
      };
      return sh.wrap;
    },

    "spectrum-slice": function () {
      var canvas = document.createElement("canvas");
      setupCanvas(canvas, 520, 120);
      canvas.className = "hf-diagram-canvas-spectrum-slice";
      var ctx = canvas.getContext("2d");
      var t = 0;
      var raf;

      function draw() {
        var w = canvas.width;
        var h = canvas.height;
        ctx.fillStyle = C.raised;
        ctx.fillRect(0, 0, w, h);
        var floor = h * 0.72;
        ctx.strokeStyle = C.grid;
        ctx.beginPath();
        ctx.moveTo(24, floor);
        ctx.lineTo(w - 12, floor);
        ctx.stroke();
        ctx.fillStyle = C.muted;
        ctx.font = "10px DM Sans, sans-serif";
        ctx.fillText("noise floor", 28, floor - 6);
        ctx.fillText("frequency →", w - 88, h - 8);
        for (var x = 30; x < w - 20; x++) {
          var n = (Math.sin(x * 0.08 + t) + Math.sin(x * 0.23)) * 3;
          ctx.fillStyle = C.grid;
          ctx.fillRect(x, floor - 4 + n, 2, 4 - n);
        }
        var peak = w * 0.55 + Math.sin(t * 0.5) * 8;
        var pulse = 0.65 + 0.35 * (0.5 + 0.5 * Math.sin(t * 3));
        ctx.fillStyle = C.ok;
        for (var i = -12; i <= 12; i++) {
          var hgt = (28 - Math.abs(i)) * pulse;
          ctx.globalAlpha = 0.4 + 0.6 * (1 - Math.abs(i) / 12);
          ctx.fillRect(peak + i * 2, floor - hgt, 3, hgt);
        }
        ctx.globalAlpha = 1;
        ctx.fillStyle = C.accent;
        ctx.font = "11px DM Sans, sans-serif";
        ctx.fillText("CW +500 Hz", peak - 24, 22);
        if (!reducedMotion()) t += 0.03;
        raf = requestAnimationFrame(draw);
      }

      var sh = shell("Spectrum slice", "Animated trace", canvas, null);
      sh.footer.textContent =
        "Panadapter FFT maps IQ snapshot → power vs offset frequency.";
      draw();
      sh.wrap._hfCleanup = function () {
        cancelAnimationFrame(raf);
      };
      return sh.wrap;
    },
  };

  function mount(el) {
    var kind = el.getAttribute("data-diagram");
    if (!kind || !registry[kind]) return;
    if (el.getAttribute("data-hf-mounted") === kind) return;
    if (el._hfCleanup) {
      el._hfCleanup();
      el._hfCleanup = null;
    }
    el.setAttribute("data-hf-mounted", kind);
    el.innerHTML = "";
    var node = registry[kind]();
    el.appendChild(node);
    if (node._hfCleanup) {
      el._hfCleanup = node._hfCleanup;
    }
  }

  function mountAll(root) {
    (root || document).querySelectorAll("[data-diagram]").forEach(mount);
  }

  function cleanup(root) {
    (root || document).querySelectorAll("[data-diagram]").forEach(function (el) {
      if (el._hfCleanup) {
        el._hfCleanup();
        el._hfCleanup = null;
      }
      el.removeAttribute("data-hf-mounted");
      el.innerHTML = "";
    });
  }

  global.HfDiagrams = { mount: mount, mountAll: mountAll, cleanup: cleanup };
})(typeof window !== "undefined" ? window : globalThis);
