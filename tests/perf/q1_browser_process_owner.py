#!/usr/bin/env python3
"""Shared fail-closed process ownership for formal Q1 browser producers."""

from __future__ import annotations

import hashlib
import json
import os
import pathlib
import signal
import subprocess
import sys
import tempfile
import threading
import time
from dataclasses import dataclass
from typing import Any


PROCESS_GROUP_TERM_GRACE_SECONDS = 5
PROCESS_GROUP_KILL_GRACE_SECONDS = 5


class ProcessOwnershipError(RuntimeError):
    """A finite process-launch or ownership-proof failure."""


def _require(condition: bool, message: str) -> None:
    if not condition:
        raise ProcessOwnershipError(message)


def _load_object(path: pathlib.Path, label: str) -> dict[str, Any]:
    try:
        value = json.loads(path.read_text(encoding="utf-8"))
    except (OSError, json.JSONDecodeError) as error:
        raise ProcessOwnershipError(f"cannot read {label}: {error}") from error
    _require(isinstance(value, dict), f"{label} must contain an object")
    return value


class ProcessTimeoutError(ProcessOwnershipError):
    """A timed-out isolated process group and its terminal cleanup receipt."""

    def __init__(self, message: str, outcome: "ProcessOutcome") -> None:
        super().__init__(message)
        self.outcome = outcome


class ProcessTreeError(ProcessOwnershipError):
    """A normal exit that left an unproven or surviving descendant tree."""

    def __init__(self, message: str, outcome: "ProcessOutcome") -> None:
        super().__init__(message)
        self.outcome = outcome


@dataclass(frozen=True)
class ProcessOutcome:
    argv: list[str]
    returncode: int
    stdout: str
    stderr: str
    timed_out: bool
    timeout_seconds: int
    cleanup: dict[str, Any]


@dataclass(frozen=True)
class ProcessIdentity:
    pid: int
    ppid: int
    pgid: int
    started: str
    command: str

    def receipt(self) -> dict[str, Any]:
        return {
            "pid": self.pid,
            "ppid": self.ppid,
            "pgid": self.pgid,
            "started": self.started,
            "command": self.command,
        }


def same_process(observed: ProcessIdentity | None, expected: ProcessIdentity) -> bool:
    """Match PID reuse safely while allowing PPID/PGID changes after detach."""

    return (
        observed is not None
        and observed.pid == expected.pid
        and observed.started == expected.started
    )


def process_table_snapshot() -> dict[int, ProcessIdentity]:
    """Read a bounded macOS/Linux ps snapshot without recursing into the runner."""

    process = subprocess.Popen(
        ["/bin/ps", "-axo", "pid=,ppid=,pgid=,lstart=,command="],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
    )
    try:
        stdout, stderr = process.communicate(timeout=5)
    except subprocess.TimeoutExpired as error:
        process.kill()
        process.communicate()
        raise ProcessOwnershipError("process-table snapshot exceeded 5 seconds") from error
    _require(process.returncode == 0, stderr.strip() or "process-table snapshot failed")
    result = {}
    for line in stdout.splitlines():
        fields = line.split(None, 8)
        if len(fields) < 9:
            continue
        try:
            pid, ppid, pgid = map(int, fields[:3])
        except ValueError:
            continue
        result[pid] = ProcessIdentity(
            pid, ppid, pgid, " ".join(fields[3:8]), fields[8]
        )
    return result


def marker_processes(
    snapshot: dict[int, ProcessIdentity], marker_argument: str
) -> list[ProcessIdentity]:
    return [
        identity
        for identity in snapshot.values()
        if marker_argument in identity.command.split()
    ]


def require_process_table_commandlines() -> None:
    _require(
        sys.platform.startswith(("darwin", "linux")),
        "formal browser ownership requires macOS or Linux ps command lines",
    )
    snapshot = process_table_snapshot()
    current = snapshot.get(os.getpid())
    _require(
        current is not None and bool(current.command),
        "formal browser ownership cannot observe process command lines",
    )


class ProcessTreeTracker:
    """Poll and retain descendant identities even after they reparent/detach."""

    def __init__(self, leader: ProcessIdentity) -> None:
        self.leader = leader
        self.known: dict[int, ProcessIdentity] = {leader.pid: leader}
        self.snapshot_count = 1
        self.errors: list[str] = []
        self._lock = threading.Lock()
        self._stop = threading.Event()
        self._thread = threading.Thread(target=self._loop, daemon=True)

    def start(self) -> None:
        self._thread.start()

    def _absorb(self, snapshot: dict[int, ProcessIdentity]) -> None:
        with self._lock:
            self.snapshot_count += 1
            for pid, known in list(self.known.items()):
                observed = snapshot.get(pid)
                if same_process(observed, known):
                    self.known[pid] = observed
            changed = True
            while changed:
                changed = False
                parents = {
                    pid
                    for pid, identity in self.known.items()
                    if same_process(snapshot.get(pid), identity)
                }
                owned_groups = {identity.pgid for identity in self.known.values()}
                for identity in snapshot.values():
                    if identity.pid not in self.known and (
                        identity.ppid in parents or identity.pgid in owned_groups
                    ):
                        self.known[identity.pid] = identity
                        changed = True

    def capture(self) -> dict[int, ProcessIdentity]:
        snapshot = process_table_snapshot()
        self._absorb(snapshot)
        return snapshot

    def _loop(self) -> None:
        while not self._stop.wait(0.05):
            try:
                self.capture()
            except BaseException as error:
                with self._lock:
                    self.errors.append(str(error))
                return

    def known_identities(self) -> dict[int, ProcessIdentity]:
        with self._lock:
            return dict(self.known)

    def stop(self) -> bool:
        self._stop.set()
        self._thread.join(timeout=6)
        return not self._thread.is_alive()


def signal_verified_identities(
    identities: list[ProcessIdentity],
    signal_number: signal.Signals,
) -> dict[str, Any]:
    """Signal identities verified by the immediately preceding ps snapshot."""

    own_pid = os.getpid()
    own_pgid = os.getpgrp()
    groups = sorted({
        identity.pgid
        for identity in identities
        if identity.pid == identity.pgid
        and identity.pid != own_pid
        and identity.pgid != own_pgid
    })
    group_members = {identity.pid for identity in identities if identity.pgid in groups}
    pids = sorted({
        identity.pid for identity in identities
        if identity.pid not in group_members and identity.pid != own_pid
    })
    signaled_groups = []
    signaled_pids = []
    for process_group_id in groups:
        try:
            os.killpg(process_group_id, signal_number)
            signaled_groups.append(process_group_id)
        except OSError:
            pass
    for pid in pids:
        try:
            os.kill(pid, signal_number)
            signaled_pids.append(pid)
        except OSError:
            pass
    return {"signal": signal_number.name, "groups": signaled_groups, "pids": signaled_pids}


def validate_browser_handshake(
    ownership: dict[str, str],
    producer_pid: int,
    known: dict[int, ProcessIdentity],
    tracker_owned: dict[int, ProcessIdentity],
) -> dict[str, Any]:
    handshake = _load_object(
        pathlib.Path(ownership["handshake_path"]), "browser ownership handshake"
    )
    expected = {
        "schema": "gsplat-q1-browser-process-ownership/v1",
        "marker": ownership["marker"],
        "marker_arg": ownership["marker_argument"],
        "user_data_dir": ownership["user_data_dir"],
        "producer_pid": producer_pid,
    }
    for key, value in expected.items():
        _require(handshake.get(key) == value, f"browser ownership handshake has wrong {key}")
    browser_pid = handshake.get("browser_pid")
    _require(
        isinstance(browser_pid, int) and browser_pid > 0 and browser_pid != producer_pid,
        "browser ownership handshake has invalid browser_pid",
    )
    identity = known.get(browser_pid)
    _require(identity is not None, "browser ownership handshake PID was never observed")

    spawnfile = handshake.get("browser_spawnfile")
    spawnargs = handshake.get("browser_spawnargs")
    marker_argument = ownership["marker_argument"]
    _require(
        isinstance(spawnfile, str)
        and pathlib.Path(spawnfile).resolve()
        == pathlib.Path(ownership["expected_executable"]).resolve()
        and isinstance(spawnargs, list)
        and bool(spawnargs)
        and all(isinstance(argument, str) for argument in spawnargs)
        and spawnargs[0] == spawnfile
        and spawnargs.count(marker_argument) == 1
        and [
            argument for argument in spawnargs if argument.startswith("--user-data-dir=")
        ]
        == [marker_argument],
        "browser ownership handshake has invalid spawn identity",
    )
    marker_identities = [
        candidate
        for candidate in tracker_owned.values()
        if marker_argument in candidate.command.split()
    ]
    direct_marker = marker_argument in identity.command.split()
    owned_identity = tracker_owned.get(browser_pid)
    _require(
        direct_marker
        or any(
            owned_identity is not None
            and same_process(identity, owned_identity)
            and identity.ppid == producer_pid
            and identity.pid == identity.pgid
            and candidate.ppid == browser_pid
            and candidate.pgid == browser_pid
            for candidate in marker_identities
        ),
        "browser ownership handshake PID lacks a related exact-marker process",
    )
    return handshake


def run_process_group(
    argv: list[str],
    *,
    cwd: pathlib.Path,
    env: dict[str, str],
    timeout_seconds: int,
    browser_ownership: dict[str, str] | None = None,
    term_grace_seconds: int | None = None,
    kill_grace_seconds: int | None = None,
) -> ProcessOutcome:
    """Run one command and clean its observed tree plus declared browser owner."""

    _require(timeout_seconds > 0, "process timeout must be positive")
    term_grace = (
        PROCESS_GROUP_TERM_GRACE_SECONDS
        if term_grace_seconds is None
        else term_grace_seconds
    )
    kill_grace = (
        PROCESS_GROUP_KILL_GRACE_SECONDS
        if kill_grace_seconds is None
        else kill_grace_seconds
    )
    _require(term_grace > 0, "process TERM grace must be positive")
    _require(kill_grace > 0, "process KILL grace must be positive")
    if browser_ownership is not None:
        marker_argument = browser_ownership["marker_argument"]
        _require(
            not marker_processes(process_table_snapshot(), marker_argument),
            "browser ownership marker already belongs to a live process",
        )
        _require(
            not pathlib.Path(browser_ownership["handshake_path"]).exists(),
            "browser ownership handshake path already exists",
        )
        _require(
            not pathlib.Path(browser_ownership["user_data_dir"]).exists(),
            "browser ownership user-data-dir already exists",
        )
    launcher = (
        "import os,signal,sys;"
        "os.kill(os.getpid(),signal.SIGSTOP);"
        "os.execvpe(sys.argv[1],sys.argv[1:],os.environ)"
    )
    stdout_file = tempfile.TemporaryFile(mode="w+t", encoding="utf-8")
    stderr_file = tempfile.TemporaryFile(mode="w+t", encoding="utf-8")
    try:
        process = subprocess.Popen(
            [sys.executable, "-c", launcher, *argv],
            cwd=cwd,
            env=env,
            stdout=stdout_file,
            stderr=stderr_file,
            text=True,
            start_new_session=True,
        )
    except BaseException:
        stdout_file.close()
        stderr_file.close()
        raise
    initial = ProcessIdentity(
        process.pid, os.getpid(), process.pid, "unobserved", "stopped-launcher"
    )
    tracker: ProcessTreeTracker | None = None
    continued = False
    timed_out = False
    stdout = ""
    stderr = ""
    runner_errors: list[str] = []
    term_receipt = {"signal": "SIGTERM", "groups": [], "pids": []}
    kill_receipt = {"signal": "SIGKILL", "groups": [], "pids": []}
    descendants_before: list[ProcessIdentity] = []
    final_survivors: list[ProcessIdentity] = []
    surviving_groups: list[int] = []
    handshake: dict[str, Any] | None = None
    try:
        try:
            waited_pid, wait_status = os.waitpid(process.pid, os.WUNTRACED)
            _require(
                waited_pid == process.pid and os.WIFSTOPPED(wait_status),
                "process launcher did not stop before exec",
            )
            observed = process_table_snapshot().get(process.pid)
            _require(observed is not None, "process leader identity was not observable before exec")
            initial = observed
            tracker = ProcessTreeTracker(initial)
            tracker.start()
            os.kill(process.pid, signal.SIGCONT)
            continued = True
            try:
                process.wait(timeout=timeout_seconds)
            except subprocess.TimeoutExpired:
                timed_out = True
        except BaseException as error:
            runner_errors.append(f"{type(error).__name__}: {error}")
    finally:
        # This block begins at spawn ownership, so even waitpid/ps/tracker
        # failures cannot strand the deliberately stopped launcher.
        if not continued and process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
                kill_receipt["groups"].append(process.pid)
            except OSError as error:
                runner_errors.append(f"initial KILL: {type(error).__name__}: {error}")
                pass
        tracker_owned: dict[int, ProcessIdentity] = {initial.pid: initial}
        if tracker is not None:
            tracker_owned.update(tracker.known_identities())
        known = dict(tracker_owned)
        snapshot_failures = 0
        convergence_rounds = 0
        clean_snapshots = 0
        final_snapshot: dict[int, ProcessIdentity] = {}
        final_snapshot_available = False
        term_seen: set[tuple[int, str]] = set()
        cleanup_deadline = time.monotonic() + term_grace + kill_grace
        while convergence_rounds < 64 and time.monotonic() < cleanup_deadline:
            convergence_rounds += 1
            try:
                snapshot = process_table_snapshot()
                final_snapshot = snapshot
                final_snapshot_available = True
                if tracker is not None:
                    tracker._absorb(snapshot)
                    tracker_owned.update(tracker.known_identities())
                    known.update(tracker_owned)
                marker_live = [] if browser_ownership is None else marker_processes(
                    snapshot, browser_ownership["marker_argument"]
                )
                known.update({identity.pid: identity for identity in marker_live})
                if browser_ownership is not None and handshake is None:
                    try:
                        handshake = validate_browser_handshake(
                            browser_ownership, process.pid, known, tracker_owned
                        )
                    except BaseException as error:
                        message = f"browser handshake: {type(error).__name__}: {error}"
                        if message not in runner_errors:
                            runner_errors.append(message)
                live = [
                    identity
                    for identity in known.values()
                    if same_process(snapshot.get(identity.pid), identity)
                ]
                newly_seen = [
                    identity
                    for identity in live
                    if identity.pid != process.pid
                    and identity.pid not in {value.pid for value in descendants_before}
                ]
                descendants_before.extend(newly_seen)
                initial_group_live = any(
                    identity.pgid == initial.pgid for identity in snapshot.values()
                )
                if not live and not initial_group_live:
                    clean_snapshots += 1
                    if clean_snapshots >= 2:
                        break
                else:
                    clean_snapshots = 0
                term_targets = [
                    identity
                    for identity in live
                    if (identity.pid, identity.started) not in term_seen
                ]
                kill_targets = [
                    identity
                    for identity in live
                    if (identity.pid, identity.started) in term_seen
                ]
                if initial_group_live and (initial.pid, initial.started) not in term_seen:
                    term_targets.append(initial)
                elif initial_group_live:
                    kill_targets.append(initial)
                if term_targets:
                    extra = signal_verified_identities(term_targets, signal.SIGTERM)
                    term_receipt["groups"] = sorted(set(term_receipt["groups"] + extra["groups"]))
                    term_receipt["pids"] = sorted(set(term_receipt["pids"] + extra["pids"]))
                    term_seen.update((value.pid, value.started) for value in term_targets)
                if kill_targets:
                    extra = signal_verified_identities(kill_targets, signal.SIGKILL)
                    kill_receipt["groups"] = sorted(set(kill_receipt["groups"] + extra["groups"]))
                    kill_receipt["pids"] = sorted(set(kill_receipt["pids"] + extra["pids"]))
            except BaseException as error:
                final_snapshot_available = False
                snapshot_failures += 1
                runner_errors.append(
                    f"cleanup snapshot {convergence_rounds}: {type(error).__name__}: {error}"
                )
                # The initial PGID is owned from spawn and needs no ps identity.
                try:
                    selected_signal = signal.SIGTERM if convergence_rounds == 1 else signal.SIGKILL
                    os.killpg(process.pid, selected_signal)
                    receipt = term_receipt if selected_signal == signal.SIGTERM else kill_receipt
                    receipt["groups"] = sorted(set(receipt["groups"] + [process.pid]))
                except OSError:
                    pass
            time.sleep(0.05)

        if process.poll() is None:
            try:
                os.killpg(process.pid, signal.SIGKILL)
                kill_receipt["groups"] = sorted(set(kill_receipt["groups"] + [process.pid]))
            except OSError:
                process.kill()
            try:
                process.wait(timeout=kill_grace)
            except subprocess.TimeoutExpired as error:
                runner_errors.append(f"leader reap timeout: {error}")
        if tracker is not None:
            if not tracker.stop():
                runner_errors.append("process tracker did not stop within 6 seconds")
            known.update(tracker.known_identities())
        # The post-tracker rescan is itself convergent: an owner first observed
        # here is signaled and rescanned instead of merely being reported.
        post_stop_clean = 0
        for post_stop_round in range(1, 9):
            try:
                final_snapshot = process_table_snapshot()
                final_snapshot_available = True
                marker_live = [] if browser_ownership is None else marker_processes(
                    final_snapshot, browser_ownership["marker_argument"]
                )
                known.update({identity.pid: identity for identity in marker_live})
                live = [
                    identity
                    for identity in known.values()
                    if same_process(final_snapshot.get(identity.pid), identity)
                ]
                if not live:
                    post_stop_clean += 1
                    if post_stop_clean >= 2:
                        break
                else:
                    post_stop_clean = 0
                    term_targets = [
                        identity
                        for identity in live
                        if (identity.pid, identity.started) not in term_seen
                    ]
                    kill_targets = [
                        identity
                        for identity in live
                        if (identity.pid, identity.started) in term_seen
                    ]
                    if term_targets:
                        extra = signal_verified_identities(term_targets, signal.SIGTERM)
                        term_receipt["groups"] = sorted(set(term_receipt["groups"] + extra["groups"]))
                        term_receipt["pids"] = sorted(set(term_receipt["pids"] + extra["pids"]))
                        term_seen.update((value.pid, value.started) for value in term_targets)
                    if kill_targets:
                        extra = signal_verified_identities(kill_targets, signal.SIGKILL)
                        kill_receipt["groups"] = sorted(set(kill_receipt["groups"] + extra["groups"]))
                        kill_receipt["pids"] = sorted(set(kill_receipt["pids"] + extra["pids"]))
            except BaseException as error:
                final_snapshot_available = False
                runner_errors.append(
                    f"post-stop snapshot {post_stop_round}: {type(error).__name__}: {error}"
                )
            time.sleep(0.05)
        final_survivors = [] if not final_snapshot_available else [
            identity
            for identity in known.values()
            if same_process(final_snapshot.get(identity.pid), identity)
        ]
        if browser_ownership is not None and final_snapshot_available:
            final_survivors.extend(
                identity
                for identity in marker_processes(
                    final_snapshot, browser_ownership["marker_argument"]
                )
                if identity.pid not in {value.pid for value in final_survivors}
            )
        owned_groups = {
            identity.pgid for identity in known.values() if identity.pid == identity.pgid
        }
        surviving_groups = [] if not final_snapshot_available else sorted({
            identity.pgid
            for identity in final_snapshot.values()
            if identity.pgid in owned_groups
        })
        stdout_file.flush()
        stderr_file.flush()
        stdout_file.seek(0)
        stderr_file.seek(0)
        stdout = stdout_file.read()
        stderr = stderr_file.read()
        stdout_file.close()
        stderr_file.close()
    cleanup = {
        "isolated_process_group": initial.pgid == initial.pid and initial.pgid != os.getpgrp(),
        "process_group_id": initial.pgid,
        "ownership_scope": "marker_handshake" if browser_ownership else "initial_group_and_observed_descendants",
        "whole_system_lineage_claimed": False,
        "lineage_complete": not runner_errors and not (tracker.errors if tracker else []),
        "snapshot_count": tracker.snapshot_count if tracker else 0,
        "known_processes": [identity.receipt() for identity in sorted(known.values(), key=lambda item: item.pid)],
        "detached_process_groups": sorted({
            identity.pgid
            for identity in known.values()
            if identity.pid != process.pid and identity.pgid != initial.pgid
        }),
        "orphan_descendants_detected": bool(descendants_before),
        "term": term_receipt,
        "kill": kill_receipt,
        "leader_reaped": process.poll() is not None,
        "survivors": [identity.receipt() for identity in final_survivors],
        "surviving_process_groups": surviving_groups,
        "group_gone": final_snapshot_available and not final_survivors and not surviving_groups,
        "term_grace_seconds": term_grace,
        "kill_grace_seconds": kill_grace,
        "tracker_errors": tracker.errors if tracker else [],
        "runner_errors": runner_errors,
        "cleanup_convergence_rounds": convergence_rounds,
        "cleanup_snapshot_failures": snapshot_failures,
        "final_snapshot_available": final_snapshot_available,
        "browser_ownership": None if browser_ownership is None else {
            **browser_ownership,
            "handshake_verified": handshake is not None,
            "handshake": handshake,
            "marker_processes_final": [
                identity.receipt()
                for identity in marker_processes(
                    final_snapshot, browser_ownership["marker_argument"]
                )
            ],
        },
    }
    return ProcessOutcome(
        argv=list(argv),
        returncode=process.returncode,
        stdout=stdout,
        stderr=stderr,
        timed_out=timed_out,
        timeout_seconds=timeout_seconds,
        cleanup=cleanup,
    )


def require_process_completed(outcome: ProcessOutcome, context: str) -> None:
    if outcome.timed_out:
        raise ProcessTimeoutError(
            f"{context} exceeded its {outcome.timeout_seconds} second safety timeout; "
            f"process-group cleanup={json.dumps(outcome.cleanup, sort_keys=True)}",
            outcome,
        )
    if (
        not outcome.cleanup.get("isolated_process_group")
        or not outcome.cleanup.get("lineage_complete")
        or not outcome.cleanup.get("group_gone")
        or outcome.cleanup.get("orphan_descendants_detected")
    ):
        raise ProcessTreeError(
            f"{context} left or created an unqualified descendant process tree; "
            f"cleanup={json.dumps(outcome.cleanup, sort_keys=True)}",
            outcome,
        )


def browser_ownership(
    root: pathlib.Path, invocation_id: str, expected_executable: pathlib.Path
) -> dict[str, Any]:
    digest = hashlib.sha256(f"{root}:{invocation_id}".encode()).hexdigest()[:32]
    marker = f"gsplat-q1-{digest}"
    user_data_dir = root / "process-home" / "browser-profiles" / invocation_id
    handshake_path = root / "browser-handshakes" / f"{invocation_id}.json"
    return {
        "marker": marker,
        "marker_argument": f"--user-data-dir={user_data_dir}",
        "user_data_dir": str(user_data_dir),
        "handshake_path": str(handshake_path),
        "expected_executable": str(expected_executable.resolve()),
        "environment": {
            "GSPLAT_Q1_BROWSER_OWNER_MARKER": marker,
            "GSPLAT_Q1_BROWSER_USER_DATA_DIR": str(user_data_dir),
            "GSPLAT_Q1_BROWSER_HANDSHAKE_PATH": str(handshake_path),
        },
    }
