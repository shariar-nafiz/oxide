// Oxide Video Stream Detector & Floating Pill Content Script
(function () {
  const scannedVideos = new WeakSet();

  function scanMedia() {
    const videos = document.querySelectorAll("video");
    videos.forEach((video) => {
      if (scannedVideos.has(video)) return;
      scannedVideos.add(video);

      attachVideoPill(video);
    });
  }

  function attachVideoPill(video) {
    const src = video.currentSrc || video.src;
    if (!src || src.startsWith("blob:")) return;

    // Create minimal floating pill in Shadow DOM to avoid CSS collisions
    const container = document.createElement("div");
    container.style.position = "absolute";
    container.style.zIndex = "999999";
    container.style.pointerEvents = "auto";

    const shadow = container.attachShadow({ mode: "closed" });
    const pill = document.createElement("button");
    pill.textContent = "⚡ Download with Oxide";
    pill.style.background = "rgba(18, 20, 24, 0.85)";
    pill.style.color = "#00d2ff";
    pill.style.border = "1px solid rgba(0, 210, 255, 0.4)";
    pill.style.borderRadius = "20px";
    pill.style.padding = "6px 12px";
    pill.style.fontSize = "12px";
    pill.style.fontWeight = "600";
    pill.style.fontFamily = "system-ui, sans-serif";
    pill.style.cursor = "pointer";
    pill.style.backdropFilter = "blur(8px)";
    pill.style.transition = "all 0.2s ease";

    pill.onmouseenter = () => {
      pill.style.background = "#00d2ff";
      pill.style.color = "#121418";
    };
    pill.onmouseleave = () => {
      pill.style.background = "rgba(18, 20, 24, 0.85)";
      pill.style.color = "#00d2ff";
    };

    pill.onclick = (e) => {
      e.stopPropagation();
      e.preventDefault();
      chrome.runtime.sendMessage({
        action: "DownloadMedia",
        url: src
      });
    };

    shadow.appendChild(pill);

    // Position relative to video element
    const updatePosition = () => {
      const rect = video.getBoundingClientRect();
      if (rect.width > 0 && rect.height > 0) {
        container.style.top = `${window.scrollY + rect.top + 12}px`;
        container.style.left = `${window.scrollX + rect.left + 12}px`;
      }
    };

    document.body.appendChild(container);
    updatePosition();
    window.addEventListener("resize", updatePosition);
  }

  // Observe dynamically added video elements
  const observer = new MutationObserver(scanMedia);
  observer.observe(document.body || document.documentElement, {
    childList: true,
    subtree: true
  });

  scanMedia();
})();
