import assert from 'node:assert/strict'
import { test } from 'node:test'
import { bySlot, isWork, slotColour, slotOf, surfaceLabel } from '../src/surfaces.ts'

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
  assert.equal(surfaceLabel('new_thing'), 'new_thing')
})
