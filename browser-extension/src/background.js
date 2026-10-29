// Oxide WebExtension Background Service Worker (Manifest V3)
const NMH_HOST = "org.oxide.nmh";
const HTTP_RPC_URL = "http://127.0.0.1:46890";
const HEADER_CACHE = new Map();
const TAB_MEDIA_STREAMS = new Map(); // tabId -> Array<{ url, type, timestamp }>

const MEDIA_EXTENSIONS = [".mp4", ".mkv", ".webm", ".m3u8", ".mpd", ".ts", ".m4s"];

// 1. Monitor outgoing requests for headers + media streams (.mp4, .mkv, .m3u8, .mpd)
chrome.webRequest.onBeforeSendHeaders.addListener(
  (details) => {
    const headers = {};
    if (details.requestHeaders) {
      for (const h of details.requestHeaders) {
        headers[h.name] = h.value;
      }
    }
    HEADER_CACHE.set(details.url, {
      headers,
      timestamp: Date.now()
    });

    if (HEADER_CACHE.size > 300) {
      const oldestKey = HEADER_CACHE.keys().next().value;
      HEADER_CACHE.delete(oldestKey);
    }

    const lowerUrl = details.url.split("?")[0].toLowerCase();
    const matchedExt = MEDIA_EXTENSIONS.find((ext) => lowerUrl.endsWith(ext));
    if (matchedExt && details.tabId >= 0) {
      const list = TAB_MEDIA_STREAMS.get(details.tabId) || [];
      if (!list.some((item) => item.url === details.url)) {
        list.unshift({
          url: details.url,
          ext: matchedExt,
          headers,
          timestamp: Date.now()
        });
        if (list.length > 25) list.pop();
        TAB_MEDIA_STREAMS.set(details.tabId, list);

        chrome.tabs.sendMessage(details.tabId, {
          action: "OxideMediaDetected",
          streams: list
        }).catch(() => {});
      }
    }
  },
  { urls: ["<all_urls>"] },
  ["requestHeaders"]
);

// 2. Intercept browser downloads via onDeterminingFilename (or onCreated fallback)
function handleInterceptedDownload(downloadItem) {
  const url = downloadItem.finalUrl || downloadItem.url;
  if (!url || url.startsWith("blob:") || url.startsWith("data:")) {
    return;
  }

  chrome.downloads.cancel(downloadItem.id, () => {
    if (chrome.downloads.erase) {
      chrome.downloads.erase({ id: downloadItem.id });
    }
  });

  const cached = HEADER_CACHE.get(url) || HEADER_CACHE.get(downloadItem.url);
  const headers = cached ? { ...cached.headers } : {};
  if (downloadItem.referrer) {
    headers["Referer"] = downloadItem.referrer;
  }

  chrome.cookies.getAll({ url }, (cookies) => {
    if (cookies && cookies.length > 0) {
      headers["Cookie"] = cookies.map((c) => `${c.name}=${c.value}`).join("; ");
    }
    const rawFilename = downloadItem.filename
      ? downloadItem.filename.replace(/\\/g, "/").split("/").pop()
      : null;

    dispatchDownload({
      url,
      filename: rawFilename || null,
      referrer: downloadItem.referrer || headers["Referer"] || null,
      cookies: headers["Cookie"] || null,
      userAgent: navigator.userAgent,
      headers
    });
  });
}

if (chrome.downloads.onDeterminingFilename) {
  chrome.downloads.onDeterminingFilename.addListener((downloadItem, suggest) => {
    suggest();
    handleInterceptedDownload(downloadItem);
  });
} else {
  chrome.downloads.onCreated.addListener(handleInterceptedDownload);
}

// 3. Context Menus: "Download link with App" and "Download all links on page"
chrome.runtime.onInstalled.addListener(() => {
  chrome.contextMenus.create({
    id: "oxide-download-link",
    title: "Download link with App",
    contexts: ["link", "video", "audio"]
  });

  chrome.contextMenus.create({
    id: "oxide-download-all-links",
    title: "Download all links on page",
    contexts: ["page"]
  });
});

chrome.contextMenus.onClicked.addListener((info, tab) => {
  if (info.menuItemId === "oxide-download-link") {
    const targetUrl = info.linkUrl || info.srcUrl;
    if (targetUrl) {
      const cached = HEADER_CACHE.get(targetUrl);
      dispatchDownload({
        url: targetUrl,
        filename: null,
        referrer: info.pageUrl || null,
        cookies: null,
        userAgent: navigator.userAgent,
        headers: cached ? cached.headers : {}
      });
    }
  } else if (info.menuItemId === "oxide-download-all-links" && tab && tab.id >= 0) {
    chrome.tabs.sendMessage(tab.id, { action: "OxideCollectAllLinks" }, (response) => {
      if (response && Array.isArray(response.links)) {
        for (const linkUrl of response.links) {
          dispatchDownload({
            url: linkUrl,
            filename: null,
            referrer: tab.url || null,
            cookies: null,
            userAgent: navigator.userAgent,
            headers: {}
          });
        }
      }
    });
  }
});

// 4. Listen for messages from the Shadow DOM Floating Media Button in content.js
chrome.runtime.onMessage.addListener((msg, sender, sendResponse) => {
  if (msg && msg.action === "DownloadMedia") {
    const tabId = sender.tab ? sender.tab.id : -1;
    let targetUrl = msg.url;

    // If video uses MSE blob: URL, resolve to the latest sniffed .m3u8 / .mpd / .mp4 stream on this tab
    if ((!targetUrl || targetUrl.startsWith("blob:")) && tabId >= 0) {
      const streams = TAB_MEDIA_STREAMS.get(tabId) || [];
      if (streams.length > 0) {
        targetUrl = streams[0].url;
      }
    }

    if (targetUrl && !targetUrl.startsWith("blob:")) {
      const cached = HEADER_CACHE.get(targetUrl);
      dispatchDownload({
        url: targetUrl,
        filename: msg.filename || null,
        referrer: (sender.tab && sender.tab.url) || null,
        cookies: null,
        userAgent: navigator.userAgent,
        headers: cached ? cached.headers : {}
      });
      sendResponse({ ok: true, url: targetUrl });
    } else {
      sendResponse({ ok: false, error: "No direct or HLS/DASH stream URL found yet" });
    }
    return true;
  }
});

// 5. Dual-Transport Dispatch: HTTP RPC (127.0.0.1:46890) + Native Messaging (org.oxide.nmh)
async function dispatchDownload(payload) {
  try {
    const res = await fetch(`${HTTP_RPC_URL}/api/downloads`, {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        url: payload.url,
        filename: payload.filename,
        referrer: payload.referrer,
        cookies: payload.cookies,
        user_agent: payload.userAgent,
        headers: payload.headers || {},
        max_segments: 8,
        start_immediately: true
      })
    });
    if (res.ok) {
      return;
    }
  } catch (_) {
    // Fallback to Native Messaging stdio bridge if HTTP RPC is unreachable
  }

  try {
    const port = chrome.runtime.connectNative(NMH_HOST);
    port.postMessage({
      action: "AddDownload",
      payload: {
        url: payload.url,
        filename: payload.filename,
        save_dir: null,
        headers: payload.headers || {},
        max_segments: 8,
        speed_limit_bps: null,
        queue_id: null,
        expected_sha256: null
      }
    });
    port.onMessage.addListener(() => port.disconnect());
  } catch (err) {
    console.error("Failed to dispatch download to Oxide:", err);
  }
}
