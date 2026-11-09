import assert from 'node:assert/strict'
import { test } from 'node:test'
import { errorFromBody } from '../src/api.ts'

test('reads the plugin error shape', () => {
  const e = errorFromBody(502, { error: { code: 'admin_error', message: 'CPR 管理接口拒绝了请求' } })
  assert.equal(e.message, 'CPR 管理接口拒绝了请求')
  assert.equal(e.code, 'admin_error')
})

test('reads the CPR host envelope instead of a bare HTTP 502', () => {
  const e = errorFromBody(502, { code: 50201, message: '上游服务请求失败' })
  assert.equal(e.message, 'CPR：上游服务请求失败（HTTP 502 · 50201）')
})

test('reads a string error and falls back to a hint with the body', () => {
  assert.equal(errorFromBody(502, { error: 'boom' }).message, 'boom')
  assert.match(errorFromBody(502, {}, '{"x":1}').message, /插件调用超时.*HTTP 502.*\{"x":1\}/)
})
