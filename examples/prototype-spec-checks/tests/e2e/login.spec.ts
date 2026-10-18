import { expect, test } from '@playwright/test'

// Stale: written before "And it succeeds again after 15 minutes" was added to the rule.
// @spec login/lockout #3e9a1b42
test('5 failed logins lock the account', async ({ page }) => {
  await page.goto('/')
  const password = page.getByLabel('password')
  const result = page.getByLabel('result')
  for (let i = 0; i < 5; i++) {
    await password.fill('wrong')
    await password.press('Enter')
    await expect(result).toHaveText('wrong-password')
  }
  await password.fill('hunter2')
  await password.press('Enter')
  await expect(result).toHaveText('locked')
})
