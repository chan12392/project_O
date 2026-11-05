#!/usr/bin/env python3
"""Local OS observations only; this does not test Project O's process manager."""
import os
import pathlib
import select
import subprocess
import sys
import tempfile
import time


def wait_for(path, timeout=8):
    deadline = time.monotonic() + timeout
    while not path.exists():
        if time.monotonic() >= deadline:
            raise RuntimeError(f"timed out waiting for {path.name}")
        time.sleep(0.02)


def fixture(directory, inherited):
    base = pathlib.Path(directory)
    child = os.fork()
    if child == 0:
        try:
            os.setsid()
            sid = os.getsid(0)
            if not inherited:
                null = os.open(os.devnull, os.O_WRONLY)
                os.dup2(null, 1)
                os.dup2(null, 2)
                os.close(null)
            (base / "ready").write_text(f"{os.getpid()} {sid}\n", encoding="utf-8")
            time.sleep(3)
            (base / "finished").write_text("finished\n", encoding="utf-8")
        finally:
            os._exit(0)
    wait_for(base / "ready")
    print("leader-ready", flush=True)
    sys.stdin.readline()  # A cooperative stop request, never a signal to a PID.


def observe_escape(base, inherited):
    directory = base / ("inherited" if inherited else "closed")
    directory.mkdir()
    command = [sys.executable, str(pathlib.Path(__file__).resolve()), "--fixture", str(directory), str(int(inherited))]
    leader = subprocess.Popen(command, stdin=subprocess.PIPE, stdout=subprocess.PIPE,
                              stderr=subprocess.DEVNULL, start_new_session=True, env={})
    try:
        readable, _, _ = select.select([leader.stdout], [], [], 8)
        if not readable or leader.stdout.readline() != b"leader-ready\n":
            raise RuntimeError("fixture did not become ready")
        child, sid = map(int, (directory / "ready").read_text(encoding="utf-8").split())
        assert child == sid and sid != leader.pid, "setsid did not create a separate session"
        leader.stdin.write(b"stop\n")
        leader.stdin.flush()
        assert leader.wait(timeout=3) == 0
        assert not (directory / "finished").exists(), "descendant finished before the observation"
        readable, _, _ = select.select([leader.stdout], [], [], 0.2)
        if inherited:
            assert not readable, "inherited pipe unexpectedly closed"
        else:
            assert readable and os.read(leader.stdout.fileno(), 1) == b"", "pipe did not close"
        print(f"PASS: stopped direct child; setsid descendant still running, stdout {'open' if inherited else 'closed'}")
        wait_for(directory / "finished")
    finally:
        # Closing stdin also asks the directly created leader to exit on an error.
        if leader.stdin:
            leader.stdin.close()
        if leader.stdout:
            leader.stdout.close()
        try:
            leader.wait(timeout=9)
        except subprocess.TimeoutExpired:
            print("UNKNOWN: fixture did not exit; no process was signalled", file=sys.stderr)


def aliases(base, first, second, label):
    directory = base / label
    directory.mkdir()
    one, two = directory / first, directory / second
    one.write_text("probe", encoding="utf-8")
    try:
        same = os.path.samefile(one, two)
    except FileNotFoundError:
        same = False
    print(f"OBSERVED: {label} aliases on this volume: {same}")
    return same


def main():
    if len(sys.argv) == 4 and sys.argv[1] == "--fixture":
        fixture(sys.argv[2], sys.argv[3] == "1")
        return
    if sys.platform != "darwin":
        raise SystemExit("Run this probe on macOS; no process was started")
    base = pathlib.Path(tempfile.mkdtemp(prefix="o-macos-probe-"))
    print(f"Temporary data retained: {base}", flush=True)
    normal = subprocess.run([sys.executable, "-c", "print('normal-exit')"], capture_output=True, text=True, timeout=5, env={})
    assert normal.returncode == 0 and normal.stdout.strip() == "normal-exit"
    print("PASS: directly created child completed normally")
    observe_escape(base, True)
    observe_escape(base, False)
    aliases(base, "CaseName", "casename", "case")
    aliases(base, "\u00e9", "e\u0301", "unicode")
    print("OS observations only: run the separate Project O checks before claiming support")


if __name__ == "__main__":
    main()
