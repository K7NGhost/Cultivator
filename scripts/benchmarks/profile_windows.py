"""Sample a launched x64 Windows process using DbgHelp, without a kernel profiler.

Outputs folded stacks, an SVG flamegraph and a Speedscope profile. Samples select
threads whose CPU time advanced since the previous poll; they are observations,
not ETW CPU-time weights. Profiling timings must not be used as benchmark timings.
Child processes (including Python/GPU workers) are outside this native profile.
"""
import argparse
from collections import Counter
import ctypes as c
from ctypes import wintypes as w
import html
import json
from pathlib import Path
import struct
import subprocess
import time


class ThreadEntry(c.Structure):
    _fields_ = [(name, w.DWORD) for name in ("size", "usage", "tid", "pid")] + [
        ("base", w.LONG), ("delta", w.LONG), ("flags", w.DWORD)]


class Address(c.Structure):
    _fields_ = [("offset", c.c_uint64), ("segment", w.WORD), ("mode", c.c_int)]


class StackFrame(c.Structure):
    _fields_ = [(name, Address) for name in ("pc", "ret", "frame", "stack", "backing")] + [
        ("function", c.c_void_p), ("params", c.c_uint64 * 4), ("far", w.BOOL),
        ("virtual", w.BOOL), ("reserved", c.c_uint64 * 3), ("kdhelp", c.c_uint64 * 16)]


class Symbol(c.Structure):
    _fields_ = [("size", w.ULONG), ("type", w.ULONG), ("reserved", c.c_uint64 * 2),
                ("index", w.ULONG), ("bytes", w.ULONG), ("module", c.c_uint64),
                ("flags", w.ULONG), ("value", c.c_uint64), ("address", c.c_uint64),
                ("register", w.ULONG), ("scope", w.ULONG), ("tag", w.ULONG),
                ("name_len", w.ULONG), ("max_name", w.ULONG), ("name", c.c_char * 1)]


def api(dll, name, result, args):
    fn = getattr(dll, name)
    fn.restype, fn.argtypes = result, args
    return fn


def flamegraph(stacks, path, title):
    tree = {"n": 0, "children": {}}
    for frames, count in stacks.items():
        node = tree
        node["n"] += count
        for frame in frames:
            node = node["children"].setdefault(frame, {"n": 0, "children": {}})
            node["n"] += count
    depth = max((len(s) for s in stacks), default=1)
    width, row, height = 1600, 19, 80 + depth * 19
    parts = [f'<svg xmlns="http://www.w3.org/2000/svg" width="{width}" height="{height}" viewBox="0 0 {width} {height}">',
             '<style>text{font:11px monospace}g:hover rect{stroke:black;stroke-width:1}</style>',
             '<rect width="100%" height="100%" fill="#fafafa"/>',
             f'<text x="10" y="20">{html.escape(title)}</text>',
             f'<text x="10" y="40">{tree["n"]} active-thread stack observations; native process only; width = sample count</text>']

    def draw(node, x, level):
        for name, child in sorted(node["children"].items()):
            size = child["n"] / max(tree["n"], 1) * width
            y = height - 10 - (level + 1) * row
            color = 60 + sum(name.encode()) % 150
            parts.append(f'<g><title>{html.escape(name)}: {child["n"]} samples</title><rect x="{x:.3f}" y="{y}" width="{size:.3f}" height="18" fill="rgb(245,{color},80)"/>')
            if size > 25:
                label = name[:max(1, int(size / 6.7) - 1)]
                parts.append(f'<text x="{x + 2:.3f}" y="{y + 13}">{html.escape(label)}</text>')
            parts.append('</g>')
            draw(child, x, level + 1)
            x += size

    draw(tree, 0, 0)
    path.write_text("\n".join(parts + ['</svg>']), encoding="utf-8")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--seconds", type=float, default=60)
    parser.add_argument("--interval-ms", type=float, default=20)
    parser.add_argument("--pid", type=int, help="attach without stopping the process")
    parser.add_argument("--symbols", type=Path, help="local PDB search directory")
    parser.add_argument("command", nargs=argparse.REMAINDER)
    args = parser.parse_args()
    command = args.command[1:] if args.command[:1] == ['--'] else args.command
    if bool(command) == bool(args.pid):
        parser.error("pass a target executable after --, or --pid")
    args.output.parent.mkdir(parents=True, exist_ok=True)
    kernel = c.WinDLL('kernel32', use_last_error=True)
    dbg = c.WinDLL('dbghelp', use_last_error=True)
    close = api(kernel, 'CloseHandle', w.BOOL, [w.HANDLE])
    snapshot = api(kernel, 'CreateToolhelp32Snapshot', w.HANDLE, [w.DWORD, w.DWORD])
    first = api(kernel, 'Thread32First', w.BOOL, [w.HANDLE, c.POINTER(ThreadEntry)])
    next_thread = api(kernel, 'Thread32Next', w.BOOL, [w.HANDLE, c.POINTER(ThreadEntry)])
    open_process = api(kernel, 'OpenProcess', w.HANDLE, [w.DWORD, w.BOOL, w.DWORD])
    open_thread = api(kernel, 'OpenThread', w.HANDLE, [w.DWORD, w.BOOL, w.DWORD])
    suspend = api(kernel, 'SuspendThread', w.DWORD, [w.HANDLE])
    resume = api(kernel, 'ResumeThread', w.DWORD, [w.HANDLE])
    context = api(kernel, 'GetThreadContext', w.BOOL, [w.HANDLE, c.c_void_p])
    times = api(kernel, 'GetThreadTimes', w.BOOL, [w.HANDLE] + [c.POINTER(w.FILETIME)] * 4)
    exit_code = api(kernel, 'GetExitCodeProcess', w.BOOL, [w.HANDLE, c.POINTER(w.DWORD)])
    options = api(dbg, 'SymSetOptions', w.DWORD, [w.DWORD])
    initialize = api(dbg, 'SymInitializeW', w.BOOL, [w.HANDLE, w.LPCWSTR, w.BOOL])
    cleanup = api(dbg, 'SymCleanup', w.BOOL, [w.HANDLE])
    walk = api(dbg, 'StackWalk64', w.BOOL, [w.DWORD, w.HANDLE, w.HANDLE, c.POINTER(StackFrame), c.c_void_p] + [c.c_void_p] * 4)
    symbol = api(dbg, 'SymFromAddr', w.BOOL, [w.HANDLE, c.c_uint64, c.POINTER(c.c_uint64), c.c_void_p])
    table = c.cast(dbg.SymFunctionTableAccess64, c.c_void_p)
    module = c.cast(dbg.SymGetModuleBase64, c.c_void_p)
    samples, previous, handles, errors = [], {}, {}, Counter()
    log_path = args.output.with_suffix('.log')
    with log_path.open('w', encoding='utf-8') as log:
        class Attached:
            pid = args.pid

            def poll(self):
                code = w.DWORD()
                return None if exit_code(target, c.byref(code)) and code.value == 259 else 0

        process = Attached() if args.pid else subprocess.Popen(command, stdout=log, stderr=subprocess.STDOUT)
        # The Windows loader must publish the main module before DbgHelp invades.
        time.sleep(1)
        target = open_process(0x00100410, False, process.pid)
        if not target:
            raise c.WinError(c.get_last_error())
        options(0x00000002 | 0x00000004 | 0x00000200 | 0x00080000)  # undecorate, deferred, fail critical, no prompts
        try:
            search = str(args.symbols.resolve()) if args.symbols else (str(Path(command[0]).resolve().parent) if command else None)
            if not initialize(target, search, True):
                raise c.WinError(c.get_last_error())
            started = time.perf_counter()
            while process.poll() is None and time.perf_counter() - started < args.seconds:
                cycle = time.perf_counter()
                snap = snapshot(4, 0)
                if snap == c.c_void_p(-1).value:
                    raise c.WinError(c.get_last_error())
                try:
                    entry = ThreadEntry()
                    entry.size = c.sizeof(entry)
                    valid = first(snap, c.byref(entry))
                    while valid:
                        if entry.pid == process.pid and entry.tid not in handles:
                            handle = open_thread(0x004A, False, entry.tid)
                            if handle:
                                handles[entry.tid] = handle
                        valid = next_thread(snap, c.byref(entry))
                finally:
                    close(snap)
                for tid, handle in list(handles.items()):
                    values = [w.FILETIME() for _ in range(4)]
                    if not times(handle, *(c.byref(v) for v in values)):
                        continue
                    cpu = sum((v.dwHighDateTime << 32) | v.dwLowDateTime for v in values[2:])
                    last = previous.get(tid, cpu)
                    previous[tid] = cpu
                    if cpu <= last:
                        continue
                    storage = c.create_string_buffer(1232 + 16)
                    address = (c.addressof(storage) + 15) & ~15
                    c.c_uint32.from_address(address + 48).value = 0x0010000B
                    if suspend(handle) == 0xffffffff:
                        errors['suspend'] += 1
                        continue
                    stack = []
                    try:
                        if not context(handle, address):
                            errors['context'] += 1
                            continue
                        frame = StackFrame()
                        frame.pc.offset = c.c_uint64.from_address(address + 248).value
                        frame.stack.offset = c.c_uint64.from_address(address + 152).value
                        frame.frame.offset = c.c_uint64.from_address(address + 160).value
                        frame.pc.mode = frame.stack.mode = frame.frame.mode = 3
                        seen = set()
                        for _ in range(96):
                            if not walk(0x8664, target, handle, c.byref(frame), address, None, table, module, None):
                                break
                            pc = frame.pc.offset
                            state = (pc, frame.stack.offset)
                            if not pc or state in seen:
                                break
                            seen.add(state)
                            stack.append(pc)
                    finally:
                        resume(handle)
                    if stack:
                        samples.append((tid, stack))
                time.sleep(max(0, args.interval_ms / 1000 - (time.perf_counter() - cycle)))
            sampled_seconds = time.perf_counter() - started
            names = {}
            for pc in {pc for _, stack in samples for pc in stack}:
                storage = c.create_string_buffer(c.sizeof(Symbol) + 4096)
                info = Symbol.from_buffer(storage)
                info.size, info.max_name = c.sizeof(Symbol), 4095
                displacement = c.c_uint64()
                if symbol(target, pc, c.byref(displacement), storage):
                    name = c.string_at(c.addressof(storage) + Symbol.name.offset, info.name_len).decode('utf-8', errors='replace')
                else:
                    name = f'0x{pc:x}'
                names[pc] = name.replace(';', ':').replace('\n', ' ')
        finally:
            for handle in handles.values():
                close(handle)
            cleanup(target)
            close(target)
        folded = Counter(tuple(reversed([names[pc] for pc in stack])) for _, stack in samples)
        args.output.with_suffix('.folded').write_text(''.join(';'.join(frames) + f' {n}\n' for frames, n in folded.items()), encoding='utf-8')
        flamegraph(folded, args.output.with_suffix('.svg'), args.output.name)
        frames = list(dict.fromkeys(name for stack in folded for name in stack))
        indexes = {name:i for i,name in enumerate(frames)}
        speedscope = {'$schema':'https://www.speedscope.app/file-format-schema.json', 'shared':{'frames':[{'name':name} for name in frames]}, 'profiles':[{'type':'sampled','name':args.output.name,'unit':'none','startValue':0,'endValue':len(samples),'samples':[[indexes[name] for name in stack] for stack,n in folded.items()], 'weights':list(folded.values())}], 'activeProfileIndex':0, 'exporter':'Cultivator Windows active-thread sampler'}
        args.output.with_suffix('.speedscope.json').write_text(json.dumps(speedscope), encoding='utf-8')
        inclusive = Counter()
        for stack, n in folded.items():
            inclusive.update({frame:n for frame in set(stack)})
        metadata = {'command':command,'pid':process.pid,'sampledSeconds':sampled_seconds,'samples':len(samples),'threads':len(handles),'errors':dict(errors),'inclusiveTopFrames':inclusive.most_common(40),'method':'CPU-active thread polling with DbgHelp stacks; equal observation weights; native process only'}
        args.output.with_suffix('.json').write_text(json.dumps(metadata,indent=2)+'\n',encoding='utf-8')
        print(json.dumps(metadata,indent=2), flush=True)
        code = 0 if args.pid else process.wait()
        print(f'Target exit code: {code}; log: {log_path}', flush=True)
        raise SystemExit(code)


if __name__ == '__main__':
    main()
