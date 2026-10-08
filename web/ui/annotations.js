// Screen-space annotations are shared too. Rust owns projection and picking;
// this transparent canvas owns text/rings, never DOM input or the 3D scene.
export function mountAnnotations(bridge) {
  const layer = document.createElement('canvas');
  layer.id = 'annotations';
  Object.assign(layer.style, { position: 'fixed', inset: '0', width: '100%', height: '100%', pointerEvents: 'none', zIndex: '1' });
  document.body.append(layer);
  const ctx = layer.getContext('2d');
  bridge.on('overlay', ({ size, rect, bodies, picked, directions, probe_frame }) => {
    const dpr = window.devicePixelRatio || 1;
    const width = Math.round(size[0] * dpr), height = Math.round(size[1] * dpr);
    if (layer.width !== width || layer.height !== height) { layer.width = width; layer.height = height; }
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.clearRect(0, 0, ...size);
    ctx.save();
    ctx.beginPath(); ctx.rect(...rect); ctx.clip();
    ctx.font = '12px system-ui, sans-serif';
    const circle = (x, y, r) => { ctx.beginPath(); ctx.arc(x, y, r, 0, Math.PI * 2); ctx.stroke(); };
    for (const b of bodies) {
      ctx.strokeStyle = ctx.fillStyle = `rgb(${b.colour.join(',')})`;
      ctx.lineWidth = 1;
      if (b.selected || b.hovered) circle(b.x, b.y, b.radius + 5);
      if (b.marker) circle(b.x, b.y, 4);
      if (!b.label) continue;
      // Wrap at word boundaries, including long catalogue/task identifiers.
      const lines = [];
      for (const paragraph of b.label.split('\n')) {
        let line = '';
        for (let word of paragraph.split(/\s+/)) {
          if (line && ctx.measureText(`${line} ${word}`).width > b.width) { lines.push(line); line = ''; }
          // Long identifiers still need character wrapping, ordinary words do not.
          while (ctx.measureText(word).width > b.width) {
            let count = 1;
            while (count < word.length && ctx.measureText(word.slice(0, count + 1)).width <= b.width) count++;
            lines.push(word.slice(0, count)); word = word.slice(count);
          }
          line += (line ? ' ' : '') + word;
        }
        lines.push(line);
      }
      const width = Math.max(...lines.map(line => ctx.measureText(line).width));
      const x = b.table ? Math.max(rect[0] + 10, Math.min(b.x - width / 2, rect[0] + rect[2] - width - 10))
        : b.x + b.radius + width + 10 > rect[0] + rect[2] - 10 ? Math.max(rect[0] + 10, b.x - b.radius - width - 10) : b.x + b.radius + 10;
      const y = b.table ? b.y + b.radius + 20 : b.y - lines.length * 14 / 2 + 11;
      lines.forEach((line, i) => ctx.fillText(line, x, y + i * 14));
    }
    ctx.strokeStyle = ctx.fillStyle = '#63d6f0'; ctx.lineWidth = 1.5;
    if (picked) { const [[x, y], label] = picked; circle(x, y, 10); ctx.fillText(label, x + 16, y - 4); }
    for (const { point: [x, y], prograde } of directions) {
      circle(x, y, 9);
      if (prograde) { ctx.beginPath(); ctx.arc(x, y, 2.5, 0, Math.PI * 2); ctx.fill(); }
      else { ctx.beginPath(); ctx.moveTo(x - 4, y - 4); ctx.lineTo(x + 4, y + 4); ctx.moveTo(x - 4, y + 4); ctx.lineTo(x + 4, y - 4); ctx.stroke(); }
    }
    ctx.restore();
    // Diagnostic only: encode the originating Rust frame in six opaque device
    // pixels, so the host can measure when its actual CEF pixels come back.
    if (probe_frame != null) {
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.fillStyle = 'rgb(53,121,211)'; ctx.fillRect(0, 0, 1, 1);
      ctx.fillStyle = 'rgb(91,17,73)'; ctx.fillRect(1, 0, 1, 1);
      for (let i = 0; i < 4; i++) { ctx.fillStyle = `rgb(${(probe_frame >>> (i * 8)) & 255},0,0)`; ctx.fillRect(i + 2, 0, 1, 1); }
    }
  });
}
