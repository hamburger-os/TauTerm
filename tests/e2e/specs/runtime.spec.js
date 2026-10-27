import { tcpFixture, udpFixture, availableTcpPort, tcpClient } from "../helpers/loopback.mjs";

const testId = id => $(`[data-testid="${id}"]`);
async function fill(element, value) {
  await element.setValue(value);
  await browser.releaseActions();
  await expect(element).toHaveValue(value);
}
async function withCleanup(work, cleanup) {
  let primaryFailure;
  try { await work(); }
  catch (error) { primaryFailure = error; throw error; }
  finally {
    try { await cleanup(); }
    catch (error) {
      if (!primaryFailure) throw error;
      console.error("Fixture cleanup also failed:", error);
    }
  }
}
async function visibleTestId(id) {
  let visible;
  await browser.waitUntil(async () => {
    for (const element of await $$(`[data-testid="${id}"]`)) {
      if (await element.isDisplayed()) { visible = element; return true; }
    }
    return false;
  }, { timeout: 10_000, timeoutMsg: `Visible control missing: ${id}` });
  return visible;
}
async function findSessionEntry(name) {
  const entries = await $$('[data-testid="session-entry"]');
  for (const entry of entries) if (await entry.getAttribute("data-session-name") === name) return entry;
  return null;
}
async function sessionEntry(name) {
  let entry;
  await browser.waitUntil(async () => Boolean(entry = await findSessionEntry(name)), {
    timeout: 10_000, timeoutMsg: `Session entry missing: ${name}`,
  });
  return entry;
}
async function sessionAction(name, action) {
  await browser.releaseActions();
  const entry = await sessionEntry(name);
  await entry.click();
  await entry.click({ button: "right" });
  await $('[role="menu"]').waitForDisplayed();
  const item = testId(`context-action-${action}`);
  await item.waitForDisplayed();
  await item.click();
  await $('[role="menu"]').waitForExist({ reverse: true });
}
async function createNetwork(name, transport, port, role = "client") {
  await testId("new-session-button").click();
  await testId("connection-type-network").click();
  await fill(testId("session-name"), name);
  await testId("network-transport").selectByAttribute("value", transport);
  await testId("network-role").selectByAttribute("value", role);
  await fill(testId(`network-${role === "client" ? "remote" : "local"}-host`), "127.0.0.1");
  await fill(testId(`network-${role === "client" ? "remote" : "local"}-port`), String(port));
  await testId("save-session").click();
  await testId("connect-dialog-overlay").waitForExist({ reverse: true });
  await sessionAction(name, "connect");
  try { await (await visibleTestId("send-input")).waitForEnabled(); }
  catch (error) {
    console.error("Connection diagnostics:", await runtimeSnapshot(name), await browser.execute(() => ({
      inputs: [...document.querySelectorAll('[data-testid="send-input"]')].map(input => ({ disabled: input.disabled, visible: input.getBoundingClientRect().height > 0 })),
      text: document.body.innerText.slice(-4000),
    })));
    throw error;
  }
}
async function send(text) {
  await fill(await visibleTestId("send-input"), text);
  await (await visibleTestId("send-button")).click();
}
async function received(text) {
  await browser.waitUntil(async () => (await (await visibleTestId("network-workspace")).getText()).includes(text), {
    timeout: 10_000, timeoutMsg: `RX presentation missing: ${text}`,
  });
}
async function runtimeSnapshot(name) {
  return browser.executeAsync(async (sessionName, done) => {
    try {
      const tabs = await window.__TAURI_INTERNALS__.invoke("get_tabs");
      const tab = tabs.find(item => item.name === sessionName);
      done(tab ? { id: tab.id, runtimeId: tab.runtimeId, state: tab.state } : null);
    } catch (error) { done({ error: String(error) }); }
  }, name);
}
async function removeSession(name) {
  const entry = await findSessionEntry(name);
  if (!entry) return;
  const runtime = await runtimeSnapshot(name);
  if (runtime?.state === "connected") {
    await sessionAction(name, "disconnect");
    await (await visibleTestId("send-input")).waitForEnabled({ reverse: true });
  }
  await sessionAction(name, "delete");
  const confirm = $('[role="alertdialog"] [data-action="confirm"]');
  await confirm.waitForDisplayed();
  await confirm.click();
  await browser.waitUntil(async () => !(await findSessionEntry(name)), { timeout: 10_000 });
}

describe("TauTerm runtime smoke", () => {
  before(async () => {
    await browser.waitUntil(async () => browser.execute(() => Boolean(window.__TAURI_INTERNALS__)), {
      timeout: 20_000, timeoutMsg: "Tauri IPC bridge did not initialize",
    });
    const identifier = await browser.executeAsync(async done => {
      try { done(await window.__TAURI_INTERNALS__.invoke("plugin:app|identifier")); }
      catch (error) { done(String(error)); }
    });
    expect(identifier).toBe("com.tauterm.e2e");
    await browser.waitUntil(
      async () => browser.execute(() => document.readyState === "complete"),
      {
        timeout: 20_000,
        timeoutMsg: "TauTerm document did not finish loading",
      },
    );

    await browser.execute(() => {
      window.__tautermE2eErrors = [];
      window.addEventListener("error", event => {
        window.__tautermE2eErrors.push(String(event.error || event.message));
      });
      window.addEventListener("unhandledrejection", event => {
        window.__tautermE2eErrors.push(String(event.reason));
      });
    });

    try {
      await browser.waitUntil(
        async () => browser.execute(
          () => document.querySelector('[data-testid="app-root"]') !== null,
        ),
        {
          timeout: 20_000,
          timeoutMsg: "TauTerm app root was not rendered",
        },
      );
    } catch (error) {
      const diagnostics = await browser.execute(() => ({
        title: document.title,
        url: location.href,
        readyState: document.readyState,
        bodyText: document.body?.innerText?.slice(0, 2000) ?? "",
        bodyHtml: document.body?.innerHTML?.slice(0, 4000) ?? "",
      }));
      console.error("TauTerm startup diagnostics:", JSON.stringify(diagnostics, null, 2));
      throw error;
    }
    const previousFixtures = [];
    for (const entry of await $$('[data-testid="session-entry"]')) {
      const name = await entry.getAttribute("data-session-name");
      if (/^e2e (tcp|udp|server) \d+$/i.test(name)) previousFixtures.push(name);
    }
    for (const name of previousFixtures) await removeSession(name);
  });

  it("renders the primary application shell without document overflow", async () => {
    await expect($('[data-testid="toolbar"]')).toBeDisplayed();
    const geometry = await browser.execute(() => ({
      bodyWidth: document.body.scrollWidth,
      viewportWidth: document.documentElement.clientWidth,
      bodyHeight: document.body.scrollHeight,
      viewportHeight: document.documentElement.clientHeight,
    }));
    expect(geometry.bodyWidth).toBeLessThanOrEqual(geometry.viewportWidth + 1);
    expect(geometry.bodyHeight).toBeLessThanOrEqual(geometry.viewportHeight + 1);
  });

  it("opens and closes the command palette", async () => {
    await $('[data-testid="command-palette-trigger"]').click();
    const palette = await $('[data-testid="command-palette-overlay"]');
    await palette.waitForDisplayed();
    await expect($('[data-testid="command-palette-input"]')).toBeDisplayed();
    await browser.keys(["Escape"]);
    await palette.waitForDisplayed({ reverse: true });
  });

  it("opens and closes the new-session workflow", async () => {
    await $('[data-testid="new-session-button"]').click();
    const dialog = await $('[data-testid="connect-dialog-overlay"]');
    await dialog.waitForDisplayed();
    await browser.keys(["Escape"]);
    await dialog.waitForDisplayed({ reverse: true });
  });

  it("renders Modbus configuration with RTU client defaults", async () => {
    await $('[data-testid="new-session-button"]').click();
    const dialog = await $('[data-testid="connect-dialog-overlay"]');
    await dialog.waitForDisplayed();

    await testId("connection-type-modbus").click();

    const form = await $('[data-testid="tauterm-modbus-connect-form"]');
    await form.waitForDisplayed();
    await expect($('[data-testid="tauterm-modbus-mode"]')).toHaveValue("rtu");
    await expect($('[data-testid="tauterm-modbus-role"]')).toHaveValue("client");
    await expect($('[data-testid="tauterm-modbus-serial-port"]')).toBeDisplayed();

    await browser.keys(["Escape"]);
    await dialog.waitForDisplayed({ reverse: true });
  });

  it("receives early TCP bytes, confirms TX, handles remote EOF and reconnects the same saved Session", async () => {
    const peer = await tcpFixture();
    const name = `e2e tcp ${Date.now()}`;
    await withCleanup(async () => {
      await createNetwork(name, "tcp", peer.port);
      await received("GREETING-1");
      const first = await runtimeSnapshot(name);
      expect(first?.runtimeId).toMatch(/^[1-9]\d*$/);
      await send("tcp-first");
      await browser.waitUntil(() => peer.clients[0].received.includes("tcp-first"));
      await received("ECHO:tcp-first");
      peer.clients[0].socket.end();
      await (await visibleTestId("send-input")).waitForEnabled({ reverse: true });
      await sessionAction(name, "connect");
      await (await visibleTestId("send-input")).waitForEnabled();
      await received("GREETING-2");
      const second = await runtimeSnapshot(name);
      expect(second.id).toBe(first.id);
      expect(BigInt(second.runtimeId) > BigInt(first.runtimeId)).toBe(true);
      const staleWrite = await browser.executeAsync(async (id, runtimeId, done) => {
        try {
          await window.__TAURI_INTERNALS__.invoke("write_data", {
            sessionId: id, runtimeId, data: Array.from(new TextEncoder().encode("STALE-WRITE")), transcode: false,
          });
          done("accepted");
        } catch { done("rejected"); }
      }, first.id, first.runtimeId);
      expect(staleWrite).toBe("rejected");
      expect(peer.clients[1].received).not.toContain("STALE-WRITE");
      await send("tcp-second");
      await browser.waitUntil(() => peer.clients[1]?.received.includes("tcp-second"));
      await received("ECHO:tcp-second");
      expect(peer.clients.length).toBe(2);
    }, async () => {
      try { await removeSession(name); } finally { await peer.close(); }
    });
  });

  it("exchanges UDP datagrams and remains usable after disconnect/reconnect", async () => {
    const peer = await udpFixture();
    const name = `e2e udp ${Date.now()}`;
    await withCleanup(async () => {
      await createNetwork(name, "udp", peer.port);
      await send("udp-first");
      await browser.waitUntil(() => peer.received.some(data => data.includes("udp-first")));
      await received("ECHO:udp-first");
      await sessionAction(name, "disconnect");
      await (await visibleTestId("send-input")).waitForEnabled({ reverse: true });
      await sessionAction(name, "connect");
      await (await visibleTestId("send-input")).waitForEnabled();
      await send("udp-second");
      await browser.waitUntil(() => peer.received.some(data => data.includes("udp-second")));
      await received("ECHO:udp-second");
    }, async () => {
      try { await removeSession(name); } finally { await peer.close(); }
    });
  });

  it("publishes TCP server peers before early data and closes active children with their parent", async () => {
    const port = await availableTcpPort();
    const name = `e2e server ${Date.now()}`;
    let peer;
    await withCleanup(async () => {
      await createNetwork(name, "tcp", port, "server");
      peer = await tcpClient(port);
      await received("EARLY-PEER-DATA");
      await send("server-reply");
      await browser.waitUntil(() => peer.received.includes("server-reply"));
      await sessionAction(name, "disconnect");
      await browser.waitUntil(() => peer.socket.destroyed);
      await (await visibleTestId("send-input")).waitForEnabled({ reverse: true });
    }, async () => {
      peer?.socket.destroy();
      await removeSession(name);
    });
  });

  after(async () => {
    const errors = await browser.execute(() => window.__tautermE2eErrors || []);
    expect(errors).toEqual([]);
  });
});
