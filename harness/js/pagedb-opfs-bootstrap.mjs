/** Browser capability preflight for a dedicated-worker PageDB OPFS runtime. */

const WORKER_PROBE_SOURCE = String.raw`
function errorInfo(error) {
  if (error && typeof error === "object") {
    return {
      name: typeof error.name === "string" ? error.name : "Error",
      message: typeof error.message === "string" ? error.message : String(error),
    };
  }
  return { name: "Error", message: String(error) };
}

self.onmessage = async () => {
  const storage = self.navigator && self.navigator.storage;
  if (!storage || typeof storage.getDirectory !== "function") {
    self.postMessage({ available: false, error: { name: "Unsupported", message: "OPFS getDirectory() is unavailable" } });
    return;
  }

  let directory;
  let name;
  let accessHandle;
  try {
    directory = await storage.getDirectory();
    name = ".pagedb-opfs-capability-" + (self.crypto && self.crypto.randomUUID ? self.crypto.randomUUID() : Date.now() + "-" + Math.random());
    const file = await directory.getFileHandle(name, { create: true });
    if (typeof file.createSyncAccessHandle !== "function") {
      throw new Error("createSyncAccessHandle() is unavailable in this dedicated worker");
    }
    accessHandle = await file.createSyncAccessHandle();
    accessHandle.close();
    accessHandle = undefined;
    await directory.removeEntry(name);
    self.postMessage({ available: true, error: null });
  } catch (error) {
    if (accessHandle) {
      try { accessHandle.close(); } catch {}
    }
    if (directory && name) {
      try { await directory.removeEntry(name); } catch {}
    }
    self.postMessage({ available: false, error: errorInfo(error) });
  }
};
`;

function errorInfo(error) {
  if (error && typeof error === "object") {
    return {
      name: typeof error.name === "string" ? error.name : "Error",
      message: typeof error.message === "string" ? error.message : String(error),
    };
  }
  return { name: "Error", message: String(error) };
}

function probeSyncAccessHandle(timeoutMs) {
  return new Promise((resolve) => {
    let worker;
    let url;
    let timeout;
    let settled = false;
    const settle = (result) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (worker) worker.terminate();
      if (url) URL.revokeObjectURL(url);
      resolve(result);
    };

    try {
      url = URL.createObjectURL(new Blob([WORKER_PROBE_SOURCE], { type: "text/javascript" }));
      worker = new Worker(url);
      worker.onmessage = (event) => settle(event.data);
      worker.onerror = (event) => settle({
        available: false,
        error: { name: "WorkerError", message: event.message || "OPFS capability worker failed" },
      });
      timeout = setTimeout(() => settle({
        available: false,
        error: { name: "Timeout", message: `OPFS capability worker exceeded ${timeoutMs}ms` },
      }), timeoutMs);
      worker.postMessage(null);
    } catch (error) {
      settle({ available: false, error: errorInfo(error) });
    }
  });
}

/**
 * Report whether this origin can construct PageDB's dedicated-worker OPFS VFS.
 *
 * `requestPersistence` defaults to false because it may request a browser
 * permission; callers must opt in to that side effect explicitly.
 */
export async function probeOpfsCapabilities({ requestPersistence = false, timeoutMs = 5_000 } = {}) {
  if (typeof requestPersistence !== "boolean") {
    throw new TypeError("requestPersistence must be a boolean");
  }
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new RangeError("timeoutMs must be a positive finite number");
  }

  const storage = globalThis.navigator && globalThis.navigator.storage;
  if (!storage) {
    return {
      opfs: { available: false, error: { name: "Unsupported", message: "StorageManager is unavailable" } },
      syncAccessHandle: { available: false, error: { name: "Unsupported", message: "StorageManager is unavailable" } },
      storage: { usage: null, quota: null, persisted: null, persistenceRequested: false, persistenceGranted: null, error: null },
      crossOriginIsolated: globalThis.crossOriginIsolated === true,
    };
  }

  let estimate = { usage: null, quota: null };
  let persisted = null;
  let persistenceGranted = null;
  let storageError = null;
  try {
    if (typeof storage.estimate === "function") {
      const result = await storage.estimate();
      estimate = {
        usage: typeof result.usage === "number" ? result.usage : null,
        quota: typeof result.quota === "number" ? result.quota : null,
      };
    }
    if (typeof storage.persisted === "function") {
      persisted = await storage.persisted();
    }
    if (requestPersistence && typeof storage.persist === "function") {
      persistenceGranted = await storage.persist();
      persisted = persistenceGranted;
    }
  } catch (error) {
    storageError = errorInfo(error);
  }

  const syncAccessHandle = await probeSyncAccessHandle(timeoutMs);
  return {
    opfs: {
      available: typeof storage.getDirectory === "function",
      error: typeof storage.getDirectory === "function" ? null : { name: "Unsupported", message: "OPFS getDirectory() is unavailable" },
    },
    syncAccessHandle,
    storage: {
      ...estimate,
      persisted,
      persistenceRequested: requestPersistence,
      persistenceGranted,
      error: storageError,
    },
    crossOriginIsolated: globalThis.crossOriginIsolated === true,
  };
}
