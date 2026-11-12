#!/usr/bin/env bash
# Exercise the real GTK/WebKit desktop locale flow through AT-SPI and keyboard input.
# Run on a Linux host with Xvfb, Openbox, xdotool, AT-SPI, pyatspi, and D-Bus.
# Restricted containers may pass WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS=1; this script never sets it.
set -euo pipefail

if [[ "$#" -ne 1 ]]; then
    echo "Usage: bash smoke-linux-locale.sh /path/to/diskunfold" >&2
    exit 2
fi

app="$(realpath "$1")"
if [[ ! -x "$app" ]]; then
    echo "DiskUnfold binary is not executable: $app" >&2
    exit 2
fi

for tool in xvfb-run dbus-run-session openbox xdotool python3; do
    if ! command -v "$tool" >/dev/null 2>&1; then
        echo "Required Linux smoke tool is missing: $tool" >&2
        exit 77
    fi
done
if ! python3 -c 'import pyatspi' >/dev/null 2>&1; then
    echo "Python AT-SPI bindings are unavailable: install python3-pyatspi in the test environment" >&2
    exit 77
fi

root="$(mktemp -d "${TMPDIR:-/tmp}/diskunfold-linux-locale.XXXXXX")"
cleanup() {
    status=$?
    if [[ "$status" -eq 0 ]]; then
        rm -rf -- "$root"
    else
        echo "Native locale smoke failed; logs and isolated profile retained at: $root" >&2
    fi
}
trap cleanup EXIT

"$app" --help >"$root/help.txt"
"$app" --version >"$root/version.txt"
cat "$root/version.txt"

xvfb-run -a -s "-screen 0 1024x700x24" dbus-run-session -- bash -s -- "$app" "$root" <<'SMOKE'
set -euo pipefail
unset NO_AT_BRIDGE
export GTK_A11Y=atspi
openbox >"$2/openbox.log" 2>&1 &
wm_pid=$!
trap 'kill "$wm_pid" 2>/dev/null || true' EXIT

python3 - <<'GDK'
import gi
gi.require_version("Gdk", "4.0")
gi.require_version("Gtk", "4.0")
from gi.repository import Gdk, Gtk

Gtk.init()
display = Gdk.Display.get_default()
if display is None or display.get_monitors().get_n_items() == 0:
    raise SystemExit("GTK/GDK could not observe the Xvfb display")
monitor = display.get_monitors().get_item(0)
geometry = monitor.get_geometry()
width_mm = monitor.get_width_mm()
height_mm = monitor.get_height_mm()
if (geometry.width, geometry.height) != (1024, 700):
    raise SystemExit(f"unexpected Xvfb geometry: {geometry.width}x{geometry.height}")
if width_mm <= 0 or height_mm <= 0:
    raise SystemExit(f"GDK did not report physical display dimensions: {width_mm}x{height_mm}mm")
dpi_x = geometry.width * 25.4 / width_mm
dpi_y = geometry.height * 25.4 / height_mm
print(
    f"DISPLAY: {geometry.width}x{geometry.height} px; "
    f"GDK physical size {width_mm}x{height_mm} mm; "
    f"calculated Xvfb DPI {dpi_x:.2f}x{dpi_y:.2f}; "
    f"scale factor {monitor.get_scale_factor()}"
)
GDK

python3 - "$1" "$2" <<'PY'
import os
import shutil
import subprocess
import sys
import time

import pyatspi

app = os.path.realpath(sys.argv[1])
root_dir = os.path.realpath(sys.argv[2])
profile = os.path.join(root_dir, "profile")
os.makedirs(profile, exist_ok=True)
processes = []

EXPECTED = {
    "en": {
        "button": "Language and display",
        "language": "Display language",
        "date": "Date format",
        "number": "Number format",
        "zone": "Time zone",
        "scale": "App scale",
    },
    "ko": {
        "button": "언어 및 표시",
        "language": "표시 언어",
        "date": "날짜 형식",
        "number": "숫자 형식",
        "zone": "시간대",
        "scale": "앱 배율",
    },
}


def run(args, *, timeout=5, check=True):
    result = subprocess.run(
        args,
        check=False,
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        timeout=timeout,
    )
    if check and result.returncode != 0:
        raise RuntimeError(
            f"command failed ({result.returncode}): {args!r}\n"
            f"stdout: {result.stdout}\nstderr: {result.stderr}"
        )
    return result


def role_name(node):
    try:
        return node.getRoleName().lower()
    except Exception:
        return ""


def node_name(node):
    try:
        return node.name or ""
    except Exception:
        return ""


def tree_snapshot():
    desktop = pyatspi.Registry.getDesktop(0)
    stack = [(desktop, 0)]
    seen = 0
    while stack and seen < 12000:
        node, depth = stack.pop()
        seen += 1
        yield node
        if depth >= 32:
            continue
        try:
            count = node.childCount
        except Exception:
            continue
        for index in range(count - 1, -1, -1):
            try:
                child = node.getChildAtIndex(index)
            except Exception:
                continue
            if child is not None:
                stack.append((child, depth + 1))


def find_named(name, roles=()):
    matches = []
    for node in tree_snapshot():
        if node_name(node) != name:
            continue
        role = role_name(node)
        if roles and role not in roles:
            continue
        matches.append(node)
    return matches


def wait_for(description, probe, proc=None, seconds=30):
    deadline = time.monotonic() + seconds
    last_error = None
    while time.monotonic() < deadline:
        if proc is not None and proc.poll() is not None:
            log_path = getattr(proc, "log_path", "<unknown>")
            log = open(log_path, encoding="utf-8", errors="replace").read()
            raise RuntimeError(
                f"DiskUnfold exited with {proc.returncode} while waiting for {description};\n{log}"
            )
        try:
            value = probe()
            if value:
                return value
        except Exception as exc:
            last_error = exc
        time.sleep(0.12)
    detail = f"; last probe error: {last_error}" if last_error else ""
    names = []
    try:
        for node in tree_snapshot():
            name = node_name(node)
            if name:
                names.append(f"{role_name(node)}:{name!r}")
            if len(names) >= 80:
                break
    except Exception as exc:
        names.append(f"accessibility snapshot failed: {exc}")
    tree = "; accessible names: " + ", ".join(names) if names else "; no named AT-SPI nodes"
    raise TimeoutError(f"timed out waiting for {description}{detail}{tree}")


def is_showing(node):
    try:
        return node.getState().contains(pyatspi.STATE_SHOWING)
    except Exception:
        return False


def assert_showing(node, description):
    if not is_showing(node):
        raise AssertionError(f"{description} is in the accessibility tree but not showing")


def visible_window_ids():
    result = run(
        ["xdotool", "search", "--onlyvisible", "--name", "^DiskUnfold$"],
        timeout=5,
        check=False,
    )
    return [line for line in result.stdout.splitlines() if line.strip()]


def wait_for_window(proc):
    def probe():
        ids = visible_window_ids()
        return ids[0] if len(ids) == 1 else None

    window_id = wait_for("one visible DiskUnfold window", probe, proc)
    return window_id


def app_frame_count():
    return sum(
        1
        for node in tree_snapshot()
        if role_name(node) in ("frame", "window") and node_name(node) == "DiskUnfold"
    )


def locale_button(language, proc=None):
    expected = EXPECTED[language]["button"]
    return wait_for(
        f"accessible locale settings button named {expected!r}",
        lambda: next(iter(find_named(expected, ("push button", "button"))), None),
        proc,
    )


def activate(button):
    try:
        action = button.queryAction()
    except Exception as exc:
        raise RuntimeError(f"accessible settings button has no Action interface: {exc}") from exc
    for index in range(action.nActions):
        action_name = action.getName(index).lower()
        if action_name in ("click", "press", "activate"):
            if not action.doAction(index):
                raise RuntimeError(f"AT-SPI action {action_name!r} failed")
            return
    if action.nActions == 0:
        raise RuntimeError("accessible settings button exposes no AT-SPI actions")
    if not action.doAction(0):
        raise RuntimeError(f"AT-SPI action {action.getName(0)!r} failed")


def combo(language, field, proc=None):
    expected = EXPECTED[language][field]
    return wait_for(
        f"accessible {field} selector named {expected!r}",
        lambda: next(iter(find_named(expected, ("combo box",))), None),
        proc,
    )


def selected_option(node):
    try:
        selection = node.querySelection()
        for index in range(selection.nSelectedChildren):
            selected = selection.getSelectedChild(index)
            if selected is not None and node_name(selected):
                return node_name(selected)
    except Exception:
        pass
    try:
        for index in range(node.childCount):
            child = node.getChildAtIndex(index)
            if child is None:
                continue
            if child.getState().contains(pyatspi.STATE_SELECTED):
                return node_name(child)
    except Exception:
        pass
    try:
        value = node.queryValue().currentValue
        return f"AT-SPI value {value:g}"
    except Exception:
        return "selected option not exposed"


def window_activate(window_id):
    run(["xdotool", "windowactivate", "--sync", window_id], timeout=5)


def keyboard_select(window_id, selector, edge):
    window_activate(window_id)
    try:
        focused = selector.queryComponent().grabFocus()
    except Exception as exc:
        raise RuntimeError(f"could not focus the AT-SPI selector: {exc}") from exc
    if not focused:
        raise RuntimeError(f"AT-SPI refused focus for {node_name(selector)!r}")
    for key in ("space", edge, "Return"):
        run(["xdotool", "key", "--clearmodifiers", key], timeout=5)


def open_locale(language, proc):
    button = locale_button(language)
    assert_showing(button, f"{language} language settings button")
    activate(button)
    language_control = wait_for(
        f"{language} display-language control",
        lambda: next(
            iter(find_named(EXPECTED[language]["language"], ("combo box",))), None
        ),
        proc,
    )
    assert_showing(language_control, f"{language} display-language control")
    return language_control


def choose_language(current_language, target_language, proc, window_id):
    control = combo(current_language, "language", proc)
    edge = "Home" if target_language == "en" else "End"
    keyboard_select(window_id, control, edge)
    button = locale_button(target_language, proc)
    field = combo(target_language, "language", proc)
    assert_showing(button, f"live-switched {target_language} language settings button")
    assert_showing(field, f"live-switched {target_language} language control")
    print(
        f"PASS: live keyboard language selection to {target_language} updated the native WebKit accessibility tree"
    )
    return target_language


def launch(label, lang, data_dir=profile):
    os.makedirs(data_dir, exist_ok=True)
    log_path = os.path.join(root_dir, f"{label}.log")
    env = os.environ.copy()
    env.update(
        {
            "DISKUNFOLD_DATA_DIR": data_dir,
            "LC_ALL": "C.UTF-8",
            "LC_MESSAGES": "C.UTF-8",
            "LANG": "C.UTF-8",
            "GTK_A11Y": "atspi",
        }
    )
    if lang == "ko-os":
        # Exercise the raw Linux message-locale provider without requiring a generated locale.
        env["LC_ALL"] = ""
        env["LC_MESSAGES"] = "ko_KR.UTF-8"
    env.pop("NO_AT_BRIDGE", None)
    log = open(log_path, "w", encoding="utf-8")
    proc = subprocess.Popen(
        [app],
        env=env,
        stdout=log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    proc.log_path = log_path
    proc.log_handle = log
    processes.append(proc)
    return proc


def assert_startup_language(language, proc):
    window_id = wait_for_window(proc)
    count = wait_for("one AT-SPI DiskUnfold window", lambda: app_frame_count(), proc)
    if count != 1:
        raise AssertionError(f"expected one AT-SPI app window, got {count}")
    button = locale_button(language, proc)
    assert_showing(button, f"{language} startup locale settings button")
    print(
        f"PASS: real GTK/WebKit {language} startup has one visible DiskUnfold window and accessible name {node_name(button)!r}"
    )
    return window_id


def close_app(proc, window_id):
    if proc.poll() is not None:
        raise RuntimeError(f"DiskUnfold exited before the close request with {proc.returncode}")
    window_activate(window_id)
    run(["xdotool", "key", "--clearmodifiers", "alt+F4"], timeout=5)
    try:
        code = proc.wait(timeout=15)
    except subprocess.TimeoutExpired as exc:
        raise TimeoutError("DiskUnfold did not exit after Alt+F4 within 15 seconds") from exc
    proc.log_handle.close()
    if code != 0:
        raise RuntimeError(f"DiskUnfold exited with status {code} after its window close request")
    if visible_window_ids():
        raise AssertionError("a visible DiskUnfold window remained after the app exited")
    print("PASS: Alt+F4 closed the sole native app window and the process exited with status 0")


def log_selected_settings(language):
    values = {}
    for field in ("date", "number", "zone"):
        control = combo(language, field)
        values[field] = selected_option(control)
    print(
        "NATIVE PROFILE: "
        f"date format={values['date']}; number format={values['number']}; "
        f"time-zone policy={values['zone']}"
    )


def assert_no_duplicate_instance(proc, window_id):
    duplicate_env = os.environ.copy()
    duplicate_env.update(
        {
            "DISKUNFOLD_DATA_DIR": profile,
            "LC_ALL": "C.UTF-8",
            "LC_MESSAGES": "C.UTF-8",
            "LANG": "C.UTF-8",
            "GTK_A11Y": "atspi",
        }
    )
    duplicate_env.pop("NO_AT_BRIDGE", None)
    duplicate_log = open(os.path.join(root_dir, "duplicate.log"), "w", encoding="utf-8")
    duplicate = subprocess.Popen(
        [app],
        env=duplicate_env,
        stdout=duplicate_log,
        stderr=subprocess.STDOUT,
        start_new_session=True,
    )
    processes.append(duplicate)
    try:
        duplicate.wait(timeout=10)
    except subprocess.TimeoutExpired as exc:
        duplicate.terminate()
        raise AssertionError("a second invocation remained running beside the first instance") from exc
    duplicate_log.close()
    if proc.poll() is not None:
        raise AssertionError("the first app instance exited when a second invocation was started")
    if visible_window_ids() != [window_id]:
        raise AssertionError("a second visible DiskUnfold window appeared for a duplicate invocation")
    print(
        f"PASS: duplicate invocation exited with status {duplicate.returncode}; exactly one native app window remained"
    )


def main():
    print("NOT TESTED: F28 analysis/scan behavior is outside this locale smoke.")
    if shutil.which("ibus") or shutil.which("fcitx5-remote"):
        print("NOT TESTED: Korean IME composition was not exercised.")
    else:
        print("NOT AVAILABLE: no IBus/Fcitx frontend command is present; Korean IME composition was not exercised.")
    if shutil.which("orca"):
        print("NOT TESTED: Orca is present but its speech output was not exercised.")
    else:
        print("NOT AVAILABLE: Orca is not installed in this headless container; speech output was not exercised.")
    print("NOT TESTED: local time-zone status is not rendered in the locale settings accessibility tree.")
    # Resolve the desktop before starting the app so connection failures are explicit.
    desktop = pyatspi.Registry.getDesktop(0)
    _ = desktop.childCount
    print("PASS: AT-SPI registry and D-Bus desktop connection are available")
    if os.environ.get("WEBKIT_DISABLE_SANDBOX_THIS_IS_DANGEROUS") == "1":
        print("ENVIRONMENT: fixture-only WebKit sandbox override enabled for this container run")

    korean_os_profile = os.path.join(root_dir, "profile-os-korean")
    os_korean = launch("startup-ko-os", "ko-os", korean_os_profile)
    window = assert_startup_language("ko", os_korean)
    close_app(os_korean, window)
    print("PASS: a fresh profile selected Korean from the Linux LC_MESSAGES provider")

    first = launch("startup-en", "en")
    window = assert_startup_language("en", first)
    assert_no_duplicate_instance(first, window)
    open_locale("en", first)
    log_selected_settings("en")
    choose_language("en", "ko", first, window)
    choose_language("ko", "en", first, window)
    choose_language("en", "ko", first, window)
    close_app(first, window)

    second = launch("restart-ko", "en")
    window = assert_startup_language("ko", second)
    open_locale("ko", second)
    choose_language("ko", "en", second, window)
    close_app(second, window)

    third = launch("restart-en", "ko")
    window = assert_startup_language("en", third)
    open_locale("en", third)
    scale = combo("en", "scale")
    keyboard_select(window, scale, "End")
    language_control = combo("en", "language")
    assert_showing(language_control, "display-language control at 200% app scale")
    assert_showing(locale_button("en"), "locale settings button at 200% app scale")
    scale = combo("en", "scale")
    selected_scale = selected_option(scale)
    if "200" not in selected_scale and "value 5" not in selected_scale:
        raise AssertionError(f"App scale selector did not report 200% after keyboard selection: {selected_scale}")
    print(f"NATIVE PROFILE: app scale={selected_scale} (200% requested)")
    print("PASS: language/settings controls remain showing at 1024x700 and appScale=200")
    close_app(third, window)

    fourth = launch("restart-en-200", "ko")
    window = assert_startup_language("en", fourth)
    open_locale("en", fourth)
    language_control = combo("en", "language")
    assert_showing(language_control, "display-language control after appScale=200 relaunch")
    scale = combo("en", "scale")
    selected_scale = selected_option(scale)
    if "200" not in selected_scale and "value 5" not in selected_scale:
        raise AssertionError(f"App scale did not persist as 200% across relaunch: {selected_scale}")
    print("PASS: English locale and the 200% profile persisted across relaunch")
    close_app(fourth, window)

try:
    main()
finally:
    for proc in processes:
        if proc.poll() is None:
            proc.terminate()
            try:
                proc.wait(timeout=3)
            except subprocess.TimeoutExpired:
                proc.kill()
                proc.wait(timeout=3)
        handle = getattr(proc, "log_handle", None)
        if handle is not None and not handle.closed:
            handle.close()
PY
SMOKE
