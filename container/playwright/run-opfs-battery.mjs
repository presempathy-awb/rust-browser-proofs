#!/usr/bin/env node

const DEFAULT_BROWSER = "/usr/bin/chromium";
const DEFAULT_TIMEOUT_SECONDS = 90;

function usage(message) {
  if (message) {
    console.error(message);
  }
  console.error(
    "Usage: run-opfs-battery.mjs --url <test-url> [--executable-path <path>] [--timeout-seconds <seconds>]",
  );
  process.exit(2);
}

function parseArguments(args) {
  const options = {
    executablePath: DEFAULT_BROWSER,
    testUrl: undefined,
    timeoutSeconds: DEFAULT_TIMEOUT_SECONDS,
  };

  for (let index = 0; index < args.length; index += 1) {
    const argument = args[index];
    const value = args[index + 1];
    if (argument === "--executable-path") {
      options.executablePath = value;
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

function wait(milliseconds) {
  return new Promise((resolve) => setTimeout(resolve, milliseconds));
}

async function gotoWhenReady(page, url, deadline) {
  let lastError;
  while (Date.now() < deadline) {
    try {
      await page.goto(url, {
        timeout: Math.max(1_000, Math.min(5_000, deadline - Date.now())),
        waitUntil: "load",
      });
      return;
    } catch (error) {
      lastError = error;
      await wait(250);
    }
  }
  throw new Error(`The wasm-bindgen test server did not become ready: ${lastError}`);
}

async function run() {
  const { chromium } = await import("playwright-core");
  const { executablePath, testUrl, timeoutSeconds } = parseArguments(process.argv.slice(2));
  const deadline = Date.now() + timeoutSeconds * 1_000;
  const browser = await chromium.launch({ executablePath, headless: true });

  try {
    const page = await browser.newPage();
    await gotoWhenReady(page, testUrl, deadline);
    await page.waitForFunction(
      () => document.getElementById("output")?.textContent?.includes("test result:"),
      undefined,
      { timeout: Math.max(1_000, deadline - Date.now()) },
    );
    const output = (await page.locator("#output").textContent()) ?? "";
    console.log(output);
    if (!output.includes("test result: ok.")) {
      throw new Error("Playwright Chromium reported a failed public OPFS battery.");
    }
  } finally {
    await browser.close();
  }
}

run().catch((error) => {
  console.error(error instanceof Error ? error.stack ?? error.message : String(error));
  process.exitCode = 1;
});
