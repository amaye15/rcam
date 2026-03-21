import { test, expect } from '@playwright/test';

test('rcam web demo: WASM loads and initializes', async ({ page }) => {
  await page.goto('http://localhost:8080');

  const status = page.locator('#status');
  // Wait for WASM to finish loading (moves away from "Loading WASM module…")
  await expect(status).not.toHaveText('Loading WASM module\u2026', { timeout: 15000 });

  // Start button becomes enabled after WASM init + device enumeration
  await expect(page.locator('#start-btn')).not.toBeDisabled({ timeout: 10000 });

  // Camera select dropdown is present
  await expect(page.locator('#camera-select')).toBeVisible();
});
