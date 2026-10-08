#!/usr/bin/env python3
"""Release A/B/C/D benchmark, Linux + Hyprland. No global configuration changes."""
import argparse
import csv
import io
import hashlib
import json
import math
import os
from pathlib import Path
import shlex
import shutil
import signal
import statistics
import subprocess
import sys
import time
from datetime import datetime, timezone

ROOT = Path(__file__).resolve().parents[1]
MODES = ["baseline", "runtime", "idle", "active", "animation"]


def call(args, **kwargs):
    return subprocess.check_output(args, cwd=ROOT, text=True, **kwargs).strip()


def percentile(values, fraction):
    if not values:
        return None
    values = sorted(values)
    index = (len(values) - 1) * fraction
    low = int(index)
    high = min(low + 1, len(values) - 1)
    return values[low] + (values[high] - values[low]) * (index - low)


def frame_summary(frames):
    return {"frames": len(frames), "median_ms": percentile(frames, .5),
            "p95_ms": percentile(frames, .95), "p99_ms": percentile(frames, .99),
            "over_33_ms": sum(f > 33.333 for f in frames),
            "over_50_ms": sum(f > 50 for f in frames),
            "over_100_ms": sum(f > 100 for f in frames)}


def plan(repetitions, interactive):
    modes = MODES + (["interactive"] if interactive else [])
    jobs = []
    for repetition in range(repetitions):
        order = modes[repetition % len(modes):] + modes[:repetition % len(modes)]
        if repetition % 2:
            order.reverse()
        presentations = ["uncapped", "normal"] if repetition % 2 == 0 else ["normal", "uncapped"]
        for presentation in presentations:
            for workload in ["stationary", "flight"]:
                for mode in order:
                    jobs.append((repetition + 1, presentation, workload, mode))
    return jobs


def proc_table():
    result = {}
    for path in Path('/proc').glob('[0-9]*/stat'):
        try:
            fields = path.read_text().rsplit(')', 1)[1].split()
            result[int(path.parent.name)] = {
                'parent': int(fields[1]), 'ticks': int(fields[11]) + int(fields[12]),
                'start': int(fields[19]), 'rss_kib': int(fields[21]) * os.sysconf('SC_PAGE_SIZE') / 1024,
            }
        except (OSError, ValueError, IndexError):
            continue
    return result


def descendants(table, root, known):
    selected = {root} if root in table else set()
    selected |= {pid for pid, start in known.items() if pid in table and table[pid]['start'] == start}
    while True:
        expanded = selected | {pid for pid, row in table.items() if row['parent'] in selected}
        if expanded == selected:
            break
        selected = expanded
    for pid in selected:
        known[pid] = table[pid]['start']
    return selected


def memory(pid):
    try:
        rows = Path(f'/proc/{pid}/smaps_rollup').read_text().splitlines()
        return int(next(line.split()[1] for line in rows if line.startswith('Pss:')))
    except (OSError, StopIteration, ValueError):
        return None


def sample(pid, known, previous, now):
    table = proc_table()
    members = descendants(table, pid, known)
    ticks_per_second = os.sysconf('SC_CLK_TCK')
    cpu = {'bevy_cpu_percent': 0.0, 'helpers_cpu_percent': 0.0}
    observed = False
    for member in members:
        row = table[member]
        key = (member, row['start'])
        old = previous.get(key)
        if old and now > old[0]:
            observed = True
            percent = max(0, row['ticks'] - old[1]) / ticks_per_second / (now - old[0]) * 100
            cpu['bevy_cpu_percent' if member == pid else 'helpers_cpu_percent'] += percent
        previous[key] = (now, row['ticks'])
    pss = [memory(member) for member in members]
    return {**(cpu if observed else {k: None for k in cpu}),
            'cpu_total_percent': sum(cpu.values()) if observed else None, 'pids': sorted(members),
            'pss_mib': sum(pss) / 1024 if members and all(v is not None for v in pss) else None,
            'rss_mib': sum(table[p]['rss_kib'] for p in members) / 1024,
            'pss_complete': bool(members) and all(v is not None for v in pss)}


def gpu_sample():
    if not shutil.which('nvidia-smi'):
        return None
    try:
        data = call(['nvidia-smi', '--query-gpu=index,utilization.gpu,memory.used,memory.total',
                     '--format=csv,noheader,nounits'], timeout=3)
        return [dict(zip(['index', 'utilization_percent', 'used_mib', 'total_mib'], map(float, row)))
                for row in csv.reader(io.StringIO(data))]
    except (subprocess.SubprocessError, ValueError):
        return None


def check_monitor():
    if not shutil.which('hyprctl'):
        raise RuntimeError('This runner requires Hyprland (hyprctl) for safe secondary-monitor placement.')
    monitors = json.loads(call(['hyprctl', 'monitors', '-j']))
    monitor = next((m for m in monitors if m['name'] == 'HDMI-A-1'), None)
    if not monitor or monitor['disabled'] or not monitor.get('dpmsStatus', True) or monitor['activeWorkspace']['id'] != 8 or monitor['scale'] != 1:
        raise RuntimeError('Show workspace 8 on HDMI-A-1 at scale 1 before running. No primary-monitor changes will be made.')
    if monitor['width'] < 1440 or monitor['height'] < 1000:
        raise RuntimeError('Secondary monitor needs room for the fixed 1440 × 900 client window.')
    return monitor


def check_window(pid, monitor_id):
    clients = json.loads(call(['hyprctl', 'clients', '-j']))
    window = next((w for w in clients if w['pid'] == pid), None)
    if not window or window['monitor'] != monitor_id or window['workspace']['id'] != 8 or window.get('hidden', False) or not window.get('mapped', True):
        raise RuntimeError('Benchmark window is not visible on secondary workspace 8. Run aborted.')
    return window


def build(env):
    subprocess.run(['./scripts/setup-native-ui.sh'], cwd=ROOT, env=env, check=True)
    paths = {}
    for name, features in [('baseline', ['--no-default-features']), ('cef', [])]:
        directory = ROOT / 'target/cef-benchmark' / name
        subprocess.run(['cargo', 'build', '--release', '--locked', '-p', 'space-client', '-p', 'space-server',
                        '--target-dir', str(directory), '-j', '6', *features], cwd=ROOT, env=env, check=True)
        paths[name] = directory / 'release'
    return paths


def json_if_exists(path):
    try:
        return json.loads(path.read_text())
    except (OSError, ValueError):
        return None


def alive(pid):
    try:
        return Path(f'/proc/{pid}/stat').read_text().rsplit(')', 1)[1].split()[0] != 'Z'
    except (OSError, IndexError):
        return False


def stop(pid):
    if pid and alive(pid):
        try:
            os.kill(pid, signal.SIGTERM)
        except ProcessLookupError:
            pass


def run_job(folder, number, job, args, paths, monitor):
    repetition, presentation, workload, mode = job
    name = f'{number:03}-{mode}-{presentation}-{workload}-r{repetition}'
    config = folder / f'{name}.json'
    config.write_text(json.dumps({'mode': mode, 'workload': workload, 'presentation': presentation,
                                 'warmup_seconds': args.warmup, 'measure_seconds': args.seconds}))
    status_path = config.with_suffix('.status.json')
    result_path = config.with_suffix('.result.json')
    executable = paths['baseline' if mode == 'baseline' else 'cef'] / 'space-client'
    command = [str(executable), '--native-benchmark', str(config)]
    if mode == 'baseline':
        command.append('--ui=egui')
    if mode != 'baseline':
        library_path = str(ROOT / 'data/cef-sdk')
        if os.environ.get('LD_LIBRARY_PATH'):
            library_path += ':' + os.environ['LD_LIBRARY_PATH']
        command = ['env', f'CEF_PATH={ROOT / "data/cef-sdk"}',
                   f'LD_LIBRARY_PATH={library_path}', *command]
    shell = 'cd ' + shlex.quote(str(ROOT)) + ' && exec ' + shlex.join(command) + ' > ' + shlex.quote(str(folder / f'{name}.log')) + ' 2>&1'
    lua = 'hl.dsp.exec_cmd(' + json.dumps('bash -c ' + shlex.quote(shell)) + ", { workspace = '8 silent', no_focus = true, float = true, size = '1440 900', center = true })"
    print(f'[{number}] {mode} · {presentation} · {workload} · repeat {repetition}', flush=True)
    call(['hyprctl', 'dispatch', lua])
    launched = time.monotonic()
    pid = None
    known, previous, samples = {}, {}, []
    prompted = False
    manual_wait = 0.0
    window_info = None
    deadline = launched + args.warmup + args.seconds + 180
    try:
        while time.monotonic() < deadline:
            if pid is None:
                try:
                    pid = int(config.with_suffix('.pid').read_text())
                except (OSError, ValueError):
                    pass
            status = json_if_exists(status_path)
            if status:
                pid = status['pid']
                check_monitor()
                window_info = check_window(pid, monitor['id']) if alive(pid) else window_info
                if status['phase'] == 'ready' and mode == 'interactive' and not prompted:
                    waiting = time.monotonic()
                    input('Panel warmed. Press Enter, then type/select/paste and scroll in the HTML panel during the timed run: ')
                    manual_wait = time.monotonic() - waiting
                    deadline += manual_wait
                    config.with_suffix('.start').touch()
                    prompted = True
                now = time.monotonic()
                measurement = sample(pid, known, previous, now)
                if status['phase'] == 'measuring' and measurement['pids'] and not result_path.exists():
                    measurement.update({'wall_since_launch_seconds': now - launched,
                                        'status': status, 'gpu_system_wide': gpu_sample()})
                    samples.append(measurement)
            result = json_if_exists(result_path)
            if result:
                if 'error' in result:
                    raise RuntimeError(result['error'])
                if result['resolution'] != [1440, 900]:
                    raise RuntimeError(f'Unexpected framebuffer resolution: {result["resolution"]}')
                if len(result.get('frame_ms', [])) < 2:
                    raise RuntimeError('Native run produced too few frame intervals for a comparison.')
                result.update({'repetition': repetition, 'process_samples': samples,
                               'frame_summary': frame_summary(result['frame_ms']),
                               'window': window_info, 'launch_to_result_seconds': time.monotonic() - launched})
                result['manual_prompt_wait_seconds'] = manual_wait
                paint = result.get('cef_paint')
                if paint:
                    paint['callbacks_per_second'] = paint['callbacks'] / result['wall_seconds']
                    paint['mean_buffer_bytes'] = paint['copied_bytes'] / paint['callbacks'] if paint['callbacks'] else None
                result_path.write_text(json.dumps(result, indent=2) + '\n')
                print(f'  Median {result["frame_summary"]["median_ms"]:.2f} ms · p99 {result["frame_summary"]["p99_ms"]:.2f} ms', flush=True)
                return result
            if pid and not alive(pid):
                raise RuntimeError(f'Client exited without results. See {name}.log')
            time.sleep(1)
        raise RuntimeError(f'Run timed out. See {name}.log')
    finally:
        if pid:
            descendants(proc_table(), pid, known)
        stop(pid)
        # Do not overlap one run's helpers with the next run.
        deadline = time.monotonic() + 10
        while any(alive(p) and proc_table().get(p, {}).get('start') == start for p, start in known.items()):
            if time.monotonic() > deadline - 7:
                table = proc_table()
                for member, start in known.items():
                    if table.get(member, {}).get('start') == start:
                        stop(member)
            if time.monotonic() > deadline:
                raise RuntimeError('A benchmark process remained alive after cleanup; refusing to overlap runs.')
            time.sleep(.2)


def package_sizes(paths):
    def size(directory):
        return sum(p.stat().st_size for p in directory.rglob('*') if p.is_file())
    base = paths['baseline'] / 'space-client'
    cef = paths['cef'] / 'space-client'
    # Exclude the CEF SDK headers/static libs/tools/cache; count copied runtime files only.
    runtime = [p for p in paths['cef'].iterdir() if p.is_file() and
               (p.name in ['libcef.so', 'libEGL.so', 'libGLESv2.so', 'libvk_swiftshader.so', 'bevy_cef_render_process']
                or p.suffix in ['.so', '.pak', '.dat', '.bin', '.json'])]
    runtime_bytes = sum(p.stat().st_size for p in runtime)
    locales = paths['cef'] / 'locales'
    if locales.exists():
        runtime_bytes += size(locales)
    return {'baseline_binary_bytes': base.stat().st_size, 'cef_binary_bytes': cef.stat().st_size,
            'cef_runtime_bytes': runtime_bytes,
            'added_uncompressed_bytes': cef.stat().st_size + runtime_bytes - base.stat().st_size,
            'runtime_files': [p.name for p in runtime], 'sdk_distribution_not_counted': True,
            'compressed_package_bytes': None}


def aggregate(results):
    groups = {}
    for result in results:
        spec = result['spec']
        key = (spec['presentation'], spec['workload'], spec['mode'])
        groups.setdefault(key, []).append(result)
    rows = []
    for key, runs in groups.items():
        row = dict(zip(['presentation', 'workload', 'mode'], key))
        for metric in ['median_ms', 'p95_ms', 'p99_ms']:
            row[metric] = statistics.median(r['frame_summary'][metric] for r in runs)
        for metric in ['pss_mib', 'bevy_cpu_percent', 'helpers_cpu_percent', 'cpu_total_percent']:
            # Equal weight per run, not per frame or longest run.
            medians = [statistics.median(values) for r in runs
                       if (values := [s[metric] for s in r['process_samples'] if s.get(metric) is not None])]
            row[metric] = statistics.median(medians) if medians else None
        row['runs'] = len(runs)
        row['run_medians_ms'] = [r['frame_summary']['median_ms'] for r in runs]
        for metric in ['over_33_ms', 'over_50_ms', 'over_100_ms']:
            row[metric + '_per_second'] = statistics.median(r['frame_summary'][metric] / r['wall_seconds'] for r in runs)
        for metric in ['startup_scene_ready_seconds', 'startup_panel_ready_seconds']:
            values = [r[metric] for r in runs if r.get(metric) is not None]
            row[metric] = statistics.median(values) if values else None
        paints = [r['cef_paint'] for r in runs if r.get('cef_paint')]
        row['paint_callbacks_per_second'] = statistics.median(p['callbacks_per_second'] for p in paints) if paints else None
        rows.append(row)
    for row in rows:
        base = next((r for r in rows if r['mode'] == 'baseline' and r['presentation'] == row['presentation']
                     and r['workload'] == row['workload']), None)
        if base:
            for metric in ['median', 'p95', 'p99']:
                row[metric + '_delta_ms'] = row[metric + '_ms'] - base[metric + '_ms']
                row[metric + '_delta_percent'] = row[metric + '_delta_ms'] / base[metric + '_ms'] * 100
            row['pss_delta_mib'] = row['pss_mib'] - base['pss_mib'] if row['pss_mib'] is not None and base['pss_mib'] is not None else None
            row['cpu_delta_percentage_points'] = row['cpu_total_percent'] - base['cpu_total_percent'] if row['cpu_total_percent'] is not None and base['cpu_total_percent'] is not None else None
    return rows


def self_test():
    assert percentile([1, 2, 3], .5) == 2
    assert frame_summary([10, 40, 120])['over_100_ms'] == 1
    jobs = plan(3, False)
    assert len(jobs) == 60 and len(set(jobs)) == 60
    assert [j[3] for j in jobs[:5]] != [j[3] for j in jobs[20:25]]
    fake = {1: {'parent': 0, 'start': 10}, 2: {'parent': 1, 'start': 11}, 3: {'parent': 2, 'start': 12}}
    known = {}
    assert descendants(fake, 1, known) == {1, 2, 3}
    fake[3]['parent'] = 0
    assert descendants(fake, 1, known) == {1, 2, 3}
    process = sample(os.getpid(), {}, {}, time.monotonic())
    assert os.getpid() in process['pids'] and process['rss_mib'] > 0
    runs = []
    for mode, frames in [('baseline', [10, 10]), ('idle', [12, 12])]:
        runs.append({'spec': {'mode': mode, 'presentation': 'normal', 'workload': 'stationary'},
                     'frame_summary': frame_summary(frames), 'wall_seconds': 1,
                     'process_samples': [{'pss_mib': 100, 'bevy_cpu_percent': 50,
                                          'helpers_cpu_percent': 0, 'cpu_total_percent': 50}]})
    compared = aggregate(runs)
    assert compared[1]['median_delta_ms'] == 2 and compared[1]['median_delta_percent'] == 20
    print('Runner percentile, ordering and process-tree tests passed. No windows opened.')


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--repetitions', type=int, default=3)
    parser.add_argument('--warmup', type=float, default=15)
    parser.add_argument('--seconds', type=float, default=20)
    parser.add_argument('--interactive', action='store_true', help='Add manual typing/scroll runs with Enter prompts')
    parser.add_argument('--skip-build', action='store_true', help='Reuse existing benchmark release builds')
    parser.add_argument('--build-only', action='store_true', help='Build both release configurations without opening windows')
    parser.add_argument('--dry-run', action='store_true', help='Print run order, do not build or launch')
    parser.add_argument('--self-test', action='store_true')
    args = parser.parse_args()
    if args.self_test:
        self_test()
        return
    if not 1 <= args.repetitions <= 10 or not all(math.isfinite(v) and 3 <= v <= 120 for v in [args.warmup, args.seconds]):
        parser.error('Repetitions must be 1..10 and timings 3..120 seconds.')
    jobs = plan(args.repetitions, args.interactive)
    if args.dry_run:
        print(json.dumps(jobs, indent=2))
        print(f'{len(jobs)} runs, at least {len(jobs) * (args.warmup + args.seconds) / 60:.1f} minutes plus readiness/build time')
        return
    if args.interactive and not args.build_only and not sys.stdin.isatty():
        parser.error('--interactive needs a terminal for the Enter prompts.')
    env = dict(os.environ, CEF_PATH=str(ROOT / 'data/cef-sdk'), LD_LIBRARY_PATH=str(ROOT / 'data/cef-sdk'))
    if args.build_only:
        build(env)
        print('Both benchmark release builds are ready. No windows opened.')
        return
    monitor = check_monitor()
    paths = {name: ROOT / 'target/cef-benchmark' / name / 'release' for name in ['baseline', 'cef']}
    if not args.skip_build:
        paths = build(env)
    folder = ROOT / 'data/logs' / ('cef-benchmark-' + datetime.now(timezone.utc).strftime('%Y%m%dT%H%M%S-%fZ'))
    folder.mkdir(parents=True)
    metadata = {'commit': call(['git', 'rev-parse', 'HEAD']), 'branch': call(['git', 'branch', '--show-current']),
                'dirty': bool(call(['git', 'status', '--porcelain'])), 'kernel': call(['uname', '-sr']),
                'rustc': call(['rustc', '-Vv']),
                'graphics': {'physical': True, 'realistic': True, 'galaxy_glow': True, 'fov_degrees': 50,
                             'resolution': [1440, 900], 'winit_updates': 'continuous, even when unfocused'},
                'monitor': monitor, 'package_sizes': package_sizes(paths),
                'process_scope': 'Native client and descendants, including CEF helpers; fixed demo backend and runner excluded',
                'limitations': ['CPU 100% means one logical core; helper CPU is aggregated',
                               'GPU measurements are system-wide, not application-attributed',
                               'Missing PSS is null; summed RSS can double-count shared pages',
                               'Uncapped requests AutoNoVsync; compositor/driver can still constrain presentation',
                               'Active mode uses scripted DOM edits/scrolling plus 10 Hz Rust updates, not OS typing',
                               'Animation is a separate requestAnimationFrame stress workload',
                               'Main-loop frame intervals are not GPU execution times',
                               'Startup readiness is measured from native main/config load, excluding OS dynamic-loader time',
                               'Processes are cold starts; OS and CEF disk caches are not cleared between runs',
                               '1 Hz process sampling can miss short-lived helpers and straddle phase boundaries',
                               'GPU texture upload time and compressed distribution size are unavailable',
                               '--skip-build can reuse stale binaries; normal command rebuilds both'],
                'jobs': jobs}
    metadata['binary_sha256'] = {}
    for name, directory in paths.items():
        with (directory / 'space-client').open('rb') as binary:
            metadata['binary_sha256'][name] = hashlib.file_digest(binary, 'sha256').hexdigest()
    (folder / 'metadata.json').write_text(json.dumps(metadata, indent=2) + '\n')
    with (folder / 'server.log').open('w') as log:
        server = subprocess.Popen([str(paths['baseline'] / 'space-server')], cwd=ROOT, env=env, stdout=log, stderr=log)
        try:
            for _ in range(100):
                if server.poll() is not None:
                    raise RuntimeError('Benchmark server failed, possibly port 4321 already in use. Stop the existing dev server first.')
                import urllib.request
                try:
                    with urllib.request.urlopen('http://127.0.0.1:4321/health', timeout=1) as response:
                        if response.status == 200:
                            time.sleep(.1)
                            if server.poll() is not None:
                                raise RuntimeError('Our server exited; refusing to use another server on port 4321.')
                            break
                except OSError:
                    time.sleep(.1)
            else:
                raise RuntimeError('Server readiness timed out.')
            results = []
            print(f'Results: {folder}\nLeave workspace 8 visible. Automated runs do not take focus. Ctrl+C stops the suite.', flush=True)
            for number, job in enumerate(jobs, 1):
                results.append(run_job(folder, number, job, args, paths, monitor))
                summary = {'metadata': metadata, 'comparisons': aggregate(results),
                           'completed_runs': len(results), 'planned_runs': len(jobs),
                           'complete': len(results) == len(jobs)}
                (folder / 'summary.json').write_text(json.dumps(summary, indent=2) + '\n')
            print(f'Completed. Share {folder}/summary.json and the per-run result files.')
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()


if __name__ == '__main__':
    try:
        main()
    except (RuntimeError, subprocess.SubprocessError, KeyboardInterrupt) as error:
        print(f'Benchmark stopped: {error}', file=sys.stderr)
        sys.exit(1)
