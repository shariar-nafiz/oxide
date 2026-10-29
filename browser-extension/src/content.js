// Oxide Media Sniffer & Shadow DOM Floating Overlay Content Script
(function () {
  const attachedVideos = new WeakSet();
  let detectedStreams = [];

  chrome.runtime.onMessage.addListener((msg, _sender, sendResponse) => {
    if (!msg) return;
    if (msg.action === "OxideMediaDetected" && Array.isArray(msg.streams)) {
      detectedStreams = msg.streams;
      scanVideos();
    } else if (msg.action === "OxideCollectAllLinks") {
      const anchors = Array.from(document.querySelectorAll("a[href]"));
      const downloadableExts = [
        ".zip", ".tar", ".gz", ".7z", ".rar", ".iso", ".exe", ".deb", ".rpm",
        ".dmg", ".AppImage", ".mp4", ".mkv", ".mp3", ".flac", ".pdf"
      ];
      const links = [];
      for (const a of anchors) {
        const href = a.href;
        if (!href || !href.startsWith("http")) continue;
        const lower = href.split("?")[0].toLowerCase();
        if (downloadableExts.some((ext) => lower.endsWith(ext))) {
          if (!links.includes(href)) links.push(href);
        }
      }
      sendResponse({ links });
    }
  });

  function scanVideos() {
    const videos = document.querySelectorAll("video");
    videos.forEach((video) => {
      if (attachedVideos.has(video)) return;
      attachedVideos.add(video);
      injectShadowOverlay(video);
    });
  }

  function injectShadowOverlay(video) {
    const host = document.createElement("div");
    host.style.position = "absolute";
    host.style.zIndex = "2147483647";
    host.style.pointerEvents = "auto";

    const shadow = host.attachShadow({ mode: "closed" });
    const btn = document.createElement("button");
    btn.textContent = "Download Media";
    btn.style.background = "rgba(15, 23, 42, 0.92)";
    btn.style.color = "#38bdf8";
    btn.style.border = "1px solid rgba(56, 189, 248, 0.45)";
    btn.style.borderRadius = "6px";
    btn.style.padding = "5px 11px";
    btn.style.fontSize = "12px";
    btn.style.fontWeight = "600";
    btn.style.fontFamily = "Inter, system-ui, sans-serif";
    btn.style.cursor = "pointer";
    btn.style.boxShadow = "0 4px 14px rgba(0, 0, 0, 0.45)";
    btn.style.transition = "all 0.15s ease";

    btn.onmouseenter = () => {
      btn.style.background = "#2563eb";
      btn.style.color = "#ffffff";
    };
    btn.onmouseleave = () => {
      btn.style.background = "rgba(15, 23, 42, 0.92)";
      btn.style.color = "#38bdf8";
    };

    btn.onclick = (e) => {
      e.stopPropagation();
      e.preventDefault();
      const candidateUrl =
        (detectedStreams.length > 0 && detectedStreams[0].url) ||
        video.currentSrc ||
        video.src;

      chrome.runtime.sendMessage(
        {
          action: "DownloadMedia",
          url: candidateUrl,
          filename: document.title
            ? document.title.replace(/[^a-zA-Z0-9._-]/g, "_") + ".mp4"
            : null
        },
        (resp) => {
          if (resp && resp.ok) {
            btn.textContent = "Sent to Oxide";
            setTimeout(() => {
              btn.textContent = "Download Media";
            }, 2000);
          }
        }
      );
    };

    shadow.appendChild(btn);

    const updatePosition = () => {
      const rect = video.getBoundingClientRect();
      if (rect.width > 80 && rect.height > 60) {
        host.style.display = "block";
        host.style.top = `${window.scrollY + rect.top + 10}px`;
        host.style.left = `${window.scrollX + rect.left + 10}px`;
      } else {
        host.style.display = "none";
      }
    };

    (document.body || document.documentElement).appendChild(host);
    updatePosition();
    window.addEventListener("scroll", updatePosition, { passive: true });
    window.addEventListener("resize", updatePosition, { passive: true });
  }

  const observer = new MutationObserver(scanVideos);
  observer.observe(document.documentElement, {
    childList: true,
    subtree: true
  });

  scanVideos();
})();
