//! Wi-Fi security of an access point (upstream network/nm/utils.cpp, itself from NetworkManager's
//! nm-utils): the strongest type both the AP and the device's capabilities allow.

// NMWirelessCapabilities
const CIPHER_WEP40: u32 = 0x1;
const CIPHER_WEP104: u32 = 0x2;
const CIPHER_TKIP: u32 = 0x4;
const CIPHER_CCMP: u32 = 0x8;
const CAP_WPA: u32 = 0x10;
const CAP_RSN: u32 = 0x20;
const CAP_IBSS_RSN: u32 = 0x2000;
// NM80211ApFlags
const PRIVACY: u32 = 0x1;
// NM80211ApSecurityFlags
const PAIR_WEP40: u32 = 0x1;
const PAIR_WEP104: u32 = 0x2;
const PAIR_TKIP: u32 = 0x4;
const PAIR_CCMP: u32 = 0x8;
const GROUP_WEP40: u32 = 0x10;
const GROUP_WEP104: u32 = 0x20;
const GROUP_TKIP: u32 = 0x40;
const GROUP_CCMP: u32 = 0x80;
const KEY_PSK: u32 = 0x100;
const KEY_8021X: u32 = 0x200;
const KEY_SAE: u32 = 0x400;
const KEY_OWE: u32 = 0x800;
const KEY_OWE_TM: u32 = 0x1000;
const KEY_SUITE_B_192: u32 = 0x2000;

/// WifiSecurityType values, strongest first.
pub const WPA3_SUITE_B_192: i32 = 0;
pub const SAE: i32 = 1;
pub const WPA2_EAP: i32 = 2;
pub const WPA2_PSK: i32 = 3;
pub const WPA_EAP: i32 = 4;
pub const WPA_PSK: i32 = 5;
pub const STATIC_WEP: i32 = 6;
pub const DYNAMIC_WEP: i32 = 7;
pub const LEAP: i32 = 8;
pub const OWE: i32 = 9;
pub const OPEN: i32 = 10;
pub const UNKNOWN: i32 = 11;

fn supports_ciphers(caps: u32, ap: u32, kind: i32) -> bool {
    let has = |c: u32, f: u32| caps & c != 0 && ap & f != 0;
    let pair = kind == STATIC_WEP
        || has(CIPHER_WEP40, PAIR_WEP40)
        || has(CIPHER_WEP104, PAIR_WEP104)
        || has(CIPHER_TKIP, PAIR_TKIP)
        || has(CIPHER_CCMP, PAIR_CCMP);
    let group = has(CIPHER_WEP40, GROUP_WEP40)
        || has(CIPHER_WEP104, GROUP_WEP104)
        || (kind != STATIC_WEP && (has(CIPHER_TKIP, GROUP_TKIP) || has(CIPHER_CCMP, GROUP_CCMP)));
    pair && group
}

fn psk_ciphers(caps: u32, flags: u32) -> bool {
    (flags & PAIR_TKIP != 0 && caps & CIPHER_TKIP != 0) || (flags & PAIR_CCMP != 0 && caps & CIPHER_CCMP != 0)
}

fn valid(kind: i32, caps: u32, adhoc: bool, flags: u32, wpa: u32, rsn: u32) -> bool {
    let wep = |caps, wpa, rsn| {
        flags & PRIVACY != 0
            && (wpa == 0 && rsn == 0 || supports_ciphers(caps, wpa, STATIC_WEP) || supports_ciphers(caps, rsn, STATIC_WEP))
    };
    let rsn_psk = |key: u32| {
        caps & CAP_RSN != 0
            && if adhoc {
                caps & CAP_IBSS_RSN != 0 && rsn & PAIR_CCMP != 0 && caps & CIPHER_CCMP != 0
            } else {
                rsn & key != 0 && psk_ciphers(caps, rsn)
            }
    };
    match kind {
        OPEN => flags & PRIVACY == 0 && wpa == 0 && rsn == 0,
        // Upstream falls through from LEAP into the static WEP checks.
        LEAP => !adhoc && wep(caps, wpa, rsn),
        STATIC_WEP => wep(caps, wpa, rsn),
        DYNAMIC_WEP => {
            !adhoc && rsn == 0 && flags & PRIVACY != 0 && (wpa == 0 || (wpa & KEY_8021X != 0 && supports_ciphers(caps, wpa, DYNAMIC_WEP)))
        }
        WPA_PSK => !adhoc && caps & CAP_WPA != 0 && wpa & KEY_PSK != 0 && psk_ciphers(caps, wpa),
        WPA2_PSK => rsn_psk(KEY_PSK),
        SAE => rsn_psk(KEY_SAE),
        WPA_EAP => !adhoc && caps & CAP_WPA != 0 && wpa & KEY_8021X != 0 && supports_ciphers(caps, wpa, WPA_EAP),
        WPA2_EAP => !adhoc && caps & CAP_RSN != 0 && rsn & KEY_8021X != 0 && supports_ciphers(caps, rsn, WPA2_EAP),
        OWE => !adhoc && caps & CAP_RSN != 0 && rsn & (KEY_OWE | KEY_OWE_TM) != 0,
        WPA3_SUITE_B_192 => !adhoc && caps & CAP_RSN != 0 && rsn & KEY_SUITE_B_192 != 0,
        _ => false,
    }
}

/// upstream findBestWirelessSecurity
pub fn best(caps: u32, adhoc: bool, flags: u32, wpa: u32, rsn: u32) -> i32 {
    (WPA3_SUITE_B_192..=OPEN).find(|&k| valid(k, caps, adhoc, flags, wpa, rsn)).unwrap_or(UNKNOWN)
}

#[cfg(test)]
mod tests {
    use super::*;

    const CAPS: u32 = CIPHER_WEP40 | CIPHER_WEP104 | CIPHER_TKIP | CIPHER_CCMP | CAP_WPA | CAP_RSN;
    const CCMP: u32 = PAIR_CCMP | GROUP_CCMP;

    #[test]
    fn common_networks() {
        assert_eq!(best(CAPS, false, 0, 0, 0), OPEN);
        assert_eq!(best(CAPS, false, PRIVACY, 0, CCMP | KEY_PSK), WPA2_PSK);
        assert_eq!(best(CAPS, false, PRIVACY, CCMP | KEY_PSK, CCMP | KEY_PSK), WPA2_PSK);
        assert_eq!(best(CAPS, false, PRIVACY, CCMP | KEY_PSK, 0), WPA_PSK);
        assert_eq!(best(CAPS, false, PRIVACY, 0, CCMP | KEY_SAE | KEY_PSK), SAE);
        assert_eq!(best(CAPS, false, PRIVACY, 0, CCMP | KEY_8021X), WPA2_EAP);
        assert_eq!(best(CAPS, false, 0, 0, CCMP | KEY_OWE), OWE);
        assert_eq!(best(CAPS, false, PRIVACY, 0, 0), STATIC_WEP);
    }

    #[test]
    fn device_without_rsn_cannot_use_wpa2() {
        assert_eq!(best(CAPS & !CAP_RSN, false, PRIVACY, 0, CCMP | KEY_PSK), UNKNOWN);
    }
}
