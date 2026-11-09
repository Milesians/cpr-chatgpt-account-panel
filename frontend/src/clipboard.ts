// 插件页面在 sandbox="allow-scripts" 的 iframe 里，宿主没给 clipboard-write 权限，
// navigator.clipboard 会被拒；退到 execCommand('copy')，还不行就由调用方选中文字让人手动复制

/** 复制成功返回 true；必须在点击回调里调用（execCommand 要用户激活） */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (navigator.clipboard?.writeText) {
      await navigator.clipboard.writeText(text)
      return true
    }
  }
  catch {}
  return legacyCopy(text)
}

function legacyCopy(text: string): boolean {
  const area = document.createElement('textarea')
  area.value = text
  area.setAttribute('readonly', '')
  area.style.position = 'fixed'
  area.style.top = '0'
  area.style.left = '0'
  area.style.opacity = '0'
  const active = document.activeElement as HTMLElement | null
  document.body.appendChild(area)
  area.select()
  let ok = false
  try {
    ok = document.execCommand('copy')
  }
  catch {}
  area.remove()
  active?.focus?.()
  return ok
}

/** 复制失败时选中输入框里的文字，方便按 Ctrl+C / ⌘C */
export function selectField(el: HTMLInputElement | null | undefined) {
  if (!el)
    return
  el.focus()
  el.select()
}
