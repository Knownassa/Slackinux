#!/usr/bin/env bash
# Phase 7 — Resource benchmark for Slackinux versus the official Slack desktop
# package.
#
# Measures, for each candidate binary:
#   - cold-start time (launch -> renderer ready, or a Chromium renderer spawned)
#   - idle PSS and RSS (summed over the whole process tree via smaps_rollup)
#   - CPU usage over an idle window (configurable, default 10 minutes)
#
# Usage:
#   scripts/benchmark.sh                      # benchmark target/release/slackinux
#   scripts/benchmark.sh [path/to/slackinux] [--slack /path/to/slack]
#   SLACKINUX_BENCH_IDLE_SECS=60 scripts/benchmark.sh ...   # shorter idle window
#
# The Slackinux candidate must already be built (cargo tauri build --no-bundle).
# The --slack baseline is optional; when it is missing the comparison is simply
# skipped with a note. It expects the official Electron Slack binary (extract
# the .deb without installing: `ar x slack.deb && tar -xf data.tar.xz`, then
# pass usr/lib/slack/slack) and readiness is detected by the appearance of a
# Chromium renderer process. Set SLACKINUX_BENCH_ARGS="--no-sandbox" when the
# unpackaged Electron binary refuses to sandbox itself.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
candidate="${1:-$root/target/release/slackinux}"
slack_bin=""
if [[ "${2:-}" == "--slack" && -n "${3:-}" ]]; then slack_bin="$3"; fi
idle_secs="${SLACKINUX_BENCH_IDLE_SECS:-600}"
# Extra arguments passed to each launched binary (e.g. --no-sandbox for an
# unpackaged Electron baseline). Intentionally word-split.
bench_args="${SLACKINUX_BENCH_ARGS:-}"
work="$(mktemp -d /tmp/slackinux-bench.XXXXXX)"
trap 'rm -rf "$work"' EXIT

# Isolate the app so the benchmark run does not collide with a running instance
# (single-instance plugin) or pollute real user data. A per-run XDG dir also
# gives us a fresh, deterministic diagnostic log to time the cold start.
launch_env() {
  env \
    HOME="$work/home" \
    XDG_CONFIG_HOME="$work/config" \
    XDG_DATA_HOME="$work/data" \
    XDG_STATE_HOME="$work/state" \
    XDG_CACHE_HOME="$work/cache" \
    "$@"
}

launch_log() { printf '%s' "$work/state/slackinux/logs/slackinux.log"; }

pids_of() { # recursively list $1 and all descendants
  local root_pid="$1"
  {
    echo "$root_pid"
    for ppid in $(ps -eo pid=,ppid= | awk -v p="$root_pid" '$2==p {print $1}'); do
      pids_of "$ppid"
    done
  }
}

pss_mb_of() { # sum Pss over every pid in $@ from smaps_rollup
  local pid total=0
  for pid in "$@"; do
    local rollup="/proc/$pid/smaps_rollup"
    [[ -r "$rollup" ]] || continue
    local kb
    kb="$(awk '/^Pss:/ {print $2}' "$rollup")" || kb=0
    total=$((total + ${kb:-0}))
  done
  awk -v k="$total" 'BEGIN {printf "%.1f", k/1024}'
}

rss_mb_of() { # sum Rss over every pid in $@ from /proc/*/statm
  local pid total=0
  for pid in "$@"; do
    local statm="/proc/$pid/statm"
    [[ -r "$statm" ]] || continue
    local rss_kb
    rss_kb="$(awk '{print int($2 * 4)}' "$statm")" || rss_kb=0
    total=$((total + ${rss_kb:-0}))
  done
  awk -v k="$total" 'BEGIN {printf "%.1f", k/1024}'
}

cpu_jiffies_of() { # sum utime+stime (jiffies) over every pid in $@
  local pid total=0
  for pid in "$@"; do
    local stat="/proc/$pid/stat"
    [[ -r "$stat" ]] || continue
    # Field 14 = utime, 15 = stime (1-indexed after comm, which may contain
    # spaces; slice from the last ')' instead of splitting on fields).
    local tail
    tail="$(sed -E 's/^[0-9]+ \(.*\) //' "$stat")"
    local utime stime
    utime="$(awk '{print $12}' <<<"$tail")" || utime=0
    stime="$(awk '{print $13}' <<<"$tail")" || stime=0
    total=$((total + ${utime:-0} + ${stime:-0}))
  done
  echo "$total"
}

# Detect readiness. $1 = strategy ("log" or "renderer"), $2 = app pid.
# "log" (Slackinux): the diagnostic log reaches the renderer-ready line.
# "renderer" (Electron Slack): a --type=renderer process exists in the tree
# (Chromium-backed UIs are Wayland-native, so X11 window tools cannot see
# them; the renderer process is the reliable cross-compositor signal).
app_ready() {
  local strategy="$1" pid="$2"
  case "$strategy" in
    log)
      [[ -f "$log_path" ]] && grep -q "webview created successfully" "$log_path"
      ;;
    renderer)
      local desc
      for desc in $(pids_of "$pid"); do
        if tr '\0' ' ' <"/proc/$desc/cmdline" 2>/dev/null | grep -q -- "--type=renderer"; then
          return 0
        fi
      done
      return 1
      ;;
    *)
      return 1
      ;;
  esac
}

measure() { # $1 = label, $2 = binary, $3 = ready strategy ("" -> "log")
  local label="$1" binary="$2" strategy="${3:-log}"
  if [[ -z "$binary" ]]; then
    echo "  $label: (not provided, skipped)"
    return
  fi
  if [[ ! -x "$binary" ]]; then
    echo "  $label: binary not found at $binary"
    return
  fi

  local log_path launch_start launch_ready
  rm -rf "$work/home" "$work/config" "$work/data" "$work/state" "$work/cache"
  mkdir -p "$work/home" "$work/config" "$work/data" "$work/state" "$work/cache"
  log_path="$(launch_log)"
  launch_start="$(date +%s%3N)"

  # Launch in the background, detached from our signal handling. Extra args
  # (e.g. --no-sandbox for an unpackaged Electron baseline) come from
  # SLACKINUX_BENCH_ARGS.
  ( launch_env "$binary" $bench_args >"$work/$label.stdout" 2>"$work/$label.stderr" ) &
  local app_pid=$!

  # Cold start: wait for the ready signal (generous timeout).
  launch_ready=""
  for _ in $(seq 1 120); do
    if app_ready "$strategy" "$app_pid"; then
      launch_ready="$(date +%s%3N)"
      break
    fi
    if ! kill -0 "$app_pid" 2>/dev/null; then
      break
    fi
    sleep 0.5
  done

  if [[ -z "$launch_ready" ]]; then
    echo "  $label: app did not reach the ready state within 60s (exit: $(wait "$app_pid" 2>/dev/null; echo $?))"
    sed -n '1,20p' "$work/$label.stderr"
    return
  fi

  # Let the webview settle to its idle footprint.
  sleep 5

  local tree
  tree="$(pids_of "$app_pid" | tr '\n' ' ')"

  local pss rss cpu_before cpu_after cpu_percent clk_tck
  pss="$(pss_mb_of $tree)"
  rss="$(rss_mb_of $tree)"
  cpu_before="$(cpu_jiffies_of $tree)"
  clk_tck="$(getconf CLK_TCK)"
  [[ "$clk_tck" =~ ^[0-9]+$ ]] || clk_tck=100

  # Idle window: sample CPU before and after the user-configurable window.
  sleep "$idle_secs"

  tree="$(pids_of "$app_pid" | tr '\n' ' ')"
  cpu_after="$(cpu_jiffies_of $tree)"
  cpu_percent="$(awk -v a="$cpu_before" -v b="$cpu_after" -v s="$idle_secs" -v h="$clk_tck" \
    'BEGIN {v=(b-a)/(h*s)*100; if (v<0) v=0; printf "%.1f", v}')"

  # Stop the app and its whole process tree (the binary, its WebKit helper
  # processes, and any descendants) so no benchmark run lingers.
  local tree_all child
  tree_all="$(pids_of "$app_pid" | tr '\n' ' ')"
  for child in $tree_all; do
    kill "$child" 2>/dev/null || true
  done
  wait "$app_pid" 2>/dev/null || true

  local cold_ms
  cold_ms=$((launch_ready - launch_start))

  echo "  $label: cold-start=${cold_ms}ms pss=${pss}MB rss=${rss}MB cpu=${cpu_percent}%"
}

echo "Slackinux Phase 7 benchmark"
echo "  candidate: $candidate"
echo "  idle window: ${idle_secs}s"
echo "  session: ${XDG_SESSION_TYPE:-unknown}, desktop: ${XDG_CURRENT_DESKTOP:-unknown}"
echo ""
echo "Slackinux (WebKitGTK):"
measure "slackinux" "$candidate" "log"
echo ""
echo "Official Slack (Electron baseline):"
measure "official-slack" "$slack_bin" "renderer"
echo ""
echo "done."
