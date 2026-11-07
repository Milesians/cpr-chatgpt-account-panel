/** Codex 给使用位置起的名字，中文 */
const LABELS: Record<string, string> = {
  cli: 'CLI',
  vscode: 'VS Code',
  desktop_app: '桌面版',
  web: '网页 / 云端',
  exec: 'exec',
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
