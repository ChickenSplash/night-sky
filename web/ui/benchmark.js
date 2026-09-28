// Collect the same one-second telemetry on both builds; the app starts at home
// with an empty starfield when the benchmark query parameter is present.
const parse = text => {
  const lines = text.split('\n');
  const header = /STAR STREAM \[([^\]]+)\] (\d+) stars \/ (\d+) cells\s+·\s+(\d+) pending\s+·\s+(\d+) mesh chunks/.exec(lines[0]);
  const work = /GEN ([\d.]+) ms \/ (\d+) cells\s+·\s+(\d+) cancelled\s+·\s+RENDER SYNC ([\d.]+) ms \/ (\d+) chunks/.exec(lines[1]);
  const wait = /MESH WAIT ([\d.]+) ms avg \/ ([\d.]+) ms oldest\s+·\s+FRAME P95 ([\d.]+) \/ P99 ([\d.]+) ms/.exec(lines[2]);
  if (!header || !work || !wait) return null;
  return {
    mode: header[1], stars: +header[2], cells: +header[3], pending: +header[4], mesh_chunks: +header[5],
    generation_ms: +work[1], generated_cells: +work[2], cancelled: +work[3],
    render_sync_ms: +work[4], built_chunks: +work[5], mesh_wait_avg_ms: +wait[1],
    mesh_wait_oldest_ms: +wait[2], frame_p95_ms: +wait[3], frame_p99_ms: +wait[4],
  };
};

export function track() {
  const samples = [];
  const started = performance.now();
  let previous = '';
  const report = { done: false, route: 'home → galactic centre', speed_ly_s: 1000, duration_s: 15,
    initial_loaded_stars: 0, samples };
  window.starBenchmark = report;
  window.addEventListener('space:tick', event => {
    const tick = JSON.parse(event.detail);
    if (tick.benchmark_seconds == null || report.done) return;
    if (tick.stream !== previous) {
      previous = tick.stream;
      const fields = parse(tick.stream);
      if (fields) samples.push({ seconds: tick.benchmark_seconds, ...fields });
    }
    if (tick.benchmark_seconds < 15) return;
    report.done = true;
    report.wall_seconds = (performance.now() - started) / 1000;
    report.final = { ...samples.at(-1), seconds: tick.benchmark_seconds,
      stars: tick.stream_stars, cells: tick.stream_cells,
      pending: tick.stream_pending, mesh_chunks: tick.mesh_queue };
    report.summary = {
      generated_cells: samples.reduce((sum, s) => sum + s.generated_cells, 0),
      generation_ms: samples.reduce((sum, s) => sum + s.generation_ms, 0),
      render_sync_ms: samples.reduce((sum, s) => sum + s.render_sync_ms, 0),
      max_pending: Math.max(0, ...samples.map(s => s.pending)),
      max_mesh_chunks: Math.max(0, ...samples.map(s => s.mesh_chunks)),
      max_frame_p99_ms: Math.max(0, ...samples.map(s => s.frame_p99_ms)),
    };
    console.log('Star-stream benchmark:', JSON.stringify(report, null, 2));
    const panel = document.createElement('div');
    panel.id = 'star-benchmark-result';
    panel.style.cssText = 'position:fixed;right:16px;bottom:16px;z-index:10;max-width:330px;padding:16px;border:1px solid #38536c;border-radius:14px;background:#07101fe8;color:#d6e4f4;font:12px ui-monospace,monospace;white-space:pre-line';
    panel.textContent = `15-second flight complete\n${report.final?.mode ?? 'unknown mode'}: ${report.final?.stars ?? 0} stars, ${report.final?.pending ?? 0} cells pending\nP99 frame: ${report.final?.frame_p99_ms ?? 0} ms`;
    const download = document.createElement('button');
    download.textContent = 'Download results JSON';
    download.style.cssText = 'display:block;margin-top:12px;padding:8px;color:#d6e4f4;background:#12304a;border:1px solid #38536c;border-radius:8px;cursor:pointer';
    download.onclick = () => {
      const url = URL.createObjectURL(new Blob([JSON.stringify(report, null, 2)], { type: 'application/json' }));
      const link = document.createElement('a');
      link.href = url; link.download = `star-flight-${report.final?.mode.toLowerCase().replaceAll(' ', '-') ?? 'unknown'}.json`;
      link.click();
      setTimeout(() => URL.revokeObjectURL(url), 1000);
    };
    panel.append(download);
    document.body.append(panel);
  });
}
