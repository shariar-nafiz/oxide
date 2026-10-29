// Oxide WebExtension Background Service Worker
const NMH_HOST = "org.oxide.nmh";
const HEADER_CACHE = new Map();

// Intercept outgoing headers to preserve User-Agent, Cookies, Referer, and Authorization
chrome.webRequest.onBeforeSendHeaders.addListener(
  (details) => {
    const headers = {};
    if (details.requestHeaders) {
      for (const h of details.requestHeaders) {
        headers[h.name.toLowerCase()] = h.value;
      }
    }
    HEADER_CACHE.set(details.url, {
      headers,
      timestamp: Date.now()
    });

    // Clean old cache entries
    if (HEADER_CACHE.size > 200) {
      const oldestKey = HEADER_CACHE.keys().next().value;
      HEADER_CACHE.delete(oldestKey);
    }
  },
  { urls: ["<all_urls>"] },
  ["requestHeaders", "extraHeaders"]
);

// Intercept browser download triggers
chrome.downloads.onCreated.addListener(async (downloadItem) => {
  // Avoid loop interception for files already being managed or blobs
  if (downloadItem.url.startsWith("blob:") || downloadItem.url.startsWith("data:")) {
    return;
  }

  // Cancel native browser download
  chrome.downloads.cancel(downloadItem.id, () => {
    chrome.downloads.erase({ id: downloadItem.id });
  });

  const cached = HEADER_CACHE.get(downloadItem.url);
  const headers = cached ? cached.headers : {};

  // Hand off to Oxide daemon via Native Messaging Host
  sendToOxide({
    action: "AddDownload",
    payload: {
      url: downloadItem.url,
      filename: downloadItem.filename ? downloadItem.filename.split("/").pop() : null,
      save_dir: null,
      headers: headers,
      max_segments: 16,
      speed_limit_bps: null
    }
  });
});

// Setup Context Menus
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "oxide-download-link",
    title: "Download with Oxide",
    contexts: ["link", "video", "audio"]
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "oxide-download-link") {
    const targetUrl = info.linkUrl || info.srcUrl;
    if (targetUrl) {
      const cached = HEADER_CACHE.get(targetUrl);
      sendToOxide({
        action: "AddDownload",
        payload: {
          url: targetUrl,
          filename: null,
          save_dir: null,
          headers: cached ? cached.headers : {},
          max_segments: 16,
          speed_limit_bps: null
        }
      });
    }
  }
});

function sendToOxide(message) {
  try {
    const port = chrome.runtime.connectNative(NMH_HOST);
    port.postMessage(message);
    port.onMessage.addListener((response) => {
      console.log("Oxide NMH response:", response);
      port.disconnect();
    });
    port.onDisconnect.addListener(() => {
      if (chrome.runtime.lastError) {
        console.warn("Oxide NMH connection notice:", chrome.runtime.lastError.message);
      }
    });
  } catch (err) {
    console.error("Failed to connect to Oxide NMH:", err);
  }
}
