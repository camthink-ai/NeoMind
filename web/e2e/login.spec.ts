import { test, expect } from '@playwright/test'

test.describe('登录页面 UI 测试', () => {
  test.beforeEach(async ({ page }) => {
    // 每个测试前清除本地存储
    await page.context().clearCookies()
    await page.goto('/')
  })

  test('应该正确显示登录页面', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    // 检查页面标题
    const title = await page.title()
    console.log('页面标题:', title)
    expect(title).toContain('NeoTalk')

    // 检查 NeoTalk 标题
    await expect(page.getByRole('heading', { name: 'NeoTalk' })).toBeVisible()

    // 检查登录表单
    await expect(page.locator('form')).toBeVisible()

    // 检查用户名输入框
    await expect(page.locator('input#username')).toBeVisible()

    // 检查密码输入框
    await expect(page.locator('input#password')).toBeVisible()

    // 检查提交按钮
    await expect(page.locator('button[type="submit"]')).toBeVisible()

    // 截图
    await page.screenshot({ path: 'screenshots/login-page.png', fullPage: true })
  })

  test('输入框应该有正确的占位符', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    const usernamePlaceholder = await page.locator('input#username').getAttribute('placeholder')
    const passwordPlaceholder = await page.locator('input#password').getAttribute('placeholder')

    console.log('用户名占位符:', usernamePlaceholder)
    console.log('密码占位符:', passwordPlaceholder)

    // 占位符不应为空
    expect(usernamePlaceholder).toBeTruthy()
    expect(passwordPlaceholder).toBeTruthy()
  })

  test('按钮在表单为空时应被禁用', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    const submitButton = page.locator('button[type="submit"]')

    // 空表单时按钮应该被禁用
    await expect(submitButton).toBeDisabled()
    console.log('提交按钮已禁用')
  })

  test('填写表单后按钮应启用', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    // 填写用户名和密码
    await page.locator('input#username').fill('test')
    await page.locator('input#password').fill('password123')

    const submitButton = page.locator('button[type="submit"]')
    await expect(submitButton).toBeEnabled()
  })

  test('记住我复选框应该可以点击', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    const checkbox = page.locator('input#remember')
    const isChecked = await checkbox.isChecked()

    console.log('记住我初始状态:', isChecked)

    // 点击复选框
    await checkbox.check()
    await expect(checkbox).toBeChecked()

    // 取消选中
    await checkbox.uncheck()
    await expect(checkbox).not.toBeChecked()
  })

  test('密码输入框应该是password类型', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    const passwordInput = page.locator('input#password')
    const inputType = await passwordInput.getAttribute('type')

    expect(inputType).toBe('password')
  })

  test('输入框应该有正确的autocomplete属性', async ({ page }) => {
    await page.waitForLoadState('networkidle')

    const usernameAutocomplete = await page.locator('input#username').getAttribute('autocomplete')
    const passwordAutocomplete = await page.locator('input#password').getAttribute('autocomplete')

    expect(usernameAutocomplete).toBe('username')
    expect(passwordAutocomplete).toBe('current-password')
  })
})

test.describe('登录页面响应式测试', () => {
  test('在移动设备上应该正确显示', async ({ page }) => {
    await page.setViewportSize({ width: 375, height: 667 })
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // 检查主要内容可见
    await expect(page.getByRole('heading', { name: 'NeoTalk' })).toBeVisible()
    await expect(page.locator('input#username')).toBeVisible()
    await expect(page.locator('input#password')).toBeVisible()

    // 截图
    await page.screenshot({ path: 'screenshots/login-mobile.png', fullPage: true })
  })

  test('在平板设备上应该正确显示', async ({ page }) => {
    await page.setViewportSize({ width: 768, height: 1024 })
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // 检查主要内容可见
    await expect(page.getByRole('heading', { name: 'NeoTalk' })).toBeVisible()
    await expect(page.locator('input#username')).toBeVisible()
    await expect(page.locator('input#password')).toBeVisible()

    // 截图
    await page.screenshot({ path: 'screenshots/login-tablet.png', fullPage: true })
  })

  test('在桌面设备上应该正确显示', async ({ page }) => {
    await page.setViewportSize({ width: 1920, height: 1080 })
    await page.goto('/')
    await page.waitForLoadState('networkidle')

    // 检查主要内容可见
    await expect(page.getByRole('heading', { name: 'NeoTalk' })).toBeVisible()
    await expect(page.locator('input#username')).toBeVisible()
    await expect(page.locator('input#password')).toBeVisible()

    // 截图
    await page.screenshot({ path: 'screenshots/login-desktop.png', fullPage: true })
  })
})
