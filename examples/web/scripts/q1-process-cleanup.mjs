export function waitForChildExit(child, timeoutMs, label) {
  if (!child || child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
  return new Promise((resolvePromise, reject) => {
    const timer = setTimeout(() => {
      cleanup();
      reject(new Error(`${label} did not exit within ${timeoutMs} ms`));
    }, timeoutMs);
    const cleanup = () => {
      clearTimeout(timer);
      child.off("exit", onExit);
      child.off("error", onError);
    };
    const onExit = () => {
      cleanup();
      resolvePromise();
    };
    const onError = (error) => {
      cleanup();
      reject(error);
    };
    child.once("exit", onExit);
    child.once("error", onError);
  });
}

function withTimeout(promise, timeoutMs, label) {
  let timer;
  return Promise.race([
    promise,
    new Promise((_, reject) => {
      timer = setTimeout(() => reject(new Error(`${label} timed out`)), timeoutMs);
    }),
  ]).finally(() => clearTimeout(timer));
}

export async function cleanupBrowserAndServer(browser, server) {
  const failures = [];
  if (browser) {
    const browserProcess = browser.process?.();
    try {
      await withTimeout(browser.close(), 2_000, "browser.close");
      await waitForChildExit(browserProcess, 2_000, "browser process");
    } catch (error) {
      failures.push(error);
      if (browserProcess?.exitCode === null && browserProcess.signalCode === null) {
        browserProcess.kill("SIGKILL");
        try {
          await waitForChildExit(browserProcess, 2_000, "force-killed browser process");
        } catch (killError) {
          failures.push(killError);
        }
      }
    }
  }
  if (server) {
    try {
      if (server.exitCode === null && server.signalCode === null && !server.kill("SIGTERM")) {
        throw new Error("HTTP server rejected SIGTERM");
      }
      await waitForChildExit(server, 2_000, "HTTP server");
    } catch (error) {
      failures.push(error);
      if (server.exitCode === null && server.signalCode === null) {
        server.kill("SIGKILL");
        try {
          await waitForChildExit(server, 2_000, "force-killed HTTP server");
        } catch (killError) {
          failures.push(killError);
        }
      }
    }
  }
  if (failures.length > 0) {
    throw new AggregateError(failures, "Q1 browser/server cleanup was not stable");
  }
}
