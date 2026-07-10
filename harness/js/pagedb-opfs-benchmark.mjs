/** Raw dedicated-worker OPFS sync-handle benchmark baseline. */

const WORKER_SOURCE = String.raw`
function errorInfo(error) {
  if (error && typeof error === "object") {
    return {
      name: typeof error.name === "string" ? error.name : "Error",
      message: typeof error.message === "string" ? error.message : String(error),
    };
  }
  return { name: "Error", message: String(error) };
}

function now() {
  return self.performance && typeof self.performance.now === "function"
    ? self.performance.now()
    : Date.now();
}

function uniqueName() {
  const suffix = self.crypto && typeof self.crypto.randomUUID === "function"
    ? self.crypto.randomUUID()
    : Date.now() + "-" + Math.random();
  return ".pagedb-opfs-benchmark-" + suffix;
}

self.onmessage = async (event) => {
  const { byteLength, iterations } = event.data;
  let directory;
  let name;
  let accessHandle;
  let result;
  let failure;

  try {
    const storage = self.navigator && self.navigator.storage;
    if (!storage || typeof storage.getDirectory !== "function") {
      throw new Error("OPFS getDirectory() is unavailable");
    }

    directory = await storage.getDirectory();
    name = uniqueName();
    const file = await directory.getFileHandle(name, { create: true });
    if (typeof file.createSyncAccessHandle !== "function") {
      throw new Error("createSyncAccessHandle() is unavailable in this dedicated worker");
    }
    accessHandle = await file.createSyncAccessHandle();

    const writeBuffer = new Uint8Array(byteLength);
    for (let index = 0; index < byteLength; index += 1) {
      writeBuffer[index] = index % 251;
    }
    const readBuffer = new Uint8Array(byteLength);
    let writtenBytes = 0;
    const writeStarted = now();
    for (let iteration = 0; iteration < iterations; iteration += 1) {
      const written = accessHandle.write(writeBuffer, { at: 0 });
      if (written !== byteLength) {
        throw new Error("sync-handle write was shorter than the requested benchmark payload");
      }
      accessHandle.flush();
      writtenBytes += written;
    }
    const writeElapsedMs = now() - writeStarted;

    let readBytes = 0;
    let checksum = 0;
    const readStarted = now();
    for (let iteration = 0; iteration < iterations; iteration += 1) {
      const read = accessHandle.read(readBuffer, { at: 0 });
      if (read !== byteLength) {
        throw new Error("sync-handle read was shorter than the requested benchmark payload");
      }
      readBytes += read;
      checksum ^= readBuffer[0] ^ readBuffer[byteLength - 1];
    }
    const readElapsedMs = now() - readStarted;

    result = {
      byteLength,
      iterations,
      writes: {
        bytes: writtenBytes,
        elapsedMs: writeElapsedMs,
        perOperationMs: writeElapsedMs / iterations,
      },
      reads: {
        bytes: readBytes,
        checksum,
        elapsedMs: readElapsedMs,
        perOperationMs: readElapsedMs / iterations,
      },
    };
  } catch (error) {
    failure = error;
  }

  if (accessHandle) {
    try {
      accessHandle.close();
    } catch (error) {
      failure ||= error;
    }
  }
  if (directory && name) {
    try {
      await directory.removeEntry(name);
    } catch (error) {
      failure ||= error;
    }
  }

  if (failure) {
    self.postMessage({ ok: false, error: errorInfo(failure) });
  } else {
    self.postMessage({ ok: true, result });
  }
};
`;

function workerError(error) {
  const result = new Error(error && error.message ? error.message : "OPFS benchmark worker failed");
  result.name = error && error.name ? error.name : "Error";
  return result;
}

function assertPositiveSafeInteger(value, name) {
  if (!Number.isSafeInteger(value) || value <= 0) {
    throw new RangeError(`${name} must be a positive safe integer`);
  }
}

function runWorker(payload, timeoutMs) {
  return new Promise((resolve, reject) => {
    let worker;
    let url;
    let timeout;
    let settled = false;
    const settle = (callback, value) => {
      if (settled) return;
      settled = true;
      clearTimeout(timeout);
      if (worker) worker.terminate();
      if (url) URL.revokeObjectURL(url);
      callback(value);
    };

    try {
      url = URL.createObjectURL(new Blob([WORKER_SOURCE], { type: "text/javascript" }));
      worker = new Worker(url);
      worker.onmessage = (event) => {
        if (event.data && event.data.ok) {
          settle(resolve, event.data.result);
        } else {
          settle(reject, workerError(event.data && event.data.error));
        }
      };
      worker.onerror = (event) => settle(reject, workerError({
        name: "WorkerError",
        message: event.message || "OPFS benchmark worker failed",
      }));
      timeout = setTimeout(() => settle(reject, workerError({
        name: "Timeout",
        message: `OPFS benchmark worker exceeded ${timeoutMs}ms`,
      })), timeoutMs);
      worker.postMessage(payload);
    } catch (error) {
      settle(reject, workerError(error));
    }
  });
}

/**
 * Measure raw write+flush and read operations through one OPFS sync handle.
 *
 * This is a baseline only: it does not benchmark PageDB's VFS or database
 * commit path, and it intentionally enforces no performance threshold.
 */
export async function benchmarkRawSyncAccessHandle({ byteLength, iterations, timeoutMs = 5_000 } = {}) {
  assertPositiveSafeInteger(byteLength, "byteLength");
  assertPositiveSafeInteger(iterations, "iterations");
  if (!Number.isFinite(timeoutMs) || timeoutMs <= 0) {
    throw new RangeError("timeoutMs must be a positive finite number");
  }
  if (byteLength * iterations > Number.MAX_SAFE_INTEGER) {
    throw new RangeError("byteLength * iterations must be a safe integer");
  }

  return runWorker({ byteLength, iterations }, timeoutMs);
}
