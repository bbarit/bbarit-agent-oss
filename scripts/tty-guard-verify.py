#!/usr/bin/env python3
"""Regression check for the tui.rs tty_guard module (macOS/Linux, needs pty + zsh).

Symptom: in long sessions, a spawned child (or an orphan it left behind) makes
itself the terminal's foreground process group and exits; the TUI's next tty
read then stops the whole process with SIGTTIN, dropping the user back to the
shell with mouse tracking / alternate screen still on — every wheel tick spews
"65;91;14M"-style SGR mouse reports into the prompt.

Usage:
  cargo build && python3 scripts/tty-guard-verify.py

Test A: SIGTSTP while running as a zsh job → the restore sequence reaches the
        tty before the stop, the process is actually stopped (T), and `fg`
        re-enters the TUI.
Test B: a background thief in the same session steals the foreground pgrp via
        tcsetpgrp and dies → the agent reclaims the tty and keeps running even
        when input arrives (no SIGTTIN stop, no "suspended" message).

Pitfall: spawning the agent directly (start_new_session) makes its pgrp
orphaned, and POSIX discards stop signals for orphaned pgrps — the tests must
run the agent as a zsh job to reproduce anything.
"""
import os, pty, sys, time, signal, subprocess, select, tempfile

REPO = os.path.dirname(os.path.dirname(os.path.abspath(__file__)))
AGENT = os.path.join(REPO, "target", "debug", "bbarit-oss")
RESTORE_MARKS = [b"\x1b[?1006l", b"\x1b[?1003l", b"\x1b[?1049l"]

THIEF_SRC = """
import os, time, signal, ctypes
# Background job (own pgrp): wait 5s, steal the tty foreground, die instantly —
# reproduces the "child stole the tty and exited" real-world scenario.
time.sleep(5)
libc = ctypes.CDLL(None)
signal.signal(signal.SIGTTOU, signal.SIG_IGN)
fd = os.open("/dev/tty", os.O_RDWR)
libc.tcsetpgrp(fd, os.getpgrp())
"""

def drain(fd, seconds):
    buf = b""
    end = time.time() + seconds
    while time.time() < end:
        try:
            r, _, _ = select.select([fd], [], [], 0.2)
            if r:
                chunk = os.read(fd, 65536)
                if not chunk:
                    break
                buf += chunk
        except OSError:
            break
    return buf

def proc_state(pid):
    return subprocess.run(["ps", "-o", "state=", "-p", str(pid)],
                          capture_output=True, text=True).stdout.strip()

def spawn_zsh():
    pid, master = pty.fork()
    if pid == 0:
        os.environ["TERM"] = "xterm-256color"
        os.execv("/bin/zsh", ["zsh", "-f", "-i"])
    drain(master, 1.5)
    return pid, master

def find_agent(zsh_pid):
    out = subprocess.run(["pgrep", "-P", str(zsh_pid), "-f", "bbarit-oss"],
                         capture_output=True, text=True).stdout.split()
    return int(out[0]) if out else None

def cleanup(master, *pids):
    for p in pids:
        try:
            os.kill(p, signal.SIGKILL)
        except (ProcessLookupError, PermissionError):
            pass
    os.close(master)

def test_a():
    zsh_pid, master = spawn_zsh()
    os.write(master, f"{AGENT}\n".encode())
    boot = drain(master, 5.0)
    if b"\x1b[?1049h" not in boot:
        print("A0 tui-boot:                FAIL"); cleanup(master, zsh_pid); return False
    agent_pid = find_agent(zsh_pid)
    if not agent_pid:
        print("A0 agent-not-found:         FAIL"); cleanup(master, zsh_pid); return False
    os.kill(agent_pid, signal.SIGTSTP)
    stopped_out = drain(master, 2.0)
    state = proc_state(agent_pid)
    ok_restore = all(m in stopped_out for m in RESTORE_MARKS)
    ok_stopped = state.startswith("T")
    ok_prompt = b"suspended" in stopped_out
    print(f"A1 restore-seq-before-stop: {'PASS' if ok_restore else 'FAIL'}")
    print(f"A2 actually-stopped:        {'PASS' if ok_stopped else 'FAIL'} (state={state})")
    print(f"A3 zsh-prompt-back:         {'PASS' if ok_prompt else 'FAIL'}")
    os.write(master, b"fg\n")
    resumed = drain(master, 3.0)
    ok_resume = b"\x1b[?1049h" in resumed and b"\x1b[?1006h" in resumed
    print(f"A4 fg-reenters-tui:         {'PASS' if ok_resume else 'FAIL'}")
    cleanup(master, agent_pid, zsh_pid)
    return ok_restore and ok_stopped and ok_prompt and ok_resume

def test_b():
    thief = tempfile.NamedTemporaryFile("w", suffix=".py", delete=False)
    thief.write(THIEF_SRC); thief.close()
    zsh_pid, master = spawn_zsh()
    os.write(master, f"python3 {thief.name} &\n".encode())
    drain(master, 0.5)
    os.write(master, f"{AGENT}\n".encode())
    drain(master, 5.0)  # TUI boot (the thief fires 5s in)
    agent_pid = find_agent(zsh_pid)
    if not agent_pid:
        print("B0 agent-not-found:         FAIL"); cleanup(master, zsh_pid); return False
    drain(master, 4.0)  # wait for the thief to fire and die
    time.sleep(1.0)
    # SIGTTIN fires the moment input arrives and read() runs — force a read
    # by sending a key; the state AFTER that is the real verdict.
    os.write(master, b"x")
    after = drain(master, 2.0)
    state = proc_state(agent_pid)
    ok_alive = bool(state) and not state.startswith("T")
    ok_no_suspend = b"suspended" not in after
    print(f"B1 no-sigttin-stop:         {'PASS' if ok_alive else 'FAIL'} (state={state})")
    print(f"B2 no-suspend-message:      {'PASS' if ok_no_suspend else 'FAIL'}")
    cleanup(master, agent_pid, zsh_pid)
    os.unlink(thief.name)
    return ok_alive and ok_no_suspend

if __name__ == "__main__":
    if not os.path.exists(AGENT):
        sys.exit(f"Build first: cargo build ({AGENT} missing)")
    which = sys.argv[1] if len(sys.argv) > 1 else "all"
    ok = True
    if which in ("a", "all"):
        ok &= test_a()
    if which in ("b", "all"):
        ok &= test_b()
    print("RESULT:", "PASS" if ok else "FAIL")
    sys.exit(0 if ok else 1)
