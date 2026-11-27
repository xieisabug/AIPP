// AIPP Artifact 运行时诊断：收集 console 输出、未捕获异常、资源加载失败与 Vite 编译错误。
// - 记录到 window.__aippDiag，供后台无头检查读取（show_artifact 的运行检查）；
// - 处于 iframe 内时 postMessage 给父窗口，供预览界面的「控制台」显示；
// - 父窗口发来 { __aippArtifactPick: "start" } 时进入点选模式，点中的元素描述发回父窗口（「选择元素修改」）。
// 同一份脚本被 HTML 预览注入、React/Vue 模板加载（public/aipp-console.js）与 Rust include_str! 共用。
// 内联注入时会去掉整行注释并压成一行（保持页面报错行号不变）：只写整行 // 注释，语句都以分号结尾。
(function () {
  if (window.__aippConsoleInstalled) return;
  window.__aippConsoleInstalled = true;
  var MAX_ENTRIES = 200;
  var MAX_MESSAGE = 4000;
  var diag = (window.__aippDiag = []);

  function toText(value) {
    if (value instanceof Error) return value.stack || value.name + ": " + value.message;
    if (typeof value === "string") return value;
    try {
      return JSON.stringify(value);
    } catch (e) {
      return String(value);
    }
  }

  function report(level, parts) {
    var message = parts.map(toText).join(" ");
    if (message.length > MAX_MESSAGE) message = message.slice(0, MAX_MESSAGE) + "…";
    var entry = { level: level, message: message, time: Date.now() };
    if (diag.length < MAX_ENTRIES) diag.push(entry);
    if (window.parent && window.parent !== window) {
      try {
        window.parent.postMessage({ __aippArtifactConsole: 1, entry: entry }, "*");
      } catch (e) {
        /* 父窗口不可达时只保留本地记录 */
      }
    }
  }

  ["log", "info", "warn", "error", "debug"].forEach(function (level) {
    var original = console[level];
    console[level] = function () {
      report(level, Array.prototype.slice.call(arguments));
      return original.apply(console, arguments);
    };
  });

  window.addEventListener(
    "error",
    function (event) {
      var target = event.target;
      if (target && target !== window && (target.src || target.href)) {
        report("error", ["资源加载失败: " + (target.src || target.href)]);
        return;
      }
      var where = event.filename
        ? " (" + event.filename + ":" + event.lineno + ":" + event.colno + ")"
        : "";
      report("error", [(event.error && event.error.stack) || (event.message || "Error") + where]);
    },
    true
  );

  window.addEventListener("unhandledrejection", function (event) {
    report("error", ["Unhandled promise rejection: " + toText(event.reason)]);
  });

  // Vite 的编译错误显示在 <vite-error-overlay>（open shadow DOM）里，不会进入 console。
  var seen = typeof WeakSet === "function" ? new WeakSet() : null;
  function readOverlay(node) {
    if (!node || node.tagName !== "VITE-ERROR-OVERLAY" || (seen && seen.has(node))) return;
    if (seen) seen.add(node);
    setTimeout(function () {
      var root = node.shadowRoot;
      function pick(selector) {
        var el = root && root.querySelector(selector);
        return el && el.textContent ? el.textContent.trim() : "";
      }
      var text = [pick(".message"), pick(".file"), pick(".frame")].filter(Boolean).join("\n");
      report("error", ["[vite] " + (text || "编译错误")]);
    }, 0);
  }
  function watchOverlay() {
    new MutationObserver(function (records) {
      records.forEach(function (record) {
        Array.prototype.forEach.call(record.addedNodes, readOverlay);
      });
    }).observe(document.documentElement, { childList: true, subtree: true });
    Array.prototype.forEach.call(document.getElementsByTagName("vite-error-overlay"), readOverlay);
  }
  if (document.documentElement) watchOverlay();
  else document.addEventListener("DOMContentLoaded", watchOverlay);

  // 点选元素：悬停描边，点击后把元素的选择器、文本和 HTML 片段发给父窗口。只接受父窗口的指令。
  var picking = false;
  var overlay = null;
  var hovered = null;
  function postPick(state, element) {
    try {
      window.parent.postMessage({ __aippArtifactPick: 1, state: state, element: element || null }, "*");
    } catch (e) {
      /* 父窗口不可达 */
    }
  }
  function simpleClasses(el) {
    var raw = typeof el.className === "string" ? el.className : (el.getAttribute && el.getAttribute("class")) || "";
    return raw.trim().split(/\s+/).filter(function (name) {
      return /^[A-Za-z_-][\w-]*$/.test(name);
    }).slice(0, 3);
  }
  function selectorFor(el) {
    var parts = [];
    while (el && el.nodeType === 1 && el !== document.documentElement && el !== document.body && parts.length < 6) {
      var part = el.tagName.toLowerCase();
      if (el.id && /^[A-Za-z][\w-]*$/.test(el.id)) {
        parts.unshift(part + "#" + el.id);
        break;
      }
      var classes = simpleClasses(el);
      if (classes.length) part += "." + classes.join(".");
      var parent = el.parentElement;
      if (parent) {
        var tag = el.tagName;
        var same = Array.prototype.filter.call(parent.children, function (child) {
          return child.tagName === tag;
        });
        if (same.length > 1) part += ":nth-of-type(" + (same.indexOf(el) + 1) + ")";
      }
      parts.unshift(part);
      el = parent;
    }
    return parts.join(" > ");
  }
  function describe(el) {
    var rect = el.getBoundingClientRect();
    var text = (el.innerText || el.textContent || "").replace(/\s+/g, " ").trim();
    var html = el.outerHTML || "";
    return {
      tag: el.tagName.toLowerCase(),
      selector: selectorFor(el),
      text: text.length > 160 ? text.slice(0, 160) + "…" : text,
      html: html.length > 1200 ? html.slice(0, 1200) + "…" : html,
      rect: { x: Math.round(rect.left), y: Math.round(rect.top), width: Math.round(rect.width), height: Math.round(rect.height) }
    };
  }
  function moveOverlay(el) {
    if (!overlay) return;
    var rect = el.getBoundingClientRect();
    overlay.style.left = rect.left + "px";
    overlay.style.top = rect.top + "px";
    overlay.style.width = rect.width + "px";
    overlay.style.height = rect.height + "px";
    overlay.firstChild.textContent = el.tagName.toLowerCase() + (el.id ? "#" + el.id : "");
  }
  function onMove(event) {
    var el = event.target;
    if (!el || el.nodeType !== 1 || el === overlay || el === hovered) return;
    hovered = el;
    moveOverlay(el);
  }
  function swallow(event) {
    event.preventDefault();
    event.stopPropagation();
    if (event.stopImmediatePropagation) event.stopImmediatePropagation();
  }
  function onClick(event) {
    swallow(event);
    var el = event.target && event.target.nodeType === 1 ? event.target : hovered;
    stopPick();
    if (el) postPick("picked", describe(el));
  }
  function onKey(event) {
    if (event.key !== "Escape") return;
    swallow(event);
    stopPick();
    postPick("cancelled");
  }
  function startPick() {
    if (picking || !document.body) return;
    picking = true;
    overlay = document.createElement("div");
    overlay.setAttribute("data-aipp-pick-overlay", "");
    overlay.style.cssText = "position:fixed;z-index:2147483647;pointer-events:none;box-sizing:border-box;border:2px solid #111;background:rgba(17,17,17,.08);box-shadow:0 0 0 1px #fff;left:0;top:0;width:0;height:0;";
    var label = document.createElement("span");
    label.style.cssText = "position:absolute;left:-2px;top:-20px;padding:1px 6px;font:11px/16px ui-monospace,monospace;color:#fff;background:#111;white-space:nowrap;";
    overlay.appendChild(label);
    document.body.appendChild(overlay);
    document.addEventListener("mousemove", onMove, true);
    document.addEventListener("click", onClick, true);
    document.addEventListener("mousedown", swallow, true);
    document.addEventListener("mouseup", swallow, true);
    document.addEventListener("keydown", onKey, true);
    document.documentElement.style.cursor = "crosshair";
  }
  function stopPick() {
    if (!picking) return;
    picking = false;
    hovered = null;
    document.removeEventListener("mousemove", onMove, true);
    document.removeEventListener("click", onClick, true);
    document.removeEventListener("mousedown", swallow, true);
    document.removeEventListener("mouseup", swallow, true);
    document.removeEventListener("keydown", onKey, true);
    document.documentElement.style.cursor = "";
    if (overlay && overlay.parentNode) overlay.parentNode.removeChild(overlay);
    overlay = null;
  }
  window.addEventListener("message", function (event) {
    if (event.source !== window.parent || window.parent === window) return;
    var command = event.data && event.data.__aippArtifactPick;
    if (command === "start") startPick();
    else if (command === "stop") stopPick();
  });
})();
