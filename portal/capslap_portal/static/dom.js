/** Tiny element builder: h('a', {href}, 'text', child…). Text is never HTML. */
export function h(tag, attrs = {}, ...children) {
  const el = document.createElement(tag)
  for (const [k, v] of Object.entries(attrs)) {
    if (v === undefined || v === null || v === false) continue
    if (k.startsWith('on')) el.addEventListener(k.slice(2), v)
    // value is a property: a textarea's attribute of that name does nothing.
    else if (k === 'value' || (k in el && typeof v !== 'string')) el[k] = v
    else el.setAttribute(k, v)
  }
  el.append(...children.flat().filter((c) => c !== null && c !== undefined && c !== false))
  return el
}
