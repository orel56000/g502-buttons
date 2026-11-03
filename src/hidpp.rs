//! Minimal HID++ 2.0 client for a mouse behind a Logitech Lightspeed receiver.

use std::ffi::CString;
use std::fmt;
use std::time::{Duration, Instant};

use hidapi::{DeviceInfo, HidApi, HidDevice, HidError};

const LOGI_VID: u16 = 0x046D;
const RECEIVER_PIDS: [u16; 3] = [0xC539, 0xC53F, 0xC547];
const HIDPP_USAGE_PAGE: u16 = 0xFF00;
pub const SHORT: u8 = 0x10; // 7-byte report
pub const LONG: u8 = 0x11; // 20-byte report
const SW_ID: u8 = 0x0A; // nonzero tag that marks replies to our own requests

pub const FEATURE_BUTTON_SPY: u16 = 0x8110;

#[derive(Debug)]
pub enum Error {
    NoReceiver,
    Hid(HidError),
    Hidpp10(u8),
    Hidpp20(u8),
    Unsupported(u16),
    Timeout,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Error::NoReceiver => write!(f, "Lightspeed receiver not found"),
            Error::Hid(e) => write!(f, "HID error: {e}"),
            Error::Hidpp10(0x08 | 0x09) => write!(f, "mouse not reachable (off or asleep?)"),
            Error::Hidpp10(c) => write!(f, "HID++ 1.0 error 0x{c:02X}"),
            Error::Hidpp20(c) => write!(f, "HID++ 2.0 error 0x{c:02X}"),
            Error::Unsupported(id) => write!(f, "feature 0x{id:04X} not supported"),
            Error::Timeout => write!(f, "no reply from mouse"),
        }
    }
}

impl From<HidError> for Error {
    fn from(e: HidError) -> Self {
        Error::Hid(e)
    }
}

pub struct Receiver {
    handles: Vec<HidDevice>,
    long: usize, // handle that accepts 20-byte reports
    pub dev: u8,
}

fn is_hidpp(d: &DeviceInfo) -> bool {
    if d.vendor_id() != LOGI_VID || !RECEIVER_PIDS.contains(&d.product_id()) {
        return false;
    }
    if cfg!(windows) {
        // One path per collection: usage 1 = short reports, 2 = long reports.
        d.usage_page() == HIDPP_USAGE_PAGE && matches!(d.usage(), 1 | 2)
    } else {
        // One hidraw node for the whole HID++ interface.
        d.usage_page() == HIDPP_USAGE_PAGE || d.interface_number() == 2
    }
}

impl Receiver {
    pub fn open(api: &mut HidApi, dev: u8) -> Result<Self, Error> {
        api.refresh_devices()?;
        let mut found: Vec<(CString, u16)> = Vec::new();
        for d in api.device_list().filter(|d| is_hidpp(d)) {
            if !found.iter().any(|(p, _)| p.as_c_str() == d.path()) {
                found.push((d.path().to_owned(), d.usage()));
            }
        }
        if found.is_empty() {
            return Err(Error::NoReceiver);
        }
        let long = found.iter().position(|&(_, usage)| usage == 2).unwrap_or(0);
        let handles = found
            .iter()
            .map(|(path, _)| {
                let h = api.open_path(path)?;
                h.set_blocking_mode(false)?;
                Ok(h)
            })
            .collect::<Result<_, HidError>>()?;
        Ok(Receiver { handles, long, dev })
    }

    /// Next pending report from any handle, without blocking.
    pub fn poll(&self) -> Result<Option<Vec<u8>>, Error> {
        let mut buf = [0u8; 64];
        for h in &self.handles {
            let n = h.read(&mut buf)?;
            if n > 0 {
                return Ok(Some(buf[..n].to_vec()));
            }
        }
        Ok(None)
    }

    /// Next report, waiting up to `timeout_ms`. Blocks on the long-report handle (where
    /// button events arrive) so they are picked up the instant they come in.
    pub fn wait(&self, timeout_ms: i32) -> Result<Option<Vec<u8>>, Error> {
        let mut buf = [0u8; 64];
        for (i, h) in self.handles.iter().enumerate() {
            if i != self.long {
                let n = h.read(&mut buf)?;
                if n > 0 {
                    return Ok(Some(buf[..n].to_vec()));
                }
            }
        }
        let n = self.handles[self.long].read_timeout(&mut buf, timeout_ms)?;
        Ok((n > 0).then(|| buf[..n].to_vec()))
    }

    /// Sends a request and waits for its reply; returns the reply's parameters.
    /// Unrelated reports that arrive meanwhile are dropped.
    pub fn request(&self, feat_idx: u8, func: u8, params: &[u8]) -> Result<Vec<u8>, Error> {
        let func_sw = (func << 4) | SW_ID;
        let mut msg = [0u8; 20];
        msg[..4].copy_from_slice(&[LONG, self.dev, feat_idx, func_sw]);
        msg[4..4 + params.len()].copy_from_slice(params);
        self.handles[self.long].write(&msg)?;

        let deadline = Instant::now() + Duration::from_secs(2);
        while Instant::now() < deadline {
            let Some(r) = self.poll()? else {
                std::thread::sleep(Duration::from_millis(2));
                continue;
            };
            if r.len() < 6 || r[1] != self.dev {
                continue;
            }
            match (r[0], r[2], r[3], r[4]) {
                (SHORT, 0x8F, f, s) if f == feat_idx && s == func_sw => return Err(Error::Hidpp10(r[5])),
                (LONG, 0xFF, f, s) if f == feat_idx && s == func_sw => return Err(Error::Hidpp20(r[5])),
                (_, f, s, _) if f == feat_idx && s == func_sw => return Ok(r[4..].to_vec()),
                _ => {}
            }
        }
        Err(Error::Timeout)
    }

    pub fn ping(&self) -> Result<(), Error> {
        self.request(0, 1, &[0, 0, 0x5A]).map(|_| ())
    }

    pub fn feature_index(&self, id: u16) -> Result<u8, Error> {
        match self.request(0, 0, &id.to_be_bytes())?[0] {
            0 => Err(Error::Unsupported(id)),
            idx => Ok(idx),
        }
    }

    pub fn start_spy(&self, spy_idx: u8) -> Result<(), Error> {
        self.request(spy_idx, 1, &[]).map(|_| ())
    }

    pub fn stop_spy(&self, spy_idx: u8) -> Result<(), Error> {
        self.request(spy_idx, 2, &[]).map(|_| ())
    }
}
