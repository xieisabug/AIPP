// AIPP Artifact 运行时诊断：收集 console 输出、未捕获异常、资源加载失败与 Vite 编译错误。
// - 记录到 window.__aippDiag，供后台无头检查读取（show_artifact 的运行检查）；
// - 处于 iframe 内时 postMessage 给父窗口，供预览界面的「控制台」显示。
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
})();
