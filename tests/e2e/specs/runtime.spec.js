describe("TauTerm runtime smoke", () => {
  before(async () => {
    const root = await $('[data-testid="app-root"]');
    await root.waitForDisplayed({ timeout: 20_000 });
    await browser.execute(() => {
      window.__tautermE2eErrors = [];
      window.addEventListener("error", event => {
        window.__tautermE2eErrors.push(String(event.error || event.message));
      });
      window.addEventListener("unhandledrejection", event => {
        window.__tautermE2eErrors.push(String(event.reason));
      });
    });
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

  after(async () => {
    const errors = await browser.execute(() => window.__tautermE2eErrors || []);
    expect(errors).toEqual([]);
  });
});
