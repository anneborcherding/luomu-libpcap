#![deny(
    future_incompatible,
    nonstandard_style,
    rust_2018_compatibility,
    rust_2018_idioms,
    rustdoc,
    unused,
    missing_docs
)]

//! # luomu-libpcap
//!
//! Safe and mostly sane Rust bindings for [libpcap](https://www.tcpdump.org/).
//!
//! We are split in two different crates:
//!
//!   * `luomu-libpcap-sys` for unsafe Rust bindings generated directly from
//!     `libpcap`.
//!   * `luomu-libpcap` for safe and sane libpcap interface.
//!
//! `luomu-libpcap` crate is split into two parts itself:
//!
//!   * `functions` module contains safe wrappers and sane return values for
//!     libpcap functions.
//!   * the root of the project contains `Pcap` struct et al. for more Rusty API
//!     to interact with libpcap.
//!
//! You probably want to use the `Pcap` struct and other things from root of
//! this crate.

use std::collections::{BTreeSet, HashSet};
use std::convert::TryFrom;
use std::default;
use std::fmt;
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};
use std::ops::Deref;
use std::rc::Rc;
use std::result;

use luomu_libpcap_sys as libpcap;

pub mod functions;
use functions::*;

mod error;
pub use error::Error;

/// A `Result` wrapping luomu-libpcap's errors in `Err` side
pub type Result<T> = result::Result<T, Error>;

/// Keeper of the `libpcap`'s `pcap_t`.
pub struct PcapT {
    pcap_t: *mut libpcap::pcap_t,
    #[allow(dead_code)]
    errbuf: Vec<u8>,
}

impl PcapT {
    /// get libpcap error message text
    ///
    /// `get_error()` returns the error pertaining to the last pcap library error.
    ///
    /// This function can also fail, how awesome is that? The `Result` of
    /// `Ok(Error)` contains the error from libpcap as intended. `Err(Error)`
    /// contains the error happened while calling this function.
    pub fn get_error(&self) -> Result<Error> {
        get_error(&self)
    }
}

impl Drop for PcapT {
    fn drop(&mut self) {
        log::trace!("PcapT::drop({:p})", self.pcap_t);
        unsafe { luomu_libpcap_sys::pcap_close(self.pcap_t) }
    }
}

/// Pcap capture
///
/// This contains everything needed to capture the packets from network.
///
/// To get started use `Pcap::builder()` to start a new Pcap capture builder.
/// Use it to set required options for the capture and then call
/// `PcapBuider::activate()` to activate the capture.
///
/// Then `Pcap::capture()` can be used to start an iterator for capturing
/// packets.
pub struct Pcap {
    pcap_t: PcapT,
}

impl Pcap {
    /// Create a live capture handle
    ///
    /// This is used to create a packet capture handle to look at packets on the
    /// network. `source` is a string that specifies the network device to open.
    pub fn new(source: &str) -> Result<Pcap> {
        let pcap_t = pcap_create(source)?;
        Ok(Pcap { pcap_t })
    }

    /// Use builder to create a live capture handle
    ///
    /// This is used to create a packet capture handle to look at packets on the
    /// network. source is a string that specifies the network device to open.
    pub fn builder(source: &str) -> Result<PcapBuilder> {
        let pcap_t = pcap_create(source)?;
        Ok(PcapBuilder { pcap_t })
    }

    /// set a filter expression
    ///
    /// `Set a filter for capture. See
    /// [pcap-filter(7)](https://www.tcpdump.org/manpages/pcap-filter.7.html)
    /// for the syntax of that string.
    pub fn set_filter(&self, filter: &str) -> Result<()> {
        let mut bpf_program = PcapFilter::compile(&self.pcap_t, filter)?;
        pcap_setfilter(&self.pcap_t, &mut bpf_program)
    }

    /// Start capturing packets
    ///
    /// This returns an iterator `PcapIter` which can be used to get captured
    /// packets.
    pub fn capture(&self) -> PcapIter<'_> {
        PcapIter::new(&self.pcap_t)
    }

    /// Transmit a packet
    pub fn inject(&self, buf: &[u8]) -> Result<usize> {
        pcap_inject(&self.pcap_t, buf)
    }

    /// activate a capture
    ///
    /// This is used to activate a packet capture to look at packets on the
    /// network, with the options that were set on the handle being in effect.
    pub fn activate(&self) -> Result<()> {
        pcap_activate(&self.pcap_t)
    }

    /// get capture statistics
    ///
    /// Returns statistics from current capture. The values represent packet
    /// statistics from the start of the run to the time of the call.
    pub fn stats(&self) -> Result<PcapStat> {
        let mut stats: PcapStat = Default::default();
        match pcap_stats(&self.pcap_t, &mut stats) {
            Ok(()) => Ok(stats),
            Err(e) => Err(e),
        }
    }
}

impl Deref for Pcap {
    type Target = PcapT;

    fn deref(&self) -> &Self::Target {
        &self.pcap_t
    }
}

/// Builder for a `Pcap`. Call `Pcap::builder()` to get started.
pub struct PcapBuilder {
    pcap_t: PcapT,
}

impl PcapBuilder {
    /// set the buffer size for a capture
    ///
    /// `set_buffer_size()` sets the buffer size that will be used on a capture
    /// handle when the handle is activated to buffer_size, which is in units of
    /// bytes.
    pub fn set_buffer_size(self, buffer_size: usize) -> Result<PcapBuilder> {
        pcap_set_buffer_size(&self.pcap_t, buffer_size)?;
        Ok(self)
    }

    /// set promiscuous mode for a capture
    ///
    /// `set_promisc()` sets whether promiscuous mode should be set on a capture
    /// handle when the handle is activated.
    pub fn set_promiscuous(self, promiscuous: bool) -> Result<PcapBuilder> {
        pcap_set_promisc(&self.pcap_t, promiscuous)?;
        Ok(self)
    }

    /// set immediate mode for a capture
    ///
    /// `set_immediate_mode()` sets whether immediate mode should be set on a
    /// capture handle when the handle is activated. In immediate mode, packets
    /// are always delivered as soon as they arrive, with no buffering.
    pub fn set_immediate(self, immediate: bool) -> Result<PcapBuilder> {
        pcap_set_immediate_mode(&self.pcap_t, immediate)?;
        Ok(self)
    }

    /// set the snapshot length for a capture
    ///
    /// `set_snaplen()` sets the snapshot length to be used on a capture handle
    /// when the handle is activated to snaplen.
    ///
    /// `libpcap` says 65535 bytes should be enough for everyone.
    pub fn set_snaplen(self, snaplen: usize) -> Result<PcapBuilder> {
        pcap_set_snaplen(&self.pcap_t, snaplen)?;
        Ok(self)
    }

    /// activate a capture
    ///
    /// `activate()` is used to activate a packet capture to look at packets on
    /// the network, with the options that were set on the handle being in
    /// effect.
    pub fn activate(self) -> Result<Pcap> {
        pcap_activate(&self.pcap_t)?;
        Ok(Pcap {
            pcap_t: self.pcap_t,
        })
    }
}

/// A BPF filter program for Pcap.
pub struct PcapFilter {
    bpf_program: libpcap::bpf_program,
}

impl PcapFilter {
    /// compile a filter expression
    ///
    /// `compile()` is used to compile the filter into a filter program. See
    /// [pcap-filter(7)](https://www.tcpdump.org/manpages/pcap-filter.7.html)
    /// for the syntax of that string.
    pub fn compile(pcap_t: &PcapT, filter_str: &str) -> Result<PcapFilter> {
        pcap_compile(pcap_t, filter_str)
    }
}

impl Drop for PcapFilter {
    fn drop(&mut self) {
        log::trace!("PcapFilter::drop({:p})", &self.bpf_program);
        unsafe { luomu_libpcap_sys::pcap_freecode(&mut self.bpf_program) }
    }
}

/// Pcap cature iterator
pub struct PcapIter<'p> {
    pcap_t: &'p PcapT,
}

impl<'p> PcapIter<'p> {
    fn new(pcap_t: &'p PcapT) -> Self {
        PcapIter { pcap_t }
    }
}

impl<'p> Iterator for PcapIter<'p> {
    type Item = Packet<'p>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            match pcap_next_ex(&self.pcap_t) {
                Ok(p) => return Some(p),
                Err(e) => match e {
                    // pcap_next_ex() sometimes seems to return
                    // "packet buffer expired" (whatever that means),
                    // even if the immediate mode is set. Just retry in
                    // this case.
                    Error::Timeout => continue,
                    _ => return None,
                },
            }
        }
    }
}

/// Pcap capture statistics
pub struct PcapStat {
    stats: libpcap::pcap_stat,
}

impl default::Default for PcapStat {
    fn default() -> Self {
        PcapStat {
            stats: libpcap::pcap_stat {
                ps_recv: 0,
                ps_drop: 0,
                ps_ifdrop: 0,
            },
        }
    }
}

impl PcapStat {
    /// Return number of packets received.
    pub fn packets_received(&self) -> u32 {
        self.stats.ps_recv
    }

    /// Return number of packets dropped because there was no room in the
    /// operating system's buffer when they arrived, because packets weren't
    /// being read fast enough.
    pub fn packets_dropped(&self) -> u32 {
        self.stats.ps_drop
    }

    /// Return number of packets dropped by the network interface or its driver.
    pub fn packets_dropped_interface(&self) -> u32 {
        self.stats.ps_ifdrop
    }
}

/// A network packet captured by libpcap.
pub enum Packet<'p> {
    /// Borrowed content is wrapped into `Rc` to advice compiler that `Packet`
    /// is not `Sync` nor `Send`. This is done because `libpcap` owns the
    /// borrowed memory and next call to `pcap_next_ex` could change the
    /// contents.
    Borrowed(Rc<&'p [u8]>),
    /// Owned version of the packet. The contents have been explicitely copied
    /// so it's safe to call `pcap_next_ex()` again.
    Owned(Vec<u8>),
}

impl<'p> Packet<'p> {
    /// Make `Packet` from slice
    pub fn from_slice(buf: &'p [u8]) -> Packet<'p> {
        Packet::Borrowed(Rc::new(buf))
    }

    /// Clone the packet
    pub fn to_vec(&self) -> Vec<u8> {
        match self {
            Packet::Borrowed(packet) => packet.to_vec(),
            Packet::Owned(packet) => packet.clone(),
        }
    }
}

impl<'p> ToOwned for Packet<'p> {
    type Owned = Packet<'p>;

    fn to_owned(&self) -> Self {
        Packet::Owned(self.to_vec())
    }
}

impl<'p> AsRef<[u8]> for Packet<'p> {
    fn as_ref(&self) -> &[u8] {
        self.deref()
    }
}

impl<'p> Deref for Packet<'p> {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Packet::Borrowed(packet) => packet,
            Packet::Owned(packet) => packet.as_ref(),
        }
    }
}

/// Keeper of the `libpcap`'s `pcap_if_t`.
pub struct PcapIfT {
    pcap_if_t: *mut libpcap::pcap_if_t,
}

impl PcapIfT {
    /// get a list of capture devices
    ///
    /// Constructs a list of network devices that can be opened with
    /// `Pcap::new()` and `Pcap::builder()`. Note that there may be network
    /// devices that cannot be opened by the process calling, because, for
    /// example, that process does not have sufficient privileges to open them
    /// for capturing; if so, those devices will not appear on the list.
    pub fn new() -> Result<Self> {
        pcap_findalldevs()
    }

    /// Return iterator for iterating capture devices.
    pub fn iter(&self) -> InterfaceIter {
        InterfaceIter {
            start: self.pcap_if_t,
            next: Some(self.pcap_if_t),
        }
    }

    /// Get all capture devices.
    pub fn get_interfaces(&self) -> HashSet<Interface> {
        self.iter().collect()
    }

    /// Find capture device with interface name `name`.
    pub fn find_interface_with_name(&self, name: &str) -> Option<Interface> {
        for interface in self.get_interfaces() {
            if interface.has_name(name) {
                log::trace!("find_interface_with_name({}) = {:?}", name, interface);
                return Some(interface);
            }
        }
        None
    }

    /// Find capture device which have IP address `ip`.
    pub fn find_interface_with_ip(&self, ip: &IpAddr) -> Option<String> {
        for interface in self.get_interfaces() {
            if interface.has_address(ip) {
                log::trace!("find_interface_with_ip({}) = {:?}", ip, interface);
                return Some(interface.name);
            }
        }
        None
    }
}

impl Drop for PcapIfT {
    fn drop(&mut self) {
        log::trace!("PcapIfT::drop({:?})", self.pcap_if_t);
        unsafe { luomu_libpcap_sys::pcap_freealldevs(self.pcap_if_t) }
    }
}

/// A network device that can be opened with `Pcap::new()` and
/// `Pcap::builder()`.
#[derive(Debug, PartialEq, Eq, Hash)]
pub struct Interface {
    /// Devices name
    pub name: String,
    /// Devices description
    pub description: Option<String>,
    /// All addresses found from device
    pub addresses: BTreeSet<InterfaceAddress>,
    /// Flags set for device
    pub flags: BTreeSet<InterfaceFlag>,
}

impl Interface {
    /// True if interface is up
    pub fn is_up(&self) -> bool {
        self.flags.get(&InterfaceFlag::Up).is_some()
    }

    /// True if interface is running
    pub fn is_running(&self) -> bool {
        self.flags.get(&InterfaceFlag::Running).is_some()
    }

    /// True if interface is loopback
    pub fn is_loopback(&self) -> bool {
        self.flags.get(&InterfaceFlag::Loopback).is_some()
    }

    /// True if interface is has name `name`
    pub fn has_name(&self, name: &str) -> bool {
        self.name == name
    }

    /// Return MAC aka Ethernet address of the interface
    pub fn get_ether_address(&self) -> Option<MacAddr> {
        for ia in &self.addresses {
            if let Address::Mac(addr) = ia.addr {
                return Some(addr);
            }
        }
        None
    }

    /// Return IP addresses of interface
    pub fn get_ip_addresses(&self) -> HashSet<IpAddr> {
        self.addresses
            .iter()
            .filter_map(|i| IpAddr::try_from(&i.addr).ok())
            .collect()
    }

    /// True if interface is has IP address `ip`
    pub fn has_address(&self, ip: &IpAddr) -> bool {
        self.get_ip_addresses().get(ip).is_some()
    }
}

/// Interface iterator
///
/// Iterates all capture interfaces.
pub struct InterfaceIter {
    // First item in linked list, only used for trace logging
    start: *mut libpcap::pcap_if_t,
    // Next item in linked list, used for iteration
    next: Option<*mut libpcap::pcap_if_t>,
}

impl Iterator for InterfaceIter {
    type Item = Interface;

    fn next(&mut self) -> Option<Interface> {
        log::trace!(
            "InterfaceIter(start: {:p}, next: {:p})",
            self.start,
            self.next.unwrap_or(std::ptr::null_mut())
        );

        let pcap_if_t = self.next?;
        if pcap_if_t.is_null() {
            self.next = None;
            return None;
        }

        let next = unsafe { (*pcap_if_t).next };
        if next.is_null() {
            self.next = None;
        } else {
            self.next = Some(next);
        }

        match try_interface_from(pcap_if_t) {
            Ok(dev) => Some(dev),
            Err(err) => {
                log::error!("try_interface_from{:p}: {}", pcap_if_t, err);
                None
            }
        }
    }
}

/// Address of some sort. IPv4, IPv6, MAC.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Address {
    /// IPv4 address
    Ipv4(Ipv4Addr),
    /// IPv6 address
    Ipv6(Ipv6Addr),
    /// MAC address
    Mac(MacAddr),
}

impl Address {
    /// True if IPv4 address
    pub fn is_ipv4(&self) -> bool {
        match self {
            Address::Ipv4(_) => true,
            _ => false,
        }
    }

    /// True if IPv6 address
    pub fn is_ipv6(&self) -> bool {
        match self {
            Address::Ipv6(_) => true,
            _ => false,
        }
    }

    /// True if either IPv4 or IPv6 address
    pub fn is_ip(&self) -> bool {
        self.is_ipv4() || self.is_ipv6()
    }

    /// True if MAC address
    pub fn is_mac(&self) -> bool {
        match self {
            Address::Mac(_) => true,
            _ => false,
        }
    }

    /// Return the `Ipv4Addr` or None
    pub fn as_ipv4(&self) -> Option<Ipv4Addr> {
        match self {
            Address::Ipv4(ip) => Some(*ip),
            _ => None,
        }
    }

    /// Return the `Ipv6Addr` or None
    pub fn as_ipv6(&self) -> Option<Ipv6Addr> {
        match self {
            Address::Ipv6(ip) => Some(*ip),
            _ => None,
        }
    }

    /// Return the `IpAddr` or None
    pub fn as_ip(&self) -> Option<IpAddr> {
        match self {
            Address::Ipv4(ip) => Some((*ip).into()),
            Address::Ipv6(ip) => Some((*ip).into()),
            _ => None,
        }
    }

    /// Return the `MacAddr` or None
    pub fn as_mac(&self) -> Option<MacAddr> {
        match self {
            Address::Mac(mac) => Some(*mac),
            _ => None,
        }
    }
}

impl From<Ipv4Addr> for Address {
    fn from(ip: Ipv4Addr) -> Self {
        Address::Ipv4(ip)
    }
}

impl From<Ipv6Addr> for Address {
    fn from(ip: Ipv6Addr) -> Self {
        Address::Ipv6(ip)
    }
}

impl From<IpAddr> for Address {
    fn from(ip: IpAddr) -> Self {
        match ip {
            IpAddr::V4(ip) => Address::Ipv4(ip),
            IpAddr::V6(ip) => Address::Ipv6(ip),
        }
    }
}

impl From<MacAddr> for Address {
    fn from(mac: MacAddr) -> Self {
        Address::Mac(mac)
    }
}

impl From<[u8; 6]> for Address {
    fn from(mac: [u8; 6]) -> Self {
        Address::Mac(MacAddr::from(mac))
    }
}

impl From<&[u8; 6]> for Address {
    fn from(mac: &[u8; 6]) -> Self {
        Address::Mac(MacAddr::from(mac))
    }
}

impl TryFrom<Address> for Ipv4Addr {
    type Error = Error;

    fn try_from(addr: Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv4(ip) => Ok(ip),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<Address> for Ipv6Addr {
    type Error = Error;

    fn try_from(addr: Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv6(ip) => Ok(ip),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<Address> for IpAddr {
    type Error = Error;

    fn try_from(addr: Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv4(ip) => Ok(ip.into()),
            Address::Ipv6(ip) => Ok(ip.into()),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<&Address> for Ipv4Addr {
    type Error = Error;

    fn try_from(addr: &Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv4(ip) => Ok(*ip),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<&Address> for Ipv6Addr {
    type Error = Error;

    fn try_from(addr: &Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv6(ip) => Ok(*ip),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<&Address> for IpAddr {
    type Error = Error;

    fn try_from(addr: &Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Ipv4(ip) => Ok((*ip).into()),
            Address::Ipv6(ip) => Ok((*ip).into()),
            _ => Err(Error::InvalidAddress),
        }
    }
}

impl TryFrom<Address> for MacAddr {
    type Error = Error;

    fn try_from(addr: Address) -> result::Result<Self, Self::Error> {
        match addr {
            Address::Mac(mac) => Ok(mac),
            _ => Err(Error::InvalidAddress),
        }
    }
}

/// Collection of addresses for network interface.
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct InterfaceAddress {
    /// Network interface's address
    addr: Address,
    /// The netmask corresponding to the address pointed to by addr.
    netmask: Option<Address>,
    /// The broadcast address corresponding to the address pointed to by addr;
    /// may be `None` if the device doesn't support broadcasts.
    broadaddr: Option<Address>,
    /// The destination address corresponding to the address pointed to by addr;
    /// may be `None` if the device isn't a point-to-point interface.
    dstaddr: Option<Address>,
}

/// Iterator for network device's addresses.
pub struct AddressIter {
    // First item in linked list, only used for trace logging
    start: *mut libpcap::pcap_addr_t,
    // Next item in linked list, used for iteration
    next: Option<*mut libpcap::pcap_addr_t>,
}

impl Iterator for AddressIter {
    type Item = InterfaceAddress;

    fn next(&mut self) -> Option<InterfaceAddress> {
        log::trace!(
            "AddressIter(start: {:p}, next: {:p})",
            self.start,
            self.next.unwrap_or(std::ptr::null_mut())
        );

        let pcap_addr_t = self.next?;
        if pcap_addr_t.is_null() {
            self.next = None;
            return None;
        }

        let next = unsafe { (*pcap_addr_t).next };
        if next.is_null() {
            self.next = None;
        } else {
            self.next = Some(next);
        }

        if let Some(dev) = try_address_from(pcap_addr_t) {
            Some(dev)
        } else {
            // Address was something we don't know how to handle. Move
            // to next address in list.
            self.next()
        }
    }
}

/// Various flags which can be set on network interface
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InterfaceFlag {
    /// set if the interface is a loopback interface
    Loopback,
    /// set if the interface is up
    Up,
    /// set if the interface is running
    Running,
}

/// A MAC address used for example with Ethernet
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct MacAddr([u8; 6]);

impl From<[u8; 6]> for MacAddr {
    fn from(val: [u8; 6]) -> Self {
        Self(val)
    }
}

impl From<&[u8; 6]> for MacAddr {
    fn from(val: &[u8; 6]) -> Self {
        Self(val.to_owned())
    }
}

impl fmt::Debug for MacAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let h = self
            .0
            .iter()
            .map(|b| format!("{:02x}", b))
            .collect::<Vec<_>>()
            .join(":");
        f.write_str(&h)
    }
}

impl std::ops::Deref for MacAddr {
    type Target = [u8; 6];
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

#[cfg(test)]
mod tests {
    use super::Packet;

    #[test]
    fn test_packet_to_owned() {
        let buf = vec![1, 2, 3, 4];
        let packet = Packet::from_slice(&buf);
        let owned = packet.to_owned();
        if let Packet::Owned(p) = owned {
            assert_eq!(p, buf);
        } else {
            panic!("Packet was not owned");
        }
    }
}
