/*
 * Renders part of the page to a PNG, for sharing an achievement as an image. The node is drawn
 * by the browser itself: it is put in an SVG <foreignObject> together with the page's styles,
 * and the SVG is drawn onto a canvas. An SVG drawn as an image loads nothing from outside, so
 * the fonts are inlined as `data:` URLs; the shelf's images already are `data:` URLs.
 */

const blobToDataUrl = (blob: Blob) =>
  new Promise<string>((resolve, reject) => {
    const reader = new FileReader();
    reader.onload = () => resolve(reader.result as string);
    reader.onerror = () => reject(reader.error);
    reader.readAsDataURL(blob);
  });

async function inlineUrls(css: string, base: string): Promise<string> {
  const urls = [...css.matchAll(/url\(\s*(['"]?)([^'")]+)\1\s*\)/g)];
  let out = css;
  for (const [match, , url] of urls) {
    if (url.startsWith("data:")) continue;
    const response = await fetch(new URL(url, base));
    if (!response.ok) throw new Error(`cannot load ${url}: ${response.status}`);
    out = out.replace(match, `url("${await blobToDataUrl(await response.blob())}")`);
  }
  return out;
}

let pageCss: Promise<string> | null = null;

/** The page's styles, with the fonts they use inlined. Loaded once. */
function stylesheet(): Promise<string> {
  pageCss ??= (async () => {
    const parts: string[] = [];
    for (const sheet of document.styleSheets) {
      const base = sheet.href ?? document.baseURI;
      for (const rule of sheet.cssRules) {
        parts.push(rule instanceof CSSFontFaceRule ? await inlineUrls(rule.cssText, base) : rule.cssText);
      }
    }
    return parts.join("\n");
  })();
  pageCss.catch(() => (pageCss = null));
  return pageCss;
}

/**
 * Draws `node` onto a canvas at `scale` device pixels per CSS pixel. `node` is laid out out of
 * sight on the page first, to measure it, so it must not be in the page yet.
 */
export async function renderCanvas(node: HTMLElement, scale: number): Promise<HTMLCanvasElement> {
  node.style.position = "fixed";
  node.style.left = "-10000px";
  node.style.top = "0";
  document.body.append(node);
  const width = Math.ceil(node.offsetWidth);
  const height = Math.ceil(node.offsetHeight);
  node.remove();
  node.removeAttribute("style");

  const style = document.createElement("style");
  style.textContent = await stylesheet();
  const root = document.createElement("div");
  root.append(style, node);
  const svg =
    `<svg xmlns="http://www.w3.org/2000/svg" width="${width}" height="${height}">` +
    `<foreignObject width="100%" height="100%">${new XMLSerializer().serializeToString(root)}</foreignObject></svg>`;

  const img = new Image();
  img.src = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
  await img.decode();
  const canvas = document.createElement("canvas");
  canvas.width = Math.round(width * scale);
  canvas.height = Math.round(height * scale);
  const ctx = canvas.getContext("2d")!;
  ctx.scale(scale, scale);
  ctx.drawImage(img, 0, 0, width, height);
  return canvas;
}

/** The canvas as a PNG, base64. */
export const pngBase64 = (canvas: HTMLCanvasElement) =>
  canvas.toDataURL("image/png").slice("data:image/png;base64,".length);

/** The canvas's pixels as RGBA rows, top first, base64. */
export function rgbaBase64(canvas: HTMLCanvasElement): string {
  const { data } = canvas.getContext("2d")!.getImageData(0, 0, canvas.width, canvas.height);
  let binary = "";
  for (let i = 0; i < data.length; i += 0x8000) binary += String.fromCharCode(...data.subarray(i, i + 0x8000));
  return btoa(binary);
}
