import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bySlot, isWork, slotColour, slotOf, slices, SURFACE_ORDER, surfaceLabel } from '../src/surfaces.ts'

test('work surfaces share one slot', () => {
  assert.ok(isWork('work'))
  assert.ok(isWork('work_mobile'))
  assert.ok(!isWork('workspace'))
  assert.equal(slotOf('work_web'), 'work')
  assert.equal(slotOf('slack'), 'other')
  assert.equal(slotOf('cli'), 'cli')
})

test('colour follows the surface, not its rank', () => {
  assert.equal(slotColour('cli'), 'var(--series-1)')
  assert.equal(slotColour('work'), 'var(--series-6)')
  assert.equal(slotColour('other'), 'var(--series-other)')
})

test('a day sums by slot', () => {
  const m = bySlot({ work: 1, work_web: 2, cli: 3, slack: 1, jetbrains: 0 })
  assert.equal(m.get('work'), 3)
  assert.equal(m.get('cli'), 3)
  assert.equal(m.get('other'), 1)
  assert.equal(m.size, 3)
})

test('labels', () => {
  assert.equal(surfaceLabel('desktop_app'), '桌面版')
  assert.equal(surfaceLabel('exec'), 'Codex Exec')
  assert.equal(surfaceLabel('new_thing'), 'new_thing')
})

test('known names keep their own colour whatever their rank', () => {
  const a = slices([{ key: 'cli', value: 1 }, { key: 'vscode', value: 9 }], SURFACE_ORDER)
  assert.deepEqual(a.map(s => [s.key, s.colour]), [['vscode', 'var(--series-2)'], ['cli', 'var(--series-1)']])
})

test('unknown names take the free colours, largest first', () => {
  const a = slices([{ key: 'cli', value: 5 }, { key: 'work_web', value: 3 }, { key: 'slack', value: 1 }], SURFACE_ORDER)
  assert.deepEqual(a.map(s => s.colour), ['var(--series-1)', 'var(--series-2)', 'var(--series-3)'])
})

test('more than six parts fold the tail into other', () => {
  const items = Array.from({ length: 8 }, (_, i) => ({ key: `m${i}`, value: 8 - i }))
  const a = slices(items)
  assert.equal(a.length, 6)
  assert.equal(a[5]!.key, 'other')
  assert.equal(a[5]!.value, 3 + 2 + 1)
  assert.equal(a[5]!.colour, 'var(--series-other)')
  assert.equal(new Set(a.map(s => s.colour)).size, 6)
})

test('zero values are dropped and other merges with the tail', () => {
  const a = slices([{ key: 'cli', value: 2 }, { key: 'other', value: 1 }, { key: 'x', value: 0 }], SURFACE_ORDER)
  assert.deepEqual(a.map(s => [s.key, s.value]), [['cli', 2], ['other', 1]])
})
