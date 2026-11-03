"""G502 Lightspeed HID++ probe.

Talks to the mouse through the Lightspeed receiver's vendor HID interface (no Logitech
software). Lists the device's HID++ 2.0 features, then enables Mouse Button Spy (0x8110)
and prints every button state change.

Only volatile state is touched: spy mode is stopped on exit and resets on power-cycle.

    pip install hidapi
    python g502_probe.py            # info + guided per-button test
    python g502_probe.py --raw      # info + free listen until Ctrl+C
    python g502_probe.py --info     # info only
"""
import argparse
import sys
import time

import hid

LOGI_VID = 0x046D
RECEIVER_PIDS = {0xC539, 0xC53F, 0xC547}  # Lightspeed receivers
HIDPP_USAGE_PAGE = 0xFF00
SHORT, LONG = 0x10, 0x11  # report IDs: 7-byte and 20-byte messages
SW_ID = 0x0A  # arbitrary nonzero tag so we can match our replies

FEATURES = {
    0x0000: "ROOT", 0x0001: "FEATURE_SET", 0x0002: "FEATURE_INFO",
    0x0003: "DEVICE_FW_VERSION", 0x0005: "DEVICE_NAME", 0x0007: "DEVICE_FRIENDLY_NAME",
    0x0020: "CONFIG_CHANGE", 0x1000: "BATTERY_STATUS", 0x1001: "BATTERY_VOLTAGE",
    0x1004: "UNIFIED_BATTERY", 0x1300: "LED_CONTROL", 0x1802: "DEVICE_RESET",
    0x1B04: "REPROG_CONTROLS_V4", 0x1D4B: "WIRELESS_DEVICE_STATUS",
    0x2201: "ADJUSTABLE_DPI", 0x2202: "EXTENDED_ADJUSTABLE_DPI", 0x8060: "REPORT_RATE",
    0x8061: "EXTENDED_REPORT_RATE", 0x8070: "COLOR_LED_EFFECTS", 0x8071: "RGB_EFFECTS",
    0x8090: "MODE_STATUS", 0x8100: "ONBOARD_PROFILES", 0x8110: "MOUSE_BUTTON_SPY",
}

HIDPP10_ERRORS = {
    0x01: "invalid sub-id", 0x02: "invalid address", 0x03: "invalid value",
    0x04: "connect fail", 0x07: "busy", 0x08: "unknown device (not paired / asleep?)",
    0x09: "resource error (mouse asleep? move it)", 0x0A: "request unavailable",
}
HIDPP20_ERRORS = {
    0x01: "unknown", 0x02: "invalid argument", 0x03: "out of range", 0x04: "hw error",
    0x05: "logitech internal", 0x06: "invalid feature index", 0x07: "invalid function",
    0x08: "busy", 0x09: "unsupported",
}


class HidppError(Exception):
    pass


def hexs(data):
    return " ".join(f"{b:02X}" for b in data)


class Device:
    def __init__(self, dev_index):
        paths = {}
        for d in hid.enumerate(LOGI_VID):
            if d["product_id"] in RECEIVER_PIDS and d["usage_page"] == HIDPP_USAGE_PAGE:
                paths[d["usage"]] = d["path"]
        if 2 not in paths:
            sys.exit("No Lightspeed receiver HID++ interface found.")
        # Windows exposes short/long reports as separate collections; replies and
        # notifications can arrive on either, so read both.
        self.handles = {}
        for usage in (1, 2):
            if usage in paths:
                h = hid.device()
                h.open_path(paths[usage])
                h.set_nonblocking(True)
                self.handles[usage] = h
        self.dev = dev_index

    def close(self):
        for h in self.handles.values():
            h.close()

    def poll(self):
        for h in self.handles.values():
            data = h.read(64)
            if data:
                return bytes(data)
        return None

    def request(self, feat_idx, func, params=(), timeout=2.0):
        func_sw = (func << 4) | SW_ID
        msg = bytes([LONG, self.dev, feat_idx, func_sw, *params]).ljust(20, b"\0")
        if self.handles[2].write(msg) < 0:
            raise HidppError("write failed")
        deadline = time.monotonic() + timeout
        while time.monotonic() < deadline:
            r = self.poll()
            if not r:
                time.sleep(0.002)
                continue
            if r[1] != self.dev:
                continue
            if r[0] == SHORT and r[2] == 0x8F and r[3] == feat_idx and r[4] == func_sw:
                raise HidppError(f"HID++1.0 error 0x{r[5]:02X}: {HIDPP10_ERRORS.get(r[5], '?')}")
            if r[0] == LONG and r[2] == 0xFF and r[3] == feat_idx and r[4] == func_sw:
                raise HidppError(f"HID++2.0 error 0x{r[5]:02X}: {HIDPP20_ERRORS.get(r[5], '?')}")
            if r[2] == feat_idx and r[3] == func_sw:
                return r[4:]
        raise HidppError(f"timeout (feature index {feat_idx}, function {func})")

    def ping(self):
        r = self.request(0, 1, [0, 0, 0x5A])
        return r[0], r[1]

    def feature_index(self, feature_id):
        r = self.request(0, 0, [feature_id >> 8, feature_id & 0xFF])
        return r[0] or None

    def features(self):
        fs = self.feature_index(0x0001)
        count = self.request(fs, 0)[0]
        out = []
        for i in range(1, count + 1):
            r = self.request(fs, 1, [i])
            out.append((i, (r[0] << 8) | r[1], r[3]))
        return out

    def name(self):
        idx = self.feature_index(0x0005)
        if not idx:
            return None
        length = self.request(idx, 0)[0]
        name = b""
        while len(name) < length:
            chunk = self.request(idx, 1, [len(name)])[: length - len(name)].rstrip(b"\0")
            if not chunk:
                break
            name += chunk
        return name.decode(errors="replace")


BUTTONS = [
    "left click", "right click", "middle click (press wheel)",
    "wheel tilt left", "wheel tilt right",
    "front thumb button (G4, forward)", "rear thumb button (G5, back)",
    "sniper button (G6, red thumb button)",
    "front button left of left-click (G8, DPI up)",
    "rear button left of left-click (G7, DPI down)",
    "button behind the wheel (G9)",
]
WIN_VKS = {0x01: "left", 0x02: "right", 0x04: "middle", 0x05: "back", 0x06: "forward"}


def windows_buttons():
    """Standard buttons Windows currently sees as held down."""
    import ctypes
    state = ctypes.windll.user32.GetAsyncKeyState
    return {name for vk, name in WIN_VKS.items() if state(vk) & 0x8000}


def next_spy_mask(dev, spy_idx):
    r = dev.poll()
    if r and r[1] == dev.dev and r[2] == spy_idx and (r[3] & 0x0F) == 0:
        return (r[4] << 8) | r[5]
    return None


def guided(dev, spy_idx):
    print("\nGuided test: press and release each button when asked (wait 6 s to skip).")
    if sys.stdin.isatty():
        input("Press Enter to start...")
    results = []
    for name in BUTTONS:
        while dev.poll():  # drop stale events
            pass
        print(f"  -> {name:46} ", end="", flush=True)
        mask, seen, released = 0, set(), False
        deadline = time.monotonic() + 6
        while time.monotonic() < deadline and not released:
            seen |= windows_buttons()
            m = next_spy_mask(dev, spy_idx)
            if m is None:
                time.sleep(0.001)
            elif m:
                mask |= m
            elif mask:
                released = True
        bits = [i for i in range(16) if mask >> i & 1]
        print(f"spy bits={str(bits or 'none'):<8} windows saw={sorted(seen) or 'nothing'}")
        results.append(bits)
        time.sleep(0.4)

    dupes = {b for b in sum(results, []) if sum(b in r for r in results) > 1}
    if dupes:
        print(f"\nBits shared by several buttons: {sorted(dupes)}")
    print("\nDid the DPI buttons still change DPI (LED flash) during the test?")


def listen(dev, spy_idx, seconds):
    print("\nPress each button (Ctrl+C to stop). Raw spy payload + set bits:\n")
    end = time.monotonic() + seconds if seconds else None
    last = None
    while end is None or time.monotonic() < end:
        r = dev.poll()
        if not r:
            time.sleep(0.001)
            continue
        if r[1] != dev.dev:
            print(f"  [receiver] {hexs(r)}")
            continue
        if r[2] == spy_idx and (r[3] & 0x0F) == 0:  # sw_id 0 = notification
            payload = r[4:]
            if payload == last:
                continue
            last = payload
            mask = (payload[0] << 8) | payload[1]
            bits = [i for i in range(16) if mask >> i & 1]
            print(f"  spy event {r[3] >> 4}: {hexs(payload[:4])}  bits={bits or '-'}")
        else:
            print(f"  [other]    {hexs(r)}")


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--device-index", type=lambda s: int(s, 0), default=1,
                    help="receiver slot of the mouse (default 1)")
    ap.add_argument("--info", action="store_true", help="print info only, don't listen")
    ap.add_argument("--raw", action="store_true", help="free listen instead of guided test")
    ap.add_argument("--seconds", type=float, default=0, help="--raw duration (0 = forever)")
    args = ap.parse_args()

    dev = Device(args.device_index)
    try:
        major, minor = dev.ping()
        print(f"HID++ {major}.{minor} on device index {args.device_index}")
        print(f"Name: {dev.name()}")
        print("\nFeatures:")
        for idx, fid, ver in dev.features():
            print(f"  [{idx:2}] 0x{fid:04X} v{ver}  {FEATURES.get(fid, '')}")

        ob = dev.feature_index(0x8100)
        if ob:
            mode = dev.request(ob, 2)[0]
            print(f"\nOnboard mode: {mode} ({ {1: 'onboard', 2: 'host'}.get(mode, '?') })")

        spy = dev.feature_index(0x8110)
        if not spy:
            print("\nMouse Button Spy (0x8110) not supported.")
            return
        print(f"\nButton Spy at index {spy}, getNbButtons -> {hexs(dev.request(spy, 0)[:4])}")
        if args.info:
            return
        dev.request(spy, 1)  # startSpy
        try:
            if args.raw:
                listen(dev, spy, args.seconds)
            else:
                guided(dev, spy)
        except KeyboardInterrupt:
            pass
        finally:
            dev.request(spy, 2)  # stopSpy
            print("\nSpy stopped.")
    except HidppError as e:
        sys.exit(f"Error: {e}")
    finally:
        dev.close()


if __name__ == "__main__":
    main()
