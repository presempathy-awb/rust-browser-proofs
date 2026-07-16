#!/usr/bin/env node

const DEFAULT_CDP_URL = "http://127.0.0.1:9222";
const DEFAULT_TIMEOUT_SECONDS = 90;

function usage(message) {
  if (message) {
    console.error(message);
  }
  console.error(
    "Usage: cdp-browser-test.mjs [--cdp-url <url>] --url <test-url> [--timeout-seconds <seconds>]",
  );
  process.exit(2);
}

function parseArguments(args) {
  const options = {
    cdpUrl: DEFAULT_CDP_URL,
    testUrl: undefined,
    timeoutSeconds: DEFAULT_TIMEOUT_SECONDS,
  };

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    const value = args[index + 1];
    if (argument === "--cdp-url") {
      options.cdpUrl = value;
      index += 1;
    } else if (argument === "--url") {
      options.testUrl = value;
      index += 1;
    } else if (argument === "--timeout-seconds") {
      options.timeoutSeconds = Number(value);
      index += 1;
    } else {
      usage(`Unknown argument: ${argument}`);
    }
  }

  if (!options.testUrl) {
    usage("--url is required.");
  }
  if (!Number.isInteger(options.timeoutSeconds) || options.timeoutSeconds < 1) {
    usage("--timeout-seconds must be a positive integer.");
  }

  return options;
}

async function fetchJson(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`HTTP ${response.status} from ${url}`);
  }
  return response.json();
}

function wait(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

class CdpClient {
  constructor(webSocketUrl) {
    this.webSocketUrl = webSocketUrl;
    this.socket = undefined;
    this.nextId = 1;
    this.pending = new Map();
    this.eventHandlers = new Map();
  }

  async connect() {
    this.socket = new WebSocket(this.webSocketUrl);
    await new Promise((resolve, reject) => {
      const timeout = setTimeout(
        () => reject(new Error(`Timed out connecting to ${this.webSocketUrl}`)),
        5_000,
      );
      this.socket.addEventListener("open", () => {
        clearTimeout(timeout);
        resolve();
      });
      this.socket.addEventListener("error", () => {
        clearTimeout(timeout);
        reject(new Error(`Could not connect to ${this.webSocketUrl}`));
      });
    });
    this.socket.addEventListener("message", ({ data }) => this.handleMessage(data));
  }

  handleMessage(data) {
    const message = JSON.parse(data);
    if (message.id) {
      const pending = this.pending.get(message.id);
      if (!pending) {
        return;
      }
      this.pending.delete(message.id);
      if (message.error) {
        pending.reject(new Error(JSON.stringify(message.error)));
      } else {
        pending.resolve(message.result);
      }
      return;
    }

    for (const handler of this.eventHandlers.get(message.method) ?? []) {
      void handler(message.params);
    }
  }

  call(method, params = {}) {
    const id = this.nextId++;
    this.socket.send(JSON.stringify({ id, method, params }));
    return new Promise((resolve, reject) => this.pending.set(id, { resolve, reject }));
  }

  on(method, handler) {
    const handlers = this.eventHandlers.get(method) ?? [];
    handlers.push(handler);
    this.eventHandlers.set(method, handlers);
  }

  close() {
    this.socket?.close();
  }
}

function patchSharedWorkerWrapper(source) {
  const start = "const __wbg_OriginalSharedWorker = SharedWorker;";
  const end = "SharedWorker.prototype = __wbg_OriginalSharedWorker.prototype;";
  if (!source.includes(start) || !source.includes(end)) {
    throw new Error("wasm-bindgen-test-runner SharedWorker wrapper markers changed");
  }

  return source
    .replace(start, `if (typeof SharedWorker === 'function') {\n${start}`)
    .replace(end, `${end}\n}`);
}

async function findTarget(cdpUrl, targetId) {
  for (let attempt = 0; attempt < 50; attempt += 1) {
    const targets = await fetchJson(`${cdpUrl}/json`);
    const target = targets.find((entry) => entry.id === targetId);
    if (target?.webSocketDebuggerUrl) {
      return target;
    }
    await wait(100);
  }
  throw new Error(`Chrome did not expose DevTools target ${targetId}`);
}

async function waitForOutput(client, timeoutSeconds) {
  const deadline = Date.now() + timeoutSeconds * 1_000;
  let output = "";
  while (Date.now() < deadline) {
    await wait(250);
    const evaluation = await client.call("Runtime.evaluate", {
      expression: 'document.getElementById("output")?.textContent ?? ""',
      returnByValue: true,
    });
    output = evaluation.result.value;
    if (output.includes("test result:")) {
      return output;
    }
  }
  throw new Error(`Timed out waiting for browser test output after ${timeoutSeconds}s.\n${output}`);
}

async function run() {
  const { cdpUrl, testUrl, timeoutSeconds } = parseArguments(process.argv.slice(2));
  const version = await fetchJson(`${cdpUrl}/json/version`);
  const browser = new CdpClient(version.webSocketDebuggerUrl);
  await browser.connect();

  let page;
  let targetId;
  try {
    ({ targetId } = await browser.call("Target.createTarget", { url: "about:blank" }));
    const target = await findTarget(cdpUrl, targetId);
    page = new CdpClient(target.webSocketDebuggerUrl);
    await page.connect();

    let patchedRunner = false;
    page.on("Fetch.requestPaused", async ({ requestId, responseStatusCode, responseHeaders }) => {
      try {
        const response = await page.call("Fetch.getResponseBody", { requestId });
        const source = Buffer.from(
          response.body,
          response.base64Encoded ? "base64" : "utf8",
        ).toString("utf8");
        const patched = patchSharedWorkerWrapper(source);
        patchedRunner = true;
        await page.call("Fetch.fulfillRequest", {
          requestId,
          responseCode: responseStatusCode,
          responseHeaders,
          body: Buffer.from(patched).toString("base64"),
        });
      } catch (error) {
        await page.call("Fetch.failRequest", { requestId, errorReason: "Failed" });
        console.error(error instanceof Error ? error.message : String(error));
      }
    });

    await page.call("Page.enable");
    await page.call("Runtime.enable");
    await page.call("Fetch.enable", {
      patterns: [{ urlPattern: "*run.js", requestStage: "Response" }],
    });
    await page.call("Page.navigate", { url: testUrl });

    const output = await waitForOutput(page, timeoutSeconds);
    console.log(output);
    if (!patchedRunner) {
      throw new Error("The test page did not load the expected wasm-bindgen runner script.");
    }
    if (!output.includes("test result: ok.")) {
      throw new Error("CDP browser test reported a failure.");
    }
  } finally {
    page?.close();
    if (targetId) {
      await browser.call("Target.closeTarget", { targetId }).catch(() => undefined);
    }
    browser.close();
  }
}

run().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});
