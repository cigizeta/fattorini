//! Whether this machine is on the home network: its default gateway's MAC address is the router
//! `machines.yaml` names (the developer, 2026-11-19). Away, on any other network, the smithy and the
//! arena talk to this machine alone: another network may hold machines named as ours, by chance,
//! and a network card's MAC address is unique from the factory, where a WiFi name is not. A
//! deliberate copy of the address is not defended against.
//!
//! Asked often (every request on a LAN listener), the answer is kept `RECHECK`: looking up the
//! gateway may send an ARP request and wait for it.

use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

/// How long an answer is kept before the gateway is looked up again.
const RECHECK: Duration = Duration::from_secs(10);

struct Home {
    router: [u8; 6],
    /// The last answer, and when it was found.
    last: Mutex<Option<(Instant, bool)>>,
}

static HOME: OnceLock<Home> = OnceLock::new();

/// A MAC address as `arp -a` prints it, `aa-bb-cc-dd-ee-ff`, or with colons, in either case. The
/// all-zero placeholder of `examples/machines.yaml` is refused.
pub fn parse_mac(text: &str) -> Result<[u8; 6], String> {
    let how = "the router's MAC address, six pairs of hex digits as `arp -a` prints it";
    let parts: Vec<&str> = text.trim().split(['-', ':']).collect();
    if parts.len() != 6 {
        return Err(format!("router: not {how}"));
    }
    let mut mac = [0u8; 6];
    for (byte, part) in mac.iter_mut().zip(&parts) {
        if part.len() != 2 {
            return Err(format!("router: not {how}"));
        }
        *byte = u8::from_str_radix(part, 16).map_err(|_| format!("router: not {how}"))?;
    }
    if mac == [0; 6] {
        return Err(format!("router: still the placeholder; write {how}"));
    }
    Ok(mac)
}

/// Sets the home router, `machines.yaml`'s `router`, once at startup.
pub fn set_router(router: &str) -> Result<(), String> {
    let router = parse_mac(router)?;
    HOME.set(Home { router, last: Mutex::new(None) }).ok();
    Ok(())
}

/// Whether the default gateway is the home router. With no router set (the tests), at home. A
/// change is printed once.
pub fn at_home() -> bool {
    let Some(home) = HOME.get() else { return true };
    let mut last = home.last.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some((at, answer)) = *last {
        if at.elapsed() < RECHECK {
            return answer;
        }
    }
    let answer = gateway_mac() == Some(home.router);
    if last.map(|(_, before)| before) != Some(answer) {
        if answer {
            log::info!("on the home network: the other machines are reached");
        } else {
            log::info!("away from the home network: this machine alone, on 127.0.0.1");
        }
    }
    *last = Some((Instant::now(), answer));
    answer
}

/// The MAC address of the gateway of the IPv4 route to the Internet; `None` with no such route, a
/// route with no gateway, or a gateway that does not answer.
fn gateway_mac() -> Option<[u8; 6]> {
    use windows_sys::Win32::NetworkManagement::IpHelper::{GetBestRoute2, ResolveIpNetEntry2, MIB_IPFORWARD_ROW2, MIB_IPNET_ROW2};
    use windows_sys::Win32::Networking::WinSock::{AF_INET, SOCKADDR_INET};

    // SAFETY: these are plain C records, valid all-zero; the union fields written and read are the
    // IPv4 ones, the family set to AF_INET, and every pointer passed is to a live local.
    unsafe {
        let mut destination: SOCKADDR_INET = std::mem::zeroed();
        destination.Ipv4.sin_family = AF_INET;
        // An address on the Internet, 1.1.1.1, so the best route is the default one. Nothing is sent.
        destination.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes([1, 1, 1, 1]);
        let mut route: MIB_IPFORWARD_ROW2 = std::mem::zeroed();
        let mut source: SOCKADDR_INET = std::mem::zeroed();
        if GetBestRoute2(std::ptr::null(), 0, std::ptr::null(), &destination, 0, &mut route, &mut source) != 0 {
            return None;
        }
        // A gateway of 0.0.0.0 is a network reached directly, with no router.
        if route.NextHop.Ipv4.sin_addr.S_un.S_addr == 0 {
            return None;
        }
        let mut row: MIB_IPNET_ROW2 = std::mem::zeroed();
        row.Address = route.NextHop;
        row.InterfaceIndex = route.InterfaceIndex;
        row.InterfaceLuid = route.InterfaceLuid;
        if ResolveIpNetEntry2(&mut row, std::ptr::null()) != 0 || row.PhysicalAddressLength != 6 {
            return None;
        }
        row.PhysicalAddress[..6].try_into().ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_mac_is_read_with_dashes_or_colons_in_either_case_and_the_placeholder_refused() {
        let mac = [0x0a, 0xbc, 0x00, 0x12, 0xef, 0xff];
        assert_eq!(parse_mac("0a-bc-00-12-ef-ff"), Ok(mac));
        assert_eq!(parse_mac(" 0A:BC:00:12:EF:FF\n"), Ok(mac));
        for bad in ["", "0a-bc-00-12-ef", "0a-bc-00-12-ef-ff-01", "0a-bc-00-12-ef-fg", "a-bc-00-12-ef-fff", "0abc0012efff"] {
            assert!(parse_mac(bad).is_err(), "{bad:?}");
        }
        assert!(parse_mac("00-00-00-00-00-00").unwrap_err().contains("placeholder"));
    }
}
