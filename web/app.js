/* PDFloom Studio: a buildless visual editor for pdfloom-doc documents.
 *
 * The editor state IS the pdfloom-doc config (the same JSON you download),
 * so importing a config restores the editor exactly, and "Download PDF"
 * and "pdfloom build" always agree. Coordinates are stored in PDF points
 * with a bottom-left origin; the DOM renders top-left, so the converter
 * functions below are the only places that translate.
 */

"use strict";

// ---------------------------------------------------------------- state

const PAGE_SIZES = {
  "Letter portrait": [612, 792],
  "Letter landscape": [792, 612],
  "A4 portrait": [595.28, 841.89],
  "A4 landscape": [841.89, 595.28],
};

const FONT_FAMILIES = [
  "Liberation Sans", "Liberation Serif", "Liberation Mono",
  "Noto Sans", "Noto Serif", "Noto Sans Mono",
  "Noto Sans Myanmar", "Noto Sans Thai", "Noto Sans Arabic",
  "Noto Sans CJK JP", "Atkinson Hyperlegible", "OpenDyslexic",
  "Helvetica", "Times-Roman", "Courier", "Arial",
];
const FONT_STYLES = ["Regular", "Bold", "Italic", "Bold Italic"];
// Which styles actually exist per family in the bundled catalog (the
// same faces the PDF embeds). Families absent from this map ship only
// Regular + Bold. The Style dropdown offers exactly these, so every
// Family/Style combo the inspector offers can be honored by the PDF;
// the canvas, the config, and the embedded font face always agree.
// Standard-14 aliases resolve to the Liberation look-alikes, which
// have all four faces.
const CATALOG_STYLES = {
  "Liberation Sans": FONT_STYLES,
  "Liberation Serif": FONT_STYLES,
  "Liberation Mono": FONT_STYLES,
  "Noto Sans": FONT_STYLES,
  "Helvetica": FONT_STYLES,
  "Times-Roman": FONT_STYLES,
  "Courier": FONT_STYLES,
  "Arial": FONT_STYLES,
};

function stylesFor(family) {
  return CATALOG_STYLES[family] || ["Regular", "Bold"];
}

// When the family changes, keep the style valid for the new family:
// Bold Italic -> Bold (keep the weight), anything else -> Regular.
function clampStyle(family, style) {
  const offered = stylesFor(family);
  if (offered.includes(style)) return style;
  if (/Bold/.test(style) && offered.includes("Bold")) return "Bold";
  return "Regular";
}

let doc = null;          // the pdfloom-doc object
let current = 0;         // current page index
let sel = null;          // selected element (live reference into doc)
let zoom = 1.0;
let showOrder = false;
let showPreview = false;
let undoStack = [];
let redoStack = [];

function newDoc() {
  return {
    format: "pdfloom-doc", version: 1,
    profile: "ua1", title: "Untitled document", lang: "en-US",
    pages: [{ size: [612, 792], elements: [] }],
  };
}

function snapshot() {
  undoStack.push(JSON.stringify(doc));
  if (undoStack.length > 64) undoStack.shift();
  redoStack = [];
}

function undo() {
  if (!undoStack.length) return;
  redoStack.push(JSON.stringify(doc));
  doc = JSON.parse(undoStack.pop());
  sel = null; renderAll();
}

function redo() {
  if (!redoStack.length) return;
  undoStack.push(JSON.stringify(doc));
  doc = JSON.parse(redoStack.pop());
  sel = null; renderAll();
}

// ---------------------------------------------------------------- helpers

const $ = (id) => document.getElementById(id);

function el(tag, attrs = {}, ...kids) {
  const n = document.createElement(tag);
  for (const [k, v] of Object.entries(attrs)) {
    if (v == null) continue; // null = explicitly do not set the attribute
    if (k === "style") Object.assign(n.style, v);
    else if (k.startsWith("on")) n.addEventListener(k.slice(2), v);
    else if (k === "value" || k === "checked") n[k] = v;
    else n.setAttribute(k, v);
  }
  for (const kid of kids) if (kid != null) n.append(kid);
  return n;
}

// pdfloom uses f64s; keep integers for draggability.
const snapPt = (v) => Math.round(v * 2) / 2; // 0.5pt grid

function pageOf(i) { return doc.pages[i]; }
function contentElements(page) {
  return page.elements.filter((e) => e.type !== "rect" && e.type !== "line");
}
function sortKey(e) { return e.order ?? 0; }

function uniqueName(base) {
  const names = new Set();
  for (const p of doc.pages) for (const e of p.elements)
    if (e.type === "form_field") names.add(e.name);
  let n = base, i = 1;
  while (names.has(n)) n = base + "_" + (++i);
  return n;
}

function defaultFont(size) {
  return { family: "Liberation Sans", style: "Regular", size, color: "#000000" };
}

function nextOrder(page) {
  return contentElements(page).reduce((m, e) => Math.max(m, e.order ?? 0), 0) + 1;
}

// ---------------------------------------------------------------- elements

const CREATORS = {
  heading(page) {
    return { type: "heading", level: 1, text: "Heading",
             rect: [72, 680, 468, 28], font: { family: "Liberation Sans", style: "Bold", size: 20, color: "#000000" },
             order: nextOrder(page) };
  },
  paragraph(page) {
    return { type: "paragraph", text: "A paragraph. Edit this text in the properties panel.",
             rect: [72, 600, 468, 60], font: defaultFont(11), order: nextOrder(page) };
  },
  list(page) {
    return { type: "list", items: ["First item", "Second item"],
             rect: [72, 500, 468, 56], font: defaultFont(11), order: nextOrder(page) };
  },
  table(page) {
    return { type: "table", rect: [72, 380, 468, 90],
             header: ["Column A", "Column B"], rows: [["one", "two"], ["three", "four"]],
             widths: [234, 234], font: defaultFont(10), order: nextOrder(page) };
  },
  image(page) {
    return { type: "image", src: "", alt: "",
             rect: [72, 300, 96, 96], order: nextOrder(page) };
  },
  form_field(page) {
    return { type: "form_field", label: "Field label", name: uniqueName("field"),
             rect: [72, 240, 300, 24], font: defaultFont(11), order: nextOrder(page) };
  },
  rect(page) {
    return { type: "rect", rect: [72, 120, 120, 60], fill: "#dde7f2", stroke: null,
             stroke_width: 1, order: 900 };
  },
  line(page) {
    const r = [72, 96, 324, 1];
    return { type: "line", rect: r, from: [72, 96], to: [396, 96],
             stroke: "#888888", stroke_width: 1, order: 901 };
  },
};

function addElement(type) {
  if (type === "image") { $("image-file").click(); return; }
  snapshot();
  const page = pageOf(current);
  const e = CREATORS[type](page);
  page.elements.push(e);
  sel = e;
  renderAll();
}

// ---------------------------------------------------------------- render

function renderAll() {
  renderPagesList();
  renderCanvas();
  renderProps();
  persist();
}

function renderPagesList() {
  const list = $("pages-list");
  list.textContent = "";
  doc.pages.forEach((p, i) => {
    const w = p.size[0], h = p.size[1];
    const thumb = el("button", {
      class: "page-thumb" + (i === current ? " active" : ""),
      onclick: () => { current = i; sel = null; renderAll(); },
      title: `Page ${i + 1} (${w}x${h}pt)` });
    const label = el("span", {}, `${i + 1}`);
    thumb.append(label);
    list.append(thumb);
  });
  list.append(el("button", {
    class: "page-remove",
    onclick: () => removeCurrentPage(),
    title: "Remove current page",
    disabled: doc.pages.length <= 1 ? true : null,
  }, "remove"));
}

function removeCurrentPage() {
  if (doc.pages.length <= 1) return;
  snapshot();
  doc.pages.splice(current, 1);
  current = Math.min(current, doc.pages.length - 1);
  sel = null;
  renderAll();
}

function renderCanvas() {
  const canvas = $("canvas");
  canvas.textContent = "";
  canvas.style.transformOrigin = "top left";
  const page = pageOf(current);
  const [w, h] = page.size;

  const pageEl = el("div", { class: "page", id: "the-page",
    style: { width: w + "px", height: h + "px" },
    mousedown: canvasBackgroundDown });
  const scaleBox = el("div", { id: "scale-box",
    style: { transform: `scale(${zoom})` } }, pageEl);

  // Decorations first (bottom z), then content in reading order.
  const ordered = contentElements(page).slice().sort((a, b) => sortKey(a) - sortKey(b));
  for (const e of page.elements) if (e.type === "rect" || e.type === "line")
    pageEl.append(elementView(e));
  for (const e of ordered) pageEl.append(elementView(e));

  canvas.append(scaleBox);
  $("zoom-label").textContent = Math.round(zoom * 100) + "%";
}

function elementView(e) {
  const [x, y, w, h] = e.rect;
  // DOM: top-left origin; PDF: bottom-left. Translate.
  const top = pageOf(current).size[1] - y - h;
  const box = el("div", {
    class: "element " + e.type + (sel === e ? " selected" : ""),
    style: { left: x + "px", top: top + "px", width: w + "px", height: h + "px" },
    onmousedown: (ev) => elementDown(ev, e),
    "data-type": e.type,
  });

  const f = e.font || defaultFont(11);
  const cssFont = `${f.style === "Italic" || f.style === "Bold Italic" ? "italic " : ""}` +
    `${f.style === "Bold" || f.style === "Bold Italic" ? "700 " : ""}` +
    `${f.size}px ${cssFamily(f.family)}`;
  const color = f.color || "#000";

  switch (e.type) {
    case "heading":
    case "paragraph": {
      box.style.font = cssFont;
      box.style.color = color;
      box.textContent = e.text || "";
      break;
    }
    case "list": {
      box.style.font = cssFont; box.style.color = color;
      const ul = el("ul");
      for (const item of e.items || []) ul.append(el("li", {}, item));
      box.append(ul);
      break;
    }
    case "table": {
      const table = el("table", { style: { font: cssFont, color } });
      const trh = el("tr");
      for (const cell of e.header || []) trh.append(el("th", {}, cell));
      table.append(trh);
      for (const row of e.rows || []) {
        const tr = el("tr");
        for (let i = 0; i < (e.header || []).length; i++)
          tr.append(el("td", {}, row[i] ?? ""));
        table.append(tr);
      }
      if (e.widths) {
        const colw = e.widths.map((cw) => cw + "px").join(",");
        table.style.tableLayout = "fixed";
        let i = 0;
        for (const th of table.rows[0].cells) th.style.width = e.widths[i++] + "px";
      }
      box.append(table);
      break;
    }
    case "image": {
      if (!e.src) {
        box.append(el("span", { class: "img-placeholder" }, "image"));
        box.classList.add("empty-image");
        box.append(el("div", { class: "hint" }, "pick file in properties"));
      } else if (e.src.startsWith("data:")) {
        box.append(el("img", { src: e.src, alt: "" }));
      } else {
        box.append(el("span", { class: "img-placeholder" }, "img: " + e.src.split("/").pop()));
      }
      break;
    }
    case "form_field": {
      box.style.font = cssFont; box.style.color = color;
      const labelW = Math.round(w * 0.4);
      box.append(el("span", { class: "ff-label",
        style: { width: labelW + "px" } }, e.label || ""));
      box.append(el("span", { class: "ff-box",
        style: { left: labelW + "px", width: (w - labelW) + "px", height: h + "px" } }));
      break;
    }
    case "rect": {
      if (e.fill) box.style.background = e.fill;
      if (e.stroke) {
        box.style.border = `${e.stroke_width || 1}px solid ${e.stroke}`;
      }
      break;
    }
    case "line": {
      // For v1, only axis-aligned lines render faithfully in the DOM;
      // the config stores from/to, and the builder draws exactly those.
      const dx = e.to[0] - e.from[0], dy = e.to[1] - e.from[1];
      const len = Math.sqrt(dx * dx + dy * dy);
      const ang = Math.atan2(-dy, dx); // DOM y is inverted
      const bar = el("div", { class: "line-bar",
        style: { width: len + "px",
                 borderTop: `${e.stroke_width || 1}px solid ${e.stroke || "#000"}`,
                 transform: `rotate(${ang}rad)` } });
      box.classList.add("line-el");
      box.style.height = "0px";
      box.append(bar);
      break;
    }
  }

  if (showOrder && !e.type.match(/rect|line/)) {
    box.append(el("span", { class: "order-badge" }, String(e.order ?? 0)));
  }

  if (sel === e) {
    // Eight resize handles: four corners (both edges track the mouse)
    // and four edge midpoints (width-only or height-only resizing).
    // Spec keys name which PDF edges follow the drag.
    const HANDLES = [
      ["nw", { left: 1, top: 1 }, "0%", "0%"],
      ["n", { top: 1 }, "50%", "0%"],
      ["ne", { right: 1, top: 1 }, "100%", "0%"],
      ["e", { right: 1 }, "100%", "50%"],
      ["se", { right: 1, bottom: 1 }, "100%", "100%"],
      ["s", { bottom: 1 }, "50%", "100%"],
      ["sw", { left: 1, bottom: 1 }, "0%", "100%"],
      ["w", { left: 1 }, "0%", "50%"],
    ];
    for (const [cls, spec, hx, hy] of HANDLES) {
      box.append(el("div", { class: "handle " + cls,
        style: { left: hx, top: hy },
        onmousedown: (ev) => handleDown(ev, e, spec) }));
    }
  }
  return box;
}

function cssFamily(f) {
  switch (f) {
    case "Liberation Serif": case "Times-Roman": return '"Liberation Serif", "Times New Roman", serif';
    case "Liberation Mono": case "Courier": return '"Liberation Mono", "Courier New", monospace';
    case "Atkinson Hyperlegible": return '"Atkinson Hyperlegible", sans-serif';
    case "OpenDyslexic": return '"OpenDyslexic", "Comic Sans MS", sans-serif';
    default: return `"${f}", "Liberation Sans", Arial, sans-serif`;
  }
}

// ------------------------------------------------------------- interaction

function canvasBackgroundDown(ev) {
  if (ev.target.id !== "the-page") return;
  select(null);
  ev.preventDefault();
}

function elementDown(ev, e) {
  select(e);
  const startX = ev.clientX, startY = ev.clientY;
  const [x, y, w, h] = e.rect;
  let moved = false;
  const move = (ev2) => {
    const dx = (ev2.clientX - startX) / zoom;
    const dy = -(ev2.clientY - startY) / zoom; // invert: PDF y up
    if (Math.abs(dx) + Math.abs(dy) > 1) {
      if (!moved) { snapshot(); moved = true; }
      e.rect = [snapPt(x + dx), snapPt(y + dy), w, h];
      if (e.type === "line") { e.from = [e.rect[0], e.rect[1] + e.rect[3]]; e.to = [e.rect[0] + e.rect[2], e.rect[1] + e.rect[3]]; }
      renderCanvas(); renderProps();
    }
  };
  const up = () => {
    document.removeEventListener("mousemove", move);
    document.removeEventListener("mouseup", up);
  };
  document.addEventListener("mousemove", move);
  document.addEventListener("mouseup", up);
  ev.preventDefault();
  ev.stopPropagation();
}

// Resize from any combination of edges. The edges NOT in `spec` stay
// anchored; the edges in it track the mouse. Corners resize both
// dimensions, edge handles only one. Coordinates are PDF points with
// y growing upward, and dy is already inverted for that.
function handleDown(ev, e, spec) {
  const startX = ev.clientX, startY = ev.clientY;
  const [x, y, w, h] = e.rect;
  let moved = false;
  const move = (ev2) => {
    const dx = (ev2.clientX - startX) / zoom;
    const dy = -(ev2.clientY - startY) / zoom;
    if (!moved) { snapshot(); moved = true; }
    let nx = x, ny = y, nw = w, nh = h;
    if (spec.left) {
      nx = snapPt(x + dx);
      nw = snapPt(w - (nx - x)); // keep the right edge anchored
    }
    if (spec.right) {
      nw = snapPt(w + dx); // left edge stays put
    }
    if (spec.top) {
      nh = snapPt(h + dy); // bottom edge stays put
    }
    if (spec.bottom) {
      ny = snapPt(y + dy);
      nh = snapPt(h - (ny - y)); // keep the top edge anchored
    }
    if (nw < 8) nw = 8;
    if (nh < 8) nh = 8;
    e.rect = [nx, ny, nw, nh];
    if (e.type === "line") { e.from = [nx, ny + nh]; e.to = [nx + nw, ny + nh]; }
    renderCanvas(); renderProps();
  };
  const up = () => {
    document.removeEventListener("mousemove", move);
    document.removeEventListener("mouseup", up);
  };
  document.addEventListener("mousemove", move);
  document.addEventListener("mouseup", up);
  ev.preventDefault();
  ev.stopPropagation();
}

function select(e) { sel = e; renderCanvas(); renderProps(); }

document.addEventListener("keydown", (ev) => {
  if (ev.target.closest("input,textarea,select")) return;
  if ((ev.metaKey || ev.ctrlKey) && ev.key === "z") { ev.preventDefault(); ev.shiftKey ? redo() : undo(); return; }
  if ((ev.metaKey || ev.ctrlKey) && ev.key === "d") { ev.preventDefault(); duplicate(); return; }
  if (!sel) return;
  const nudge = ev.shiftKey ? 10 : 1;
  switch (ev.key) {
    case "Backspace": case "Delete": {
      snapshot();
      const page = pageOf(current);
      page.elements.splice(page.elements.indexOf(sel), 1);
      sel = null; renderAll(); ev.preventDefault(); break;
    }
    case "ArrowLeft": sel.rect[0] -= nudge; renderCanvas(); renderProps(); ev.preventDefault(); break;
    case "ArrowRight": sel.rect[0] += nudge; renderCanvas(); renderProps(); ev.preventDefault(); break;
    case "ArrowUp": sel.rect[1] += nudge; renderCanvas(); renderProps(); ev.preventDefault(); break;
    case "ArrowDown": sel.rect[1] -= nudge; renderCanvas(); renderProps(); ev.preventDefault(); break;
  }
});

function duplicate() {
  if (!sel) return;
  snapshot();
  const copy = JSON.parse(JSON.stringify(sel));
  copy.rect = [copy.rect[0] + 12, Math.max(12, copy.rect[1] - 12), copy.rect[2], copy.rect[3]];
  if (copy.type === "line") { copy.from[0] += 12; copy.to[0] += 12; copy.from[1] -= 12; copy.to[1] -= 12; }
  if (copy.type === "form_field") copy.name = uniqueName(copy.name);
  if (copy.order != null) copy.order = nextOrder(pageOf(current));
  pageOf(current).elements.push(copy);
  sel = copy;
  renderAll();
}

// ------------------------------------------------------------- properties

function renderProps() {
  const props = $("props");
  props.textContent = "";
  const empty = $("props-empty");
  const orderBox = $("order-box");
  if (!sel) {
    empty.style.display = "block";
    orderBox.style.display = "none";
    $("props-title").textContent = "Document";
    props.append(docProps());
    return;
  }
  empty.style.display = "none";
  orderBox.style.display = sel.type.match(/rect|line/) ? "none" : "block";
  $("order-input").value = sel.order ?? 0;
  $("props-title").textContent = sel.type.replace("_", " ");

  const f = sel.font || defaultFont(11);
  switch (sel.type) {
    case "heading": {
      props.append(
        labeled("Level", number(1, 6, 1, sel.level, (v) => { snapshot(); sel.level = v; renderAll(); })),
        labeled("Text", textArea(sel.text, (v) => { snapshot(); sel.text = v; renderCanvas(); })),
      );
      props.append(fontProps());
      break;
    }
    case "paragraph": {
      props.append(labeled("Text", textArea(sel.text, (v) => { snapshot(); sel.text = v; renderCanvas(); })));
      props.append(fontProps());
      break;
    }
    case "list": {
      props.append(labeled("Items (one per line)", textArea((sel.items || []).join("\n"),
        (v) => { snapshot(); sel.items = v.split("\n"); renderCanvas(); })));
      props.append(fontProps());
      break;
    }
    case "table": {
      props.append(
        labeled("Header (comma-separated)", input((sel.header || []).join(", "),
          (v) => { snapshot(); sel.header = v.split(",").map((s) => s.trim()); renderCanvas(); })),
        labeled("Rows (rows separated by ;, cells by ,)",
          textArea((sel.rows || []).map((r) => r.join(", ")).join("\n"),
            (v) => { snapshot(); sel.rows = v.split("\n").map((line) => line.split(",").map((s) => s.trim())).filter((r) => r.length); renderCanvas(); })),
      );
      props.append(fontProps());
      break;
    }
    case "image": {
      props.append(
        el("button", { class: "pick-image", onclick: () => $("image-file").click() }, "Choose PNG/JPEG"),
        labeled("Alt text (empty = decorative)",
          input(sel.alt || "", (v) => { snapshot(); sel.alt = v; renderCanvas(); })),
      );
      break;
    }
    case "form_field": {
      props.append(
        labeled("Label", input(sel.label || "", (v) => { snapshot(); sel.label = v; renderCanvas(); })),
        labeled("Field name (/T)", input(sel.name || "", (v) => { snapshot(); sel.name = v; })),
      );
      props.append(fontProps());
      break;
    }
    case "rect": {
      props.append(
        labeled("Fill (#rrggbb, empty = none)", input(sel.fill || "", (v) => { snapshot(); sel.fill = v || null; renderCanvas(); })),
        labeled("Stroke (#rrggbb, empty = none)", input(sel.stroke || "", (v) => { snapshot(); sel.stroke = v || null; renderCanvas(); })),
        labeled("Stroke width", number(0.25, 12, 0.25, sel.stroke_width ?? 1, (v) => { snapshot(); sel.stroke_width = v; renderCanvas(); })),
      );
      break;
    }
    case "line": {
      props.append(
        labeled("Stroke color", input(sel.stroke || "#000000", (v) => { snapshot(); sel.stroke = v; renderCanvas(); })),
        labeled("Width", number(0.25, 12, 0.25, sel.stroke_width ?? 1, (v) => { snapshot(); sel.stroke_width = v; renderCanvas(); })),
      );
      break;
    }
  }

  // Geometry readout.
  const [x, y, w, h] = sel.rect;
  props.append(el("div", { class: "geom" }, `x ${x}  y ${y}  w ${w}  h ${h} (pt)`));
}

function docProps() {
  const p = $("props");
  const box = el("div", { class: "doc-props" });
  box.append(
    labeled("Title", input(doc.title, (v) => { snapshot(); doc.title = v; })),
    labeled("Language", input(doc.lang, (v) => { snapshot(); doc.lang = v; })),
    labeled("Profile", selectFor(["ua1", "ua2"], doc.profile, (v) => { snapshot(); doc.profile = v; })),
  );
  return box;
}

function fontProps() {
  const f = sel.font || defaultFont(11);
  const setFont = (mut) => {
    snapshot();
    const fo = sel.font ||= defaultFont(11);
    mut(fo);
    renderAll();
  };
  return el("div", { class: "font-props" },
    labeled("Family", selectFor(FONT_FAMILIES, f.family, (v) => setFont((fo) => {
      fo.family = v;
      fo.style = clampStyle(v, fo.style); // keep the combo buildable
    }))),
    labeled("Style", selectFor(stylesFor(f.family), f.style,
      (v) => setFont((fo) => { fo.style = v; }))),
    labeled("Size (pt)", number(4, 96, 0.5, f.size, (v) => setFont((fo) => { fo.size = v; }))),
    labeled("Color", el("input", { type: "color", value: /^#[0-9a-f]{6}$/i.test(f.color) ? f.color : "#000000",
      oninput: (ev) => setFont((fo) => { fo.color = ev.target.value; }) })),
  );
}

function labeled(label, control) {
  return el("label", { class: "prop" }, el("span", { class: "prop-label" }, label), control);
}
function input(value, onchange) {
  const i = el("input", { type: "text", value });
  i.addEventListener("change", () => onchange(i.value));
  i.addEventListener("input", () => { i._t && clearTimeout(i._t); i._t = setTimeout(() => onchange(i.value), 400); });
  return i;
}
function textArea(value, onchange) {
  const t = el("textarea", { value });
  t.addEventListener("change", () => onchange(t.value));
  t.addEventListener("input", () => { t._t && clearTimeout(t._t); t._t = setTimeout(() => onchange(t.value), 400); });
  return t;
}
function number(min, max, step, value, onchange) {
  const i = el("input", { type: "number", min, max, step, value });
  i.addEventListener("change", () => { const v = parseFloat(i.value); if (!isNaN(v)) onchange(v); });
  return i;
}
function selectFor(options, value, onchange) {
  const s = el("select");
  for (const o of options) s.append(el("option", { value: o, selected: o === value ? "" : null }, o));
  s.addEventListener("change", () => onchange(s.value));
  return s;
}

$("order-input").addEventListener("change", () => {
  if (!sel) return;
  snapshot();
  sel.order = parseInt($("order-input").value, 10) || 0;
  renderAll();
});

// ------------------------------------------------------------- toolbar

for (const b of document.querySelectorAll("[data-add]"))
  b.addEventListener("click", () => addElement(b.dataset.add));

$("add-page").addEventListener("click", () => {
  snapshot();
  const key = $("page-size").value;
  const [w, h] = PAGE_SIZES[key] || [612, 792];
  doc.pages.push({ size: [w, h], elements: [] });
  current = doc.pages.length - 1;
  sel = null;
  renderAll();
});

for (const [k, v] of Object.entries(PAGE_SIZES))
  $("page-size").append(el("option", { value: k }, k));

$("zoom-in").addEventListener("click", () => { zoom = Math.min(2.5, zoom * 1.25); renderCanvas(); });
$("zoom-out").addEventListener("click", () => { zoom = Math.max(0.3, zoom / 1.25); renderCanvas(); });
$("toggle-order").addEventListener("click", (ev) => {
  showOrder = !showOrder; ev.target.classList.toggle("on", showOrder); renderCanvas();
});

$("preview-close").addEventListener("click", () => {
  showPreview = false; $("preview-panel").hidden = true;
});
$("new-doc").addEventListener("click", () => {
  if (!confirm("Discard this document and start fresh?")) return;
  snapshot(); doc = newDoc(); sel = null; current = 0; renderAll();
  setBadge("not built yet", "unknown");
  clearPreview();
});

// ---------------------------------------------------------------- files

$("image-file").addEventListener("change", async () => {
  const file = $("image-file").files[0];
  if (!file) return;
  const reader = new FileReader();
  reader.onload = () => {
    if (!sel || sel.type !== "image") {
      // Adding a NEW image element with this file.
      snapshot();
      const page = pageOf(current);
      const e = CREATORS.image(page);
      e.src = reader.result; // data: URL
      e.alt = file.name.replace(/\.[a-z]+$/i, "");
      page.elements.push(e);
      sel = e;
    } else {
      snapshot(); sel.src = reader.result;
    }
    renderAll();
  };
  reader.readAsDataURL(file);
  $("image-file").value = "";
});

$("export-config").addEventListener("click", () => {
  downloadBlob(new Blob([JSON.stringify(doc, null, 2)], { type: "application/json" }),
    (doc.title || "document").replace(/[^\w-]+/g, "_") + ".pdfloom.json");
});

$("import-config").addEventListener("click", () => $("config-file").click());
$("config-file").addEventListener("change", async () => {
  const file = $("config-file").files[0];
  if (!file) return;
  try {
    const parsed = JSON.parse(await file.text());
    if (parsed.format !== "pdfloom-doc") throw new Error("not a pdfloom-doc file");
    snapshot();
    doc = parsed;
    current = 0; sel = null;
    renderAll();
    setStatus("config imported");
  } catch (e) {
    alert("Could not import: " + e.message);
  }
  $("config-file").value = "";
});

// --------------------------------------------------------------- export

// Show a centered state message in the preview pane; kind is "" (idle
// placeholder), "loading" or "error". No message = hide the overlay.
function previewState(message, kind = "") {
  const state = $("preview-state");
  $("preview-message").textContent = message || "";
  state.classList.toggle("loading", kind === "loading");
  state.classList.toggle("error", kind === "error");
  state.hidden = !message;
}

async function buildPdf() {
  setStatus("building...");
  if (showPreview) previewState("Building the PDF\u2026", "loading");
  try {
    const res = await fetch("/api/build", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ def: doc }),
    });
    if (!res.ok) {
      const t = await res.text();
      setBadge("build error", "bad");
      setStatus("build failed: " + t.slice(0, 300));
      if (showPreview) previewState("The build failed:\n" + t.slice(0, 300), "error");
      return null;
    }
    const data = await res.json();
    showReport(data.report);
    const bytes = Uint8Array.from(atob(data.pdf_base64), (c) => c.charCodeAt(0));
    return bytes;
  } catch (e) {
    setBadge("offline?", "bad");
    setStatus("could not reach pdfloom-server: " + e.message);
    if (showPreview) previewState("Could not reach pdfloom-server:\n" + e.message, "error");
    return null;
  }
}

function showReport(r) {
  if (String(r.status).toLowerCase() === "compliant") {
    setBadge("PDF/UA compliant", "ok");
    setStatus("compliant as " + r.profile_name);
  } else {
    setBadge("not compliant yet", "warn");
    const vs = (r.violations || []).map((v) => `${v.id}: ${v.message}`).join("; ");
    setStatus("file saved WITHOUT the conformance claim. " + (vs || r.note || ""));
  }
}

$("export-pdf").addEventListener("click", async () => {
  const bytes = await buildPdf();
  if (bytes) {
    downloadBlob(new Blob([bytes], { type: "application/pdf" }),
      (doc.title || "document").replace(/[^\w-]+/g, "_") + ".pdf");
    // Also refresh the preview if open.
    if (showPreview) showPdfInPreview(bytes);
  }
});

// Load bytes into the preview iframe (one object URL at a time).
function showPdfInPreview(bytes) {
  const frame = $("preview-frame");
  if (frame.dataset.url) URL.revokeObjectURL(frame.dataset.url);
  const url = URL.createObjectURL(new Blob([bytes], { type: "application/pdf" }));
  frame.dataset.url = url;
  frame.src = url;
  previewState(null); // hide any placeholder/loading/error overlay
}

function clearPreview() {
  const frame = $("preview-frame");
  if (frame.dataset.url) URL.revokeObjectURL(frame.dataset.url);
  delete frame.dataset.url;
  frame.src = "about:blank";
  if (showPreview) previewState("Click preview to build the PDF");
}

async function preview(bytes) {
  if (bytes) { showPdfInPreview(bytes); return; }
  const b = await buildPdf();
  if (!b) return; // buildPdf already put the error into the pane
  showPdfInPreview(b);
}
// Re-point the preview toggle so it builds once when opening.
$("toggle-preview").onclick = async () => {
  if (!showPreview) {
    showPreview = true;
    $("preview-panel").hidden = false;
    if (!$("preview-frame").dataset.url) previewState("Click preview to build the PDF");
    await preview();
  } else {
    showPreview = false;
    $("preview-panel").hidden = true;
  }
};

function downloadBlob(blob, name) {
  const a = el("a", { href: URL.createObjectURL(blob), download: name });
  document.body.append(a);
  a.click();
  a.remove();
  setTimeout(() => URL.revokeObjectURL(a.href), 5000);
}

function setBadge(text, cls) { $("compliance-badge").textContent = text; $("compliance-badge").className = cls; }
function setStatus(text) { $("status-text").textContent = text; }

// ------------------------------------------------------------ persistence

function persist() {
  try { localStorage.setItem("pdfloom-studio-doc", JSON.stringify(doc)); } catch {}
}

(function init() {
  try {
    const saved = JSON.parse(localStorage.getItem("pdfloom-studio-doc"));
    if (saved && saved.format === "pdfloom-doc") doc = saved;
  } catch {}
  if (!doc) doc = newDoc();
  renderAll();
})();
