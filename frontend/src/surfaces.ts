/** Codex 给使用位置起的名字，中文 */
const LABELS: Record<string, string> = {
  cli: 'CLI',
  vscode: 'VS Code',
  desktop_app: '桌面版',
  web: '网页 / 云端',
  exec: 'Codex Exec',
  sdk: 'SDK',
  code_review: '代码审查',
  github_code_review: '代码审查',
  github: 'GitHub',
  jetbrains: 'JetBrains',
  mobile: '移动端',
  slack: 'Slack',
  linear: 'Linear',
  work: 'Work',
  work_web: 'Work 网页',
  work_desktop: 'Work 桌面版',
  work_mobile: 'Work 移动端',
  codex: 'Codex',
  unknown: '未知',
  other: '其他',
}

export function surfaceLabel(key: string): string {
  return LABELS[key] ?? key
}

/** 发起方式（ChatGPT 的 thread source） */
const FEATURES: Record<string, string> = {
  user: '任务',
  subagent: '子代理',
  automation: '自动化',
  guardian_review: '自动审查',
  guardian_classifier: '自动审查分类',
  automated_review: '自动审查',
  image_generation: '图片生成',
  thread_title: '标题生成',
  system: '系统',
  memory_consolidation: '记忆整理',
  agent_identity: '工作区代理',
  unknown: '未知',
  other: '其他',
}

export function featureLabel(key: string): string {
  return FEATURES[key] ?? key
}

/** ChatGPT Work 的使用位置；CPR 以 Codex 的身份转发，这里的用量一定不是 CPR 转发的 */
export function isWork(key: string): boolean {
  return key === 'work' || key.startsWith('work_')
}

/**
 * 颜色跟着位置走，不跟名次（范围变了颜色不变）；Work 的各位置合成一格。
 * 顺序就是堆叠顺序，相邻两格是校验过的配对（浅色、深色都过了 dataviz 校验脚本）；其余是灰色的“其他”
 */
export const SLOTS = ['cli', 'vscode', 'desktop_app', 'web', 'exec', 'work'] as const
export type Slot = typeof SLOTS[number] | 'other'

export function slotOf(key: string): Slot {
  if (isWork(key))
    return 'work'
  return (SLOTS as readonly string[]).includes(key) ? key as Slot : 'other'
}

export function slotColour(slot: Slot): string {
  const i = (SLOTS as readonly string[]).indexOf(slot)
  return i < 0 ? 'var(--series-other)' : `var(--series-${i + 1})`
}

export function slotLabel(slot: Slot): string {
  return slot === 'work' ? 'Work' : surfaceLabel(slot)
}

/** 一天按格汇总 */
export function bySlot(surfaces: Record<string, number>): Map<Slot, number> {
  const out = new Map<Slot, number>()
  for (const [k, v] of Object.entries(surfaces)) {
    if (v > 0) {
      const s = slotOf(k)
      out.set(s, (out.get(s) ?? 0) + v)
    }
  }
  return out
}

/** 饼图最多分几块（多的合成“其他”）；和 index.css 的 --series-1…6 一一对应 */
export const SERIES = 6

/** 常见的使用位置固定用自己的颜色（和详情页的颜色一致），换范围、换账号颜色不变 */
export const SURFACE_ORDER = ['cli', 'vscode', 'desktop_app', 'web', 'exec', 'work'] as const

export interface Slice {
  key: string
  value: number
  colour: string
}

/**
 * 按占比从大到小取前几块，其余合成 `other`。
 * 颜色跟着名字走：`fixed` 里的名字用它自己的位置；不在里面的，按大小依次用这次没被占的位置
 */
export function slices(items: { key: string, value: number }[], fixed: readonly string[] = []): Slice[] {
  const sorted = items.filter(i => i.value > 0).sort((a, b) => b.value - a.value || a.key.localeCompare(b.key))
  const named = sorted.filter(i => i.key !== 'other')
  const head = named.length > SERIES ? named.slice(0, SERIES - 1) : named
  const rest = sorted.filter(i => !head.includes(i)).reduce((s, i) => s + i.value, 0)
  const taken = new Set(head.map(i => fixed.indexOf(i.key)).filter(n => n >= 0 && n < SERIES))
  const free = Array.from({ length: SERIES }, (_, n) => n).filter(n => !taken.has(n))
  const out = head.map((i) => {
    let n = fixed.indexOf(i.key)
    if (n < 0 || n >= SERIES)
      n = free.shift()!
    return { key: i.key, value: i.value, colour: `var(--series-${n + 1})` }
  })
  if (rest > 0)
    out.push({ key: 'other', value: rest, colour: 'var(--series-other)' })
  return out
}
