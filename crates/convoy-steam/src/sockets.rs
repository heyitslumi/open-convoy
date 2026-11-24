use std::ffi::c_void;

use std::sync::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionEvent {
    Connecting(u32, u32),
    Connected(u32),
    Closed(u32),
}

static CONNECTION_EVENTS: Mutex<Vec<ConnectionEvent>> = Mutex::new(Vec::new());

unsafe extern "C" fn on_steam_net_connection_status_changed(ptr: *mut c_void) {
    if ptr.is_null() { return; }
    let raw = std::slice::from_raw_parts(ptr as *const u8, 256);
    let h_conn = u32::from_le_bytes([raw[0], raw[1], raw[2], raw[3]]);
    // Linux SteamNetworkingSockets012: offsetof(callback.m_info.m_eState) is 180.
    let e_state = i32::from_ne_bytes(raw[180..184].try_into().unwrap());
    eprintln!("[STEAM STATUS CALLBACK] h_conn={h_conn} e_state={e_state}");
    if let Ok(mut evs) = CONNECTION_EVENTS.lock() {
        match e_state {
            1 => evs.push(ConnectionEvent::Connecting(h_conn, u32::from_ne_bytes(raw[148..152].try_into().unwrap()))),
            3 => evs.push(ConnectionEvent::Connected(h_conn)),
            4 | 5 => evs.push(ConnectionEvent::Closed(h_conn)),
            _ => {}
        }
    }
}

use std::sync::Arc;
use bytes::Bytes;

use crate::gameserver::SteamApiSymbols;
use crate::SteamError;

pub const SEND_UNRELIABLE: i32 = 0;
pub const SEND_RELIABLE: i32 = 8;
pub const SEND_NO_NAGLE: i32 = 1;

#[repr(C)]
struct RawSteamNetworkingMessage {
    data: *mut c_void,
    size: i32,
    conn: u32,
    identity_peer_type: i32,
    identity_peer_buf: [u8; 132],
    conn_user_data: i64,
    usec_time_received: i64,
    message_number: i64,
    free_data: Option<unsafe extern "C" fn(msg: *mut RawSteamNetworkingMessage)>,
    release: Option<unsafe extern "C" fn(msg: *mut RawSteamNetworkingMessage)>,
    channel: i32,
    flags: i32,
    user_data: i64,
    idx_lane: u16,
    _pad1: u16,
}

#[derive(Debug, Clone)]
pub struct SteamMessage {
    pub conn: u32,
    pub payload: Bytes,
    pub peer_id: Option<u64>,
}

pub struct SteamSockets {
    symbols: Arc<SteamApiSymbols>,
    sockets_iface: *mut c_void,
    poll_group: u32,
}

unsafe impl Send for SteamSockets {}
unsafe impl Sync for SteamSockets {}

impl SteamSockets {
    pub(crate) fn new(symbols: Arc<SteamApiSymbols>, sockets_iface: *mut c_void, utils_iface: *mut c_void) -> Self {
        let poll_group = unsafe {
            (symbols.create_poll_group_fn)(sockets_iface)
        };

        if let Some(set_cb) = symbols.set_status_callback_fn {
            if !utils_iface.is_null() {
                eprintln!("[STEAM SOCKETS] Setting global status callback on {utils_iface:?}...");
                unsafe {
                    let ok = set_cb(utils_iface, on_steam_net_connection_status_changed);
                    eprintln!("[STEAM SOCKETS] SetGlobalCallback result: {ok}");
                }
            } else {
                eprintln!("[STEAM SOCKETS] utils_iface is NULL!");
            }
        } else {
            eprintln!("[STEAM SOCKETS] set_status_callback_fn is NONE!");
        }
        Self {
            symbols,
            sockets_iface,
            poll_group,
        }
    }

    pub fn create_listen_socket_p2p(&self, vport: i32) -> u32 {
        #[repr(C)]
        struct PointerOption {
            key: i32,
            kind: i32,
            value: unsafe extern "C" fn(*mut c_void),
        }
        let callback = PointerOption {
            key: 201, // k_ESteamNetworkingConfig_Callback_ConnectionStatusChanged
            kind: 5,  // k_ESteamNetworkingConfig_Ptr
            value: on_steam_net_connection_status_changed,
        };
        unsafe {
            (self.symbols.create_listen_socket_p2p_fn)(
                self.sockets_iface,
                vport,
                1,
                (&callback as *const PointerOption).cast(),
            )
        }
    }

    pub fn create_listen_socket_ip(&self, port: u16) -> u32 {
        let mut addr_buf = [0u8; 18];
        addr_buf[16..18].copy_from_slice(&port.to_be_bytes());
        unsafe {
            (self.symbols.create_listen_socket_ip_fn)(
                self.sockets_iface,
                addr_buf.as_ptr() as *const c_void,
                0,
                std::ptr::null(),
            )
        }
    }

    pub fn set_connection_poll_group(&self, conn: u32) -> bool {
        unsafe {
            (self.symbols.set_connection_poll_group_fn)(
                self.sockets_iface,
                conn,
                self.poll_group,
            )
        }
    }

    pub fn poll_group_handle(&self) -> u32 {
        self.poll_group
    }

    pub(crate) fn run_callbacks(&self) {
        unsafe { (self.symbols.run_socket_callbacks_fn)(self.sockets_iface) }
    }

    pub fn poll_incoming(&self) -> Vec<SteamMessage> {
        const MAX_MESSAGES: usize = 32;
        let mut raw_ptrs: [*mut c_void; MAX_MESSAGES] = [std::ptr::null_mut(); MAX_MESSAGES];

        let count = unsafe {
            (self.symbols.receive_messages_on_poll_group_fn)(
                self.sockets_iface,
                self.poll_group,
                raw_ptrs.as_mut_ptr(),
                MAX_MESSAGES as i32,
            )
        };

        if count <= 0 {
            return Vec::new();
        }

        let mut messages = Vec::with_capacity(count as usize);

        for i in 0..count as usize {
            let msg_ptr = raw_ptrs[i] as *mut RawSteamNetworkingMessage;
            if msg_ptr.is_null() {
                continue;
            }

            unsafe {
                let raw_msg = &*msg_ptr;
                let payload = if !raw_msg.data.is_null() && raw_msg.size > 0 {
                    let slice = std::slice::from_raw_parts(
                        raw_msg.data as *const u8,
                        raw_msg.size as usize,
                    );
                    Bytes::copy_from_slice(slice)
                } else {
                    Bytes::new()
                };

                messages.push(SteamMessage {
                    conn: raw_msg.conn,
                    peer_id: (raw_msg.identity_peer_type == 16 && raw_msg.identity_peer_buf[..4] == 8i32.to_ne_bytes())
                        .then(|| u64::from_ne_bytes(raw_msg.identity_peer_buf[4..12].try_into().unwrap())),
                    payload,
                });

                if let Some(release_fn) = raw_msg.release {
                    release_fn(msg_ptr);
                }
            }
        }

        messages
    }

    pub fn send(&self, conn: u32, data: &[u8], reliable: bool) -> Result<(), SteamError> {
        let flags = if reliable {
            SEND_RELIABLE | SEND_NO_NAGLE
        } else {
            SEND_UNRELIABLE | SEND_NO_NAGLE
        };

        let mut out_msg_num: i64 = 0;
        let result = unsafe {
            (self.symbols.send_message_to_connection_fn)(
                self.sockets_iface,
                conn,
                data.as_ptr() as *const c_void,
                data.len() as u32,
                flags,
                &mut out_msg_num,
            )
        };

        // EResult 1 is k_EResultOK
        if result == 1 {
            Ok(())
        } else {
            Err(SteamError::SocketError(format!("SendMessage failed with EResult {result}")))
        }
    }

    pub fn accept_connection(&self, conn: u32) -> Result<(), SteamError> {
        let res = unsafe { (self.symbols.accept_connection_fn)(self.sockets_iface, conn) };
        if res == 1 {
            Ok(())
        } else {
            Err(SteamError::SocketError(format!("AcceptConnection failed with result {res}")))
        }
    }

    pub fn poll_connection_events(&self) -> Vec<ConnectionEvent> {
        if let Ok(mut evs) = CONNECTION_EVENTS.lock() {
            std::mem::take(&mut *evs)
        } else {
            Vec::new()
        }
    }

    pub fn close(&self, conn: u32) {
        unsafe {
            (self.symbols.close_connection_fn)(
                self.sockets_iface,
                conn,
                0,
                std::ptr::null(),
                false,
            );
        }
    }
}
