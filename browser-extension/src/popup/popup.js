document.getElementById("open-app-btn").addEventListener("click", () => {
  try {
    const port = chrome.runtime.connectNative("org.oxide.nmh");
    port.postMessage({ action: "GetStatus" });
    port.disconnect();
  } catch (e) {
    console.error("Could not trigger Oxide:", e);
  }
});
